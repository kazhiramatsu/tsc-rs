//! H2.5f ES2017 lowering for the ES2015 and ES2016 target boundaries.
//!
//! The observable contract follows `transformES2017`: async functions become
//! `__awaiter` calls whose continuation is a generator, while top-level await
//! remains owned by the module pipeline.  Rust keeps parameter forwarding,
//! lexical capture, and function-context state explicit instead of mirroring
//! the reference transform's mutable closure graph.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeCheckFlags, NodeFlags, ScriptTarget};

use crate::{
    factory::EmitHelperName, EmitFlags, EmitResolver, EmitResolverNode, LexicalEnvironment,
    TransformError, TransformFlags, TransformNode, TransformNodeArray, TransformRoot,
    TransformSourceId, TransformationContext, Transformer,
};

use super::{
    flags_after_update,
    generated_bindings::{AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes},
    initialize_transform_flags,
    target_bindings::{
        collect_untagged_identifier_texts, finalize_generated_binding_names, TargetBinding,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FunctionShape {
    Arrow,
    Ordinary,
}

#[derive(Debug)]
struct FunctionFrame {
    colliding_parameter_names: Option<BTreeSet<String>>,
}

#[derive(Clone, Debug)]
enum ForwardedArgument {
    Direct(TargetBinding),
    Spread(TargetBinding),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AsyncParameterFact {
    PlainIdentifier,
    RestIdentifier,
    RequiresInnerScope,
}

impl AsyncParameterFact {
    const fn is_simple(self) -> bool {
        matches!(self, Self::PlainIdentifier | Self::RestIdentifier)
    }
}

#[derive(Clone, Debug)]
enum AsyncLexicalArgumentsPlan {
    Unused,
    Inherited,
    Capture(TargetBinding),
}

impl AsyncLexicalArgumentsPlan {
    fn capture_binding(&self) -> Option<&TargetBinding> {
        match self {
            Self::Capture(binding) => Some(binding),
            Self::Unused | Self::Inherited => None,
        }
    }
}

#[derive(Debug)]
struct AsyncParameterPlan {
    outer: Option<NodeArrayId>,
    inner: Option<NodeArrayId>,
    forwarded_arguments: Vec<ForwardedArgument>,
}

#[derive(Clone, Debug)]
struct CapturedSuperProperty {
    text: String,
}

#[derive(Clone, Debug, Default)]
struct AsyncSuperCapture {
    binding: Option<TargetBinding>,
    index_binding: Option<TargetBinding>,
    properties: Vec<CapturedSuperProperty>,
    has_element_access: bool,
    has_assignment: bool,
    owns_access: bool,
}

struct TransformedFunction {
    modifiers: Option<NodeArrayId>,
    asterisk_token: Option<NodeId>,
    parameters: Option<NodeArrayId>,
    body: Option<NodeId>,
}

/// tsc-port: transformES2017 @6.0.3
/// tsc-hash: 07da9b5d92984c37fa3326cfc93d3d4c5af591ebdca4521f317c49ea51c02335
/// tsc-span: _tsc.js:100810-101560
pub(super) fn transform_es2017<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(Es2017Transformer {
        resolver,
        target: options.emit_script_target(),
        always_strict: options.always_strict_effective(),
    })
}

struct Es2017Transformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    target: ScriptTarget,
    always_strict: bool,
}

impl Transformer for Es2017Transformer<'_> {
    fn name(&self) -> &'static str {
        "transformES2017"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES2015 || self.target > ScriptTarget::ES2016 {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "transformES2017",
                detail: "H2.5g composes transformES2017 at the ES2015 and ES2016 target boundaries",
            });
        }
        Ok(())
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(root);
        }
        initialize_transform_flags(context.arena_mut()?, source)?;
        let current_root = context.arena().root(source)?;
        let source_has_lexical_this =
            !self.always_strict && !source_file_is_effectively_strict(context, current_root)?;
        let mut visitor = Es2017Visitor::new(
            context,
            source,
            self.resolver,
            current_root,
            source_has_lexical_this,
        )?;
        let transformed =
            visitor
                .visit(current_root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.node(transformed);
        if self.target >= ScriptTarget::ES2016 {
            finalize_generated_binding_names(visitor.context, source, transformed)?;
        }
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

fn source_file_is_effectively_strict(
    context: &TransformationContext,
    root: TransformNode,
) -> Result<bool, TransformError> {
    let syntax = context.arena().source(root.source())?.syntax();
    if syntax.external_module_indicator.is_some() {
        return Ok(true);
    }
    let NodeData::SourceFile(data) = &context.arena().node(root)?.data else {
        return Ok(false);
    };
    let Some(statements) = data
        .statements
        .and_then(|statements| context.arena().node_array_ref(root.source(), statements))
    else {
        return Ok(false);
    };
    for statement in &context.arena().node_array(statements)?.nodes {
        let Some(statement) = context.arena().node_ref(root.source(), *statement) else {
            continue;
        };
        let NodeData::ExpressionStatement(statement) = &context.arena().node(statement)?.data
        else {
            break;
        };
        let Some(expression) = statement
            .expression
            .and_then(|expression| context.arena().node_ref(root.source(), expression))
        else {
            break;
        };
        let NodeData::StringLiteral(literal) = &context.arena().node(expression)?.data else {
            break;
        };
        if literal.text == "use strict" {
            return Ok(true);
        }
    }
    Ok(false)
}

struct Es2017Visitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    generated_bindings: GeneratedBindingScopes,
    frames: Vec<FunctionFrame>,
    non_top_level_depth: usize,
    has_lexical_this: bool,
    lexical_arguments_binding: Option<TargetBinding>,
    super_captures: Vec<Option<AsyncSuperCapture>>,
}

impl<'context, 'resolver> Es2017Visitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        root: TransformNode,
        has_lexical_this: bool,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_untagged_identifier_texts(context.arena(), source, root)?,
                AncestorBindingPolicy::AllowShadow,
            ),
            context,
            source,
            resolver,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            frames: Vec::new(),
            non_top_level_depth: 0,
            has_lexical_this,
            lexical_arguments_binding: None,
            super_captures: Vec::new(),
        })
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();

        if matches!(record.data, NodeData::Identifier(_))
            && self.lexical_arguments_binding.is_some()
            && self
                .resolver
                .is_arguments_local_binding(self.resolver_node(original)?)?
        {
            let binding = self
                .lexical_arguments_binding
                .clone()
                .expect("lexical arguments binding was tested above");
            let identifier = self.create_generated_identifier(&binding)?;
            let identifier = self.set_original_and_range(identifier, original)?;
            self.nodes.insert(id, Some(identifier.node()));
            return Ok(Some(identifier.node()));
        }

        let direct_super_use =
            self.super_capture_is_active() && self.node_is_direct_super_use(original)?;
        let requires_full_walk =
            self.lexical_arguments_binding.is_some() || self.current_collision_names().is_some();
        if !direct_super_use
            && !requires_full_walk
            && !self
                .context
                .arena()
                .transform_flags(original)
                .contains(TransformFlags::CONTAINS_ES_2017)
        {
            self.nodes.insert(id, Some(id));
            return Ok(Some(id));
        }

        let transformed = match record.data {
            NodeData::CallExpression(data) if self.call_expression_targets_super(&data)? => {
                Some(self.visit_captured_super_call(original, data)?.node())
            }
            NodeData::PropertyAccessExpression(data)
                if self.property_access_targets_super(&data)? =>
            {
                Some(self.visit_captured_super_property(original, data)?.node())
            }
            NodeData::ElementAccessExpression(data)
                if self.element_access_targets_super(&data)? =>
            {
                Some(self.visit_captured_super_element(original, data)?.node())
            }
            NodeData::AwaitExpression(data) if self.in_non_top_level_context() => {
                Some(self.visit_await_expression(original, data)?)
            }
            NodeData::VariableStatement(data) if self.variable_statement_collides(&data)? => {
                self.visit_colliding_variable_statement(original, data)?
            }
            NodeData::ForStatement(data) if self.for_initializer_collides(data.initializer)? => {
                Some(self.visit_colliding_for_statement(original, data)?)
            }
            NodeData::ForInStatement(data)
                if self.for_initializer_collides(data.initializer)? =>
            {
                Some(self.visit_colliding_for_in_statement(original, data)?)
            }
            NodeData::ForOfStatement(data)
                if self.for_initializer_collides(data.initializer)? =>
            {
                Some(self.visit_colliding_for_of_statement(original, data)?)
            }
            NodeData::CatchClause(data) if self.catch_clause_shadows_parameter(&data)? => {
                Some(self.visit_shadowing_catch_clause(original, data)?)
            }
            NodeData::FunctionDeclaration(data) => {
                Some(self.with_non_top_level_context(|visitor| {
                    visitor.visit_function_declaration(original, data)
                })?)
            }
            NodeData::FunctionExpression(data) => {
                Some(self.with_non_top_level_context(|visitor| {
                    visitor.visit_function_expression(original, data)
                })?)
            }
            NodeData::ArrowFunction(data) => Some(self.with_non_top_level_context(|visitor| {
                visitor.visit_arrow_function(original, data)
            })?),
            NodeData::MethodDeclaration(data) => {
                Some(self.with_non_top_level_context(|visitor| {
                    visitor.visit_method_declaration(original, data)
                })?)
            }
            NodeData::GetAccessor(data) => Some(self.with_non_top_level_context(|visitor| {
                visitor.visit_get_accessor(original, data)
            })?),
            NodeData::SetAccessor(data) => Some(self.with_non_top_level_context(|visitor| {
                visitor.visit_set_accessor(original, data)
            })?),
            NodeData::Constructor(data) => {
                Some(self.with_non_top_level_context(|visitor| {
                    visitor.visit_constructor(original, data)
                })?)
            }
            NodeData::ClassDeclaration(data) => {
                Some(self.with_non_top_level_context(|visitor| {
                    visitor.visit_class_boundary(original, NodeData::ClassDeclaration(data))
                })?)
            }
            NodeData::ClassExpression(data) => {
                Some(self.with_non_top_level_context(|visitor| {
                    visitor.visit_class_boundary(original, NodeData::ClassExpression(data))
                })?)
            }
            NodeData::Token if record.kind == SyntaxKind::AsyncKeyword => None,
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn in_non_top_level_context(&self) -> bool {
        self.non_top_level_depth != 0
    }

    /// tsc's ES2017 transform lowers every AwaitExpression reached under its
    /// NonTopLevel context flag, including syntactically invalid awaits in a
    /// non-async function. Diagnostics remain checker-owned; emit must still
    /// follow the same recovery shape.
    fn with_non_top_level_context<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<T, TransformError> {
        self.non_top_level_depth = self
            .non_top_level_depth
            .checked_add(1)
            .expect("ES2017 non-top-level context depth overflowed");
        let result = operation(self);
        self.non_top_level_depth -= 1;
        result
    }

    fn super_capture_is_active(&self) -> bool {
        self.super_captures
            .last()
            .and_then(Option::as_ref)
            .is_some_and(|capture| capture.owns_access)
    }

    fn expression_is_super(&self, expression: Option<NodeId>) -> Result<bool, TransformError> {
        let Some(expression) = expression.map(|expression| self.node(expression)) else {
            return Ok(false);
        };
        Ok(self.context.arena().node(expression)?.kind == SyntaxKind::SuperKeyword)
    }

    fn property_access_targets_super(
        &self,
        data: &tsc_syntax::nodes::PropertyAccessExpressionData,
    ) -> Result<bool, TransformError> {
        Ok(self.super_capture_is_active() && self.expression_is_super(data.expression)?)
    }

    fn element_access_targets_super(
        &self,
        data: &tsc_syntax::nodes::ElementAccessExpressionData,
    ) -> Result<bool, TransformError> {
        Ok(self.super_capture_is_active() && self.expression_is_super(data.expression)?)
    }

    fn call_expression_targets_super(
        &self,
        data: &tsc_syntax::nodes::CallExpressionData,
    ) -> Result<bool, TransformError> {
        if !self.super_capture_is_active() {
            return Ok(false);
        }
        let Some(expression) = data.expression.map(|expression| self.node(expression)) else {
            return Ok(false);
        };
        match &self.context.arena().node(expression)?.data {
            NodeData::PropertyAccessExpression(property) => {
                self.property_access_targets_super(property)
            }
            NodeData::ElementAccessExpression(element) => {
                self.element_access_targets_super(element)
            }
            _ => Ok(false),
        }
    }

    fn node_is_direct_super_use(&self, node: TransformNode) -> Result<bool, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::PropertyAccessExpression(data) => self.property_access_targets_super(data),
            NodeData::ElementAccessExpression(data) => self.element_access_targets_super(data),
            NodeData::CallExpression(data) => self.call_expression_targets_super(data),
            _ => Ok(false),
        }
    }

    fn plan_async_super_capture(
        &mut self,
        function: TransformNode,
        body: Option<NodeId>,
    ) -> Result<AsyncSuperCapture, TransformError> {
        let resolver_node = self.resolver_node(function)?;
        let has_assignment = self.resolver.has_node_check_flag(
            resolver_node,
            NodeCheckFlags::METHOD_WITH_SUPER_PROPERTY_ASSIGNMENT_IN_ASYNC.bits() as u32,
        )?;
        let owns_access = has_assignment
            || self.resolver.has_node_check_flag(
                resolver_node,
                NodeCheckFlags::METHOD_WITH_SUPER_PROPERTY_ACCESS_IN_ASYNC.bits() as u32,
            )?;
        if !owns_access {
            return Ok(AsyncSuperCapture::default());
        }
        let mut properties = Vec::new();
        let mut has_element_access = false;
        if let Some(body) = body {
            self.collect_super_uses(
                self.node(body),
                true,
                &mut properties,
                &mut has_element_access,
            )?;
        }
        let binding = (!properties.is_empty())
            .then(|| self.allocate_preferred_binding("_super"))
            .transpose()?;
        let index_binding = has_element_access
            .then(|| self.allocate_preferred_binding("_superIndex"))
            .transpose()?;
        Ok(AsyncSuperCapture {
            binding,
            index_binding,
            properties,
            has_element_access,
            has_assignment,
            owns_access,
        })
    }

    fn collect_super_uses(
        &self,
        node: TransformNode,
        owns_access: bool,
        properties: &mut Vec<CapturedSuperProperty>,
        has_element_access: &mut bool,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(node)?.clone();
        let owns_access = if matches!(
            record.kind,
            SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::Constructor
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::GetAccessor
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::SetAccessor
        ) {
            false
        } else {
            owns_access
        };
        if owns_access {
            match &record.data {
                NodeData::PropertyAccessExpression(data)
                    if self.expression_is_super(data.expression)? =>
                {
                    let name = data.name.map(|name| self.node(name)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyAccessExpression,
                            field: "name",
                        },
                    )?;
                    let text = self.identifier_text(name)?.to_owned();
                    if !properties.iter().any(|property| property.text == text) {
                        properties.push(CapturedSuperProperty { text });
                    }
                    return Ok(());
                }
                NodeData::ElementAccessExpression(data)
                    if self.expression_is_super(data.expression)? =>
                {
                    *has_element_access = true;
                    if let Some(argument) = data.argument_expression {
                        self.collect_super_uses(
                            self.node(argument),
                            owns_access,
                            properties,
                            has_element_access,
                        )?;
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        if !owns_access {
            return Ok(());
        }
        let syntax = self.context.arena().source(self.source)?.syntax();
        let mut children = Vec::new();
        for_each_child(&syntax.arena, &record, |child| {
            children.push(child);
            false
        });
        for child in children {
            self.collect_super_uses(
                self.node(child),
                owns_access,
                properties,
                has_element_access,
            )?;
        }
        Ok(())
    }

    fn visit_captured_super_property(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::PropertyAccessExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let name =
            data.name
                .map(|name| self.node(name))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyAccessExpression,
                    field: "name",
                })?;
        let property = self.identifier_text(name)?.to_owned();
        let binding = self
            .super_captures
            .last()
            .and_then(Option::as_ref)
            .and_then(|capture| capture.binding.clone())
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SuperKeyword,
                field: "captured super binding",
            })?;
        let proxy = self.create_generated_identifier(&binding)?;
        let name = self.create_identifier(&property)?;
        let access = self.create_property_access(proxy, name)?;
        self.set_original_and_range(access, original)
    }

    fn visit_captured_super_element(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ElementAccessExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let argument = self.visit_required(
            data.argument_expression,
            SyntaxKind::ElementAccessExpression,
            "argument_expression",
        )?;
        let capture = self
            .super_captures
            .last()
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SuperKeyword,
                field: "captured super element",
            })?;
        let binding = capture
            .index_binding
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ElementAccessExpression,
                field: "captured super index binding",
            })?;
        let index = self.create_generated_identifier(&binding)?;
        let access = self.create_call(index, vec![argument])?;
        let access = if capture.has_assignment {
            let value = self.create_identifier("value")?;
            self.create_property_access(access, value)?
        } else {
            access
        };
        self.set_original_and_range(access, original)
    }

    fn visit_captured_super_call(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::CallExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let callee = data.expression.map(|callee| self.node(callee)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CallExpression,
                field: "expression",
            },
        )?;
        let access = match self.context.arena().node(callee)?.data.clone() {
            NodeData::PropertyAccessExpression(property) => {
                self.visit_captured_super_property(callee, property)?
            }
            NodeData::ElementAccessExpression(element) => {
                self.visit_captured_super_element(callee, element)?
            }
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::CallExpression,
                    field: "super property callee",
                });
            }
        };
        let call_name = self.create_identifier("call")?;
        let call = self.create_property_access(access, call_name)?;
        let this_arg = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        let mut arguments = vec![this_arg];
        for argument in self.array_nodes(data.arguments)? {
            arguments.push(self.visit_required(
                Some(argument.node()),
                SyntaxKind::CallExpression,
                "argument",
            )?);
        }
        let call = self.create_call(call, arguments)?;
        self.set_original_and_range(call, original)
    }

    fn visit_await_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::AwaitExpressionData,
    ) -> Result<NodeId, TransformError> {
        let expression =
            self.visit_required(data.expression, SyntaxKind::AwaitExpression, "expression")?;
        let flags = self.context.arena().propagate_child_flags(expression)?
            | TransformFlags::CONTAINS_YIELD;
        let yield_expression = self.context.factory()?.create_node(
            self.source,
            NodeData::YieldExpression(tsc_syntax::nodes::YieldExpressionData {
                asterisk_token: None,
                expression: Some(expression.node()),
            }),
            flags,
        )?;
        let yield_expression = self.set_original_and_range(yield_expression, original)?;
        let result = if self.yield_requires_parentheses(original)? {
            self.create_parenthesized(yield_expression)?
        } else {
            yield_expression
        };
        Ok(result.node())
    }

    fn yield_requires_parentheses(&self, original: TransformNode) -> Result<bool, TransformError> {
        let Some(parent) = self.context.arena().node(original)?.parent else {
            return Ok(false);
        };
        let parent = self.node(parent);
        Ok(match &self.context.arena().node(parent)?.data {
            NodeData::BinaryExpression(data) => {
                let operator = data
                    .operator_token
                    .map(|operator| self.context.arena().node(self.node(operator)))
                    .transpose()?
                    .map(|operator| operator.kind);
                let assignment = operator.is_some_and(|operator| {
                    operator.value() >= SyntaxKind::FirstAssignment.value()
                        && operator.value() <= SyntaxKind::LastAssignment.value()
                });
                !(assignment && data.right == Some(original.node()))
            }
            NodeData::PrefixUnaryExpression(_) | NodeData::PostfixUnaryExpression(_) => true,
            NodeData::PropertyAccessExpression(data) => data.expression == Some(original.node()),
            NodeData::ElementAccessExpression(data) => data.expression == Some(original.node()),
            NodeData::CallExpression(data) => data.expression == Some(original.node()),
            NodeData::NewExpression(data) => data.expression == Some(original.node()),
            NodeData::TaggedTemplateExpression(data) => data.tag == Some(original.node()),
            NodeData::ConditionalExpression(data) => data.condition == Some(original.node()),
            _ => false,
        })
    }

    fn current_collision_names(&self) -> Option<&BTreeSet<String>> {
        self.frames
            .last()
            .and_then(|frame| frame.colliding_parameter_names.as_ref())
    }

    fn variable_statement_collides(
        &self,
        data: &tsc_syntax::nodes::VariableStatementData,
    ) -> Result<bool, TransformError> {
        self.variable_list_collides(data.declaration_list)
    }

    fn for_initializer_collides(
        &self,
        initializer: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        self.variable_list_collides(initializer)
    }

    fn variable_list_collides(&self, list: Option<NodeId>) -> Result<bool, TransformError> {
        let Some(names) = self.current_collision_names() else {
            return Ok(false);
        };
        let Some(list) = list.map(|list| self.node(list)) else {
            return Ok(false);
        };
        let record = self.context.arena().node(list)?;
        let NodeData::VariableDeclarationList(data) = &record.data else {
            return Ok(false);
        };
        if NodeFlags::from_bits(record.flags).intersects(NodeFlags::BLOCK_SCOPED) {
            return Ok(false);
        }
        for declaration in self.array_nodes(data.declarations)? {
            let NodeData::VariableDeclaration(declaration) =
                &self.context.arena().node(declaration)?.data
            else {
                continue;
            };
            if declaration
                .name
                .map(|name| self.binding_name_intersects(self.node(name), names))
                .transpose()?
                .unwrap_or(false)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A synthetic variable declaration produced by an earlier target pass
    /// can already carry a generated-binding identity. ES2017 must preserve
    /// that identity when it hoists or rewrites a colliding `var`; recreating
    /// a plain identifier loses the shared name-generator scope used by the
    /// nested __awaiter generator.
    fn generated_binding_for_identifier(&self, name: TransformNode) -> Option<TargetBinding> {
        let metadata = self.context.arena().metadata(name)?;
        let id = metadata.generated_binding_id()?;
        let NodeData::Identifier(identifier) = &self.context.arena().node(name).ok()?.data else {
            return None;
        };
        Some(TargetBinding::from_existing(
            id,
            identifier.text.clone(),
            metadata.generated_binding_base().map(str::to_owned),
            metadata
                .generated_binding_preferred_base()
                .map(str::to_owned),
            metadata.generated_binding_role_suffix().map(str::to_owned),
            metadata.generated_binding_reserved_in_nested_scopes(),
        ))
    }

    fn binding_name_intersects(
        &self,
        name: TransformNode,
        names: &BTreeSet<String>,
    ) -> Result<bool, TransformError> {
        match &self.context.arena().node(name)?.data {
            NodeData::Identifier(identifier) => {
                // getGeneratedNameForNode retains the source identifier's
                // escapedText for collision checks even though the printer
                // renders a numbered spelling. TargetBinding records that
                // source identity explicitly as numbered_base.
                let collision_name = self
                    .context
                    .arena()
                    .metadata(name)
                    .and_then(|metadata| metadata.generated_binding_base())
                    .unwrap_or(&identifier.text);
                Ok(names.contains(collision_name))
            }
            NodeData::ObjectBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if let NodeData::BindingElement(element) =
                        &self.context.arena().node(element)?.data
                    {
                        if let Some(name) = element.name {
                            if self.binding_name_intersects(self.node(name), names)? {
                                return Ok(true);
                            }
                        }
                    }
                }
                Ok(false)
            }
            NodeData::ArrayBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if let NodeData::BindingElement(element) =
                        &self.context.arena().node(element)?.data
                    {
                        if let Some(name) = element.name {
                            if self.binding_name_intersects(self.node(name), names)? {
                                return Ok(true);
                            }
                        }
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn visit_colliding_variable_statement(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::VariableStatementData,
    ) -> Result<Option<NodeId>, TransformError> {
        let expression = self.lower_colliding_variable_list(
            data.declaration_list,
            false,
            SyntaxKind::VariableStatement,
        )?;
        let Some(expression) = expression else {
            return Ok(None);
        };
        let statement = self.create_expression_statement(expression)?;
        Ok(Some(
            self.set_original_and_range(statement, original)?.node(),
        ))
    }

    fn visit_colliding_for_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForStatementData,
    ) -> Result<NodeId, TransformError> {
        data.initializer = self
            .lower_colliding_variable_list(data.initializer, false, SyntaxKind::ForStatement)?
            .map(TransformNode::node);
        data.condition = self.visit_optional_node(data.condition)?;
        data.incrementor = self.visit_optional_node(data.incrementor)?;
        data.statement = self.visit_optional_node(data.statement)?;
        self.update_without_visit(original, NodeData::ForStatement(data))
    }

    fn visit_colliding_for_in_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForInStatementData,
    ) -> Result<NodeId, TransformError> {
        data.initializer = self
            .lower_colliding_variable_list(data.initializer, true, SyntaxKind::ForInStatement)?
            .map(TransformNode::node);
        data.expression = self.visit_optional_node(data.expression)?;
        data.statement = self.visit_optional_node(data.statement)?;
        self.update_without_visit(original, NodeData::ForInStatement(data))
    }

    fn visit_colliding_for_of_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForOfStatementData,
    ) -> Result<NodeId, TransformError> {
        data.await_modifier = self.visit_optional_node(data.await_modifier)?;
        data.initializer = self
            .lower_colliding_variable_list(data.initializer, true, SyntaxKind::ForOfStatement)?
            .map(TransformNode::node);
        data.expression = self.visit_optional_node(data.expression)?;
        data.statement = self.visit_optional_node(data.statement)?;
        self.update_without_visit(original, NodeData::ForOfStatement(data))
    }

    fn lower_colliding_variable_list(
        &mut self,
        list: Option<NodeId>,
        has_receiver: bool,
        parent: SyntaxKind,
    ) -> Result<Option<TransformNode>, TransformError> {
        let list =
            list.map(|list| self.node(list))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent,
                    field: "variable declaration list",
                })?;
        let NodeData::VariableDeclarationList(data) = self.context.arena().node(list)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent,
                field: "variable declaration list",
            });
        };
        let declarations = self.array_nodes(data.declarations)?;
        for declaration in &declarations {
            let NodeData::VariableDeclaration(data) =
                self.context.arena().node(*declaration)?.data.clone()
            else {
                continue;
            };
            if let Some(name) = data.name {
                self.hoist_binding_name(self.node(name))?;
            }
        }
        let mut assignments = Vec::new();
        for declaration in &declarations {
            let NodeData::VariableDeclaration(data) =
                self.context.arena().node(*declaration)?.data.clone()
            else {
                continue;
            };
            let Some(initializer) = data.initializer else {
                continue;
            };
            let name = data.name.map(|name| self.node(name)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclaration,
                    field: "name",
                },
            )?;
            let target = self.binding_name_to_assignment_target(name)?;
            let target = self.visit_required(
                Some(target.node()),
                SyntaxKind::BinaryExpression,
                "assignment target",
            )?;
            let initializer = self.visit_required(
                Some(initializer),
                SyntaxKind::VariableDeclaration,
                "initializer",
            )?;
            let assignment = self.create_binary(target, SyntaxKind::EqualsToken, initializer)?;
            self.context
                .factory()?
                .set_text_range(assignment, *declaration)?;
            assignments.push(assignment);
        }
        if assignments.is_empty() {
            if !has_receiver {
                return Ok(None);
            }
            let first =
                declarations
                    .first()
                    .copied()
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent,
                        field: "receiver declaration",
                    })?;
            let NodeData::VariableDeclaration(data) =
                self.context.arena().node(first)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent,
                    field: "receiver declaration",
                });
            };
            let name = data.name.map(|name| self.node(name)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent,
                    field: "receiver name",
                },
            )?;
            return self.binding_name_to_assignment_target(name).map(Some);
        }
        self.inline_expressions(assignments).map(Some)
    }

    fn hoist_binding_name(&mut self, name: TransformNode) -> Result<(), TransformError> {
        if let Some(binding) = self.generated_binding_for_identifier(name) {
            let name = self.create_generated_identifier(&binding)?;
            self.context.hoist_variable_declaration(name)?;
            return Ok(());
        }
        let mut names = BTreeSet::new();
        self.collect_binding_names(name, &mut names)?;
        for name in names {
            let name = self.create_identifier(&name)?;
            self.context.hoist_variable_declaration(name)?;
        }
        Ok(())
    }

    fn binding_name_to_assignment_target(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if let Some(binding) = self.generated_binding_for_identifier(name) {
            return self.create_generated_identifier(&binding);
        }
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(identifier) => self.create_identifier(&identifier.text),
            // Binding-pattern conversion is structural. Keeping it here makes
            // the collision plan explicit and leaves object-rest flattening to
            // the already-closed ES2018 pass.
            NodeData::ObjectBindingPattern(data) => {
                let mut properties = Vec::new();
                for element in self.array_nodes(data.elements)? {
                    let NodeData::BindingElement(element_data) =
                        self.context.arena().node(element)?.data.clone()
                    else {
                        continue;
                    };
                    let target = element_data.name.map(|name| self.node(name)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BindingElement,
                            field: "name",
                        },
                    )?;
                    let target = self.binding_name_to_assignment_target(target)?;
                    let property_name = element_data
                        .property_name
                        .map(|name| self.node(name))
                        .unwrap_or(target);
                    let initializer = if let Some(initializer) = element_data.initializer {
                        let initializer = self.visit_required(
                            Some(initializer),
                            SyntaxKind::BindingElement,
                            "initializer",
                        )?;
                        self.create_binary(target, SyntaxKind::EqualsToken, initializer)?
                    } else {
                        target
                    };
                    properties.push(self.create_property_assignment(property_name, initializer)?);
                }
                self.create_object_literal(properties)
            }
            NodeData::ArrayBindingPattern(data) => {
                let mut elements = Vec::new();
                for element in self.array_nodes(data.elements)? {
                    if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                        elements.push(element);
                        continue;
                    }
                    let NodeData::BindingElement(element_data) =
                        self.context.arena().node(element)?.data.clone()
                    else {
                        continue;
                    };
                    let target = element_data.name.map(|name| self.node(name)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BindingElement,
                            field: "name",
                        },
                    )?;
                    let mut target = self.binding_name_to_assignment_target(target)?;
                    if let Some(initializer) = element_data.initializer {
                        let initializer = self.visit_required(
                            Some(initializer),
                            SyntaxKind::BindingElement,
                            "initializer",
                        )?;
                        target =
                            self.create_binary(target, SyntaxKind::EqualsToken, initializer)?;
                    }
                    if element_data.dot_dot_dot_token.is_some() {
                        let flags = self.context.arena().propagate_child_flags(target)?
                            | TransformFlags::CONTAINS_REST_OR_SPREAD;
                        target = self.context.factory()?.create_node(
                            self.source,
                            NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                                expression: Some(target.node()),
                            }),
                            flags,
                        )?;
                    }
                    elements.push(target);
                }
                self.create_array_literal(elements)
            }
            _ => Ok(name),
        }
    }

    fn catch_clause_shadows_parameter(
        &self,
        data: &tsc_syntax::nodes::CatchClauseData,
    ) -> Result<bool, TransformError> {
        let Some(names) = self.current_collision_names() else {
            return Ok(false);
        };
        let Some(variable) = data
            .variable_declaration
            .map(|variable| self.node(variable))
        else {
            return Ok(false);
        };
        let NodeData::VariableDeclaration(variable) = &self.context.arena().node(variable)?.data
        else {
            return Ok(false);
        };
        variable
            .name
            .map(|name| self.binding_name_intersects(self.node(name), names))
            .transpose()
            .map(|value| value.unwrap_or(false))
    }

    fn visit_shadowing_catch_clause(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::CatchClauseData,
    ) -> Result<NodeId, TransformError> {
        let mut shadowed = BTreeSet::new();
        if let Some(variable) = data
            .variable_declaration
            .map(|variable| self.node(variable))
        {
            if let NodeData::VariableDeclaration(variable) =
                &self.context.arena().node(variable)?.data
            {
                if let Some(name) = variable.name {
                    self.collect_binding_names(self.node(name), &mut shadowed)?;
                }
            }
        }
        let previous = self
            .frames
            .last_mut()
            .and_then(|frame| frame.colliding_parameter_names.take());
        if let Some(mut unshadowed) = previous.clone() {
            for name in shadowed {
                unshadowed.remove(&name);
            }
            self.frames
                .last_mut()
                .expect("catch clause is inside an async frame")
                .colliding_parameter_names = Some(unshadowed);
        }
        let result = self.update_generic(original, NodeData::CatchClause(data));
        self.frames
            .last_mut()
            .expect("catch clause is inside an async frame")
            .colliding_parameter_names = previous;
        result
    }

    fn visit_class_boundary(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        let saved_lexical_this = self.has_lexical_this;
        let saved_arguments = self.lexical_arguments_binding.take();
        self.has_lexical_this = true;
        self.super_captures.push(None);
        let result = self.update_generic(original, data);
        let capture = self.super_captures.pop();
        debug_assert!(matches!(capture, Some(None)));
        self.has_lexical_this = saved_lexical_this;
        self.lexical_arguments_binding = saved_arguments;
        result
    }

    fn visit_function_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::FunctionDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::FunctionDeclaration,
            FunctionShape::Ordinary,
            data.modifiers,
            data.asterisk_token,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.asterisk_token = transformed.asterisk_token;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::FunctionDeclaration(data))
    }

    fn visit_function_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::FunctionExpressionData,
    ) -> Result<NodeId, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::FunctionExpression,
            FunctionShape::Ordinary,
            data.modifiers,
            data.asterisk_token,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.asterisk_token = transformed.asterisk_token;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::FunctionExpression(data))
    }

    fn visit_arrow_function(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ArrowFunctionData,
    ) -> Result<NodeId, TransformError> {
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        data.equals_greater_than_token =
            self.visit_optional_node(data.equals_greater_than_token)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::ArrowFunction,
            FunctionShape::Arrow,
            data.modifiers,
            None,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::ArrowFunction(data))
    }

    fn visit_method_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::MethodDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.question_token = self.visit_optional_node(data.question_token)?;
        data.exclamation_token = self.visit_optional_node(data.exclamation_token)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::MethodDeclaration,
            FunctionShape::Ordinary,
            data.modifiers,
            data.asterisk_token,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.asterisk_token = transformed.asterisk_token;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::MethodDeclaration(data))
    }

    fn visit_get_accessor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::GetAccessorData,
    ) -> Result<NodeId, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::GetAccessor,
            FunctionShape::Ordinary,
            data.modifiers,
            None,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::GetAccessor(data))
    }

    fn visit_set_accessor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SetAccessorData,
    ) -> Result<NodeId, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::SetAccessor,
            FunctionShape::Ordinary,
            data.modifiers,
            None,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::SetAccessor(data))
    }

    fn visit_constructor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ConstructorData,
    ) -> Result<NodeId, TransformError> {
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let transformed = self.transform_function(
            original,
            SyntaxKind::Constructor,
            FunctionShape::Ordinary,
            data.modifiers,
            None,
            data.parameters,
            data.body,
        )?;
        data.modifiers = transformed.modifiers;
        data.parameters = transformed.parameters;
        data.body = transformed.body;
        self.update_without_visit(original, NodeData::Constructor(data))
    }

    #[allow(clippy::too_many_arguments)]
    fn transform_function(
        &mut self,
        original: TransformNode,
        kind: SyntaxKind,
        shape: FunctionShape,
        modifiers: Option<NodeArrayId>,
        asterisk_token: Option<NodeId>,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
    ) -> Result<TransformedFunction, TransformError> {
        let is_async = self.modifiers_contain_async(modifiers)?;
        let (previous_scope, scope) = self
            .generated_bindings
            .enter(GeneratedBindingOwner::FunctionBody);
        let saved_lexical_this = self.has_lexical_this;
        let saved_arguments = (shape == FunctionShape::Ordinary)
            .then(|| self.lexical_arguments_binding.take())
            .flatten();
        if shape == FunctionShape::Ordinary {
            self.has_lexical_this = true;
            let capture = if is_async {
                Some(self.plan_async_super_capture(original, body)?)
            } else {
                None
            };
            self.super_captures.push(capture);
        }

        let result = if is_async {
            self.transform_async_function(original, kind, shape, modifiers, parameters, body)
        } else {
            self.frames.push(FunctionFrame {
                colliding_parameter_names: None,
            });
            let modifiers = self.visit_optional_nodes(modifiers);
            let asterisk_token = self.visit_optional_node(asterisk_token);
            let parameters = self.visit_optional_nodes(parameters);
            let body = self.visit_optional_node(body);
            let frame = self.frames.pop();
            debug_assert!(frame.is_some());
            Ok(TransformedFunction {
                modifiers: modifiers?,
                asterisk_token: asterisk_token?,
                parameters: parameters?,
                body: body?,
            })
        };

        if shape == FunctionShape::Ordinary {
            let capture = self.super_captures.pop();
            debug_assert!(capture.is_some());
            self.lexical_arguments_binding = saved_arguments;
        }
        self.has_lexical_this = saved_lexical_this;
        let _ = self.generated_bindings.exit(previous_scope, scope);
        result
    }

    fn transform_async_function(
        &mut self,
        original: TransformNode,
        kind: SyntaxKind,
        shape: FunctionShape,
        modifiers: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
    ) -> Result<TransformedFunction, TransformError> {
        let parameter_plan = self.plan_async_parameters(shape, parameters)?;
        // transformAsyncFunctionBody always installs the complete original
        // parameter-name set before walking the body (101398-101403). This is
        // needed even for a simple parameter list: a preceding target pass
        // can synthesize a `var` whose generated-name source collides with a
        // parameter (for-await's `c_1` derived from parameter `c`).
        let colliding_parameter_names = Some(self.collect_parameter_names(parameters)?);

        let lexical_arguments = self.plan_async_lexical_arguments(original)?;

        self.context.start_lexical_environment()?;
        self.frames.push(FunctionFrame {
            colliding_parameter_names,
        });
        let inner_body = self.visit_async_body(body, kind);
        let frame = self.frames.pop();
        debug_assert!(frame.is_some());
        let lexical_environment = self.context.end_lexical_environment();
        let inner_body = inner_body?;
        let inner_body =
            self.merge_lexical_environment_into_block(inner_body, lexical_environment?)?;

        let arguments_expression =
            if shape == FunctionShape::Ordinary && parameter_plan.inner.is_some() {
                Some(self.create_identifier("arguments")?)
            } else {
                self.create_forwarded_arguments(shape, &parameter_plan.forwarded_arguments)?
            };
        let awaiter = self.create_awaiter_call(
            self.has_lexical_this,
            arguments_expression,
            parameter_plan.inner,
            inner_body,
        )?;
        let outer_body =
            if shape == FunctionShape::Arrow && lexical_arguments.capture_binding().is_none() {
                awaiter
            } else {
                let mut statements = if shape == FunctionShape::Ordinary {
                    self.super_captures
                        .last()
                        .and_then(Option::as_ref)
                        .cloned()
                        .map(|capture| self.create_async_super_statements(capture))
                        .transpose()?
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                if let Some(binding) = lexical_arguments.capture_binding() {
                    statements.push(self.create_capture_arguments_statement(binding)?);
                }
                statements.push(self.create_return_statement(Some(awaiter))?);
                self.create_block(statements, true)?
            };
        if let Some(original_body) = body.map(|body| self.node(body)) {
            self.context
                .factory()?
                .set_text_range(outer_body, original_body)?;
        }
        Ok(TransformedFunction {
            modifiers: self.visit_modifier_array_without_async(modifiers)?,
            asterisk_token: None,
            parameters: parameter_plan.outer,
            body: Some(outer_body.node()),
        })
    }

    /// tsc-port: transformAsyncFunctionParameterList/transformAsyncFunctionBody @6.0.3
    /// tsc-hash: d30d6713a38b765219ff9c4248c5c8c749bf478885903c2ea517186fdd701ec7
    /// tsc-span: _tsc.js:101284-101347
    fn plan_async_parameters(
        &mut self,
        shape: FunctionShape,
        parameters: Option<NodeArrayId>,
    ) -> Result<AsyncParameterPlan, TransformError> {
        if self.parameter_list_is_simple(parameters)? {
            return Ok(AsyncParameterPlan {
                outer: self.visit_optional_nodes(parameters)?,
                inner: None,
                forwarded_arguments: Vec::new(),
            });
        }
        let Some(parameters) = parameters else {
            unreachable!("an absent parameter list is simple")
        };
        let original = self.array(parameters);
        let mut outer = Vec::new();
        let mut forwarded_arguments = Vec::new();
        for parameter in self.array_nodes(Some(parameters))? {
            let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "parameter",
                });
            };
            if data.initializer.is_some() || data.dot_dot_dot_token.is_some() {
                if shape == FunctionShape::Arrow {
                    let binding = self.allocate_numbered_binding("args")?;
                    outer.push(self.create_forwarding_parameter(&binding, true)?);
                    forwarded_arguments.push(ForwardedArgument::Spread(binding));
                }
                break;
            }
            let name = data.name.map(|name| self.node(name)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "name",
                },
            )?;
            let identifier_text = match &self.context.arena().node(name)?.data {
                NodeData::Identifier(identifier) => Some(identifier.text.clone()),
                _ => None,
            };
            let binding = if let Some(identifier_text) = identifier_text {
                self.allocate_numbered_binding(&identifier_text)?
            } else {
                self.allocate_temp_binding()?
            };
            outer.push(self.create_forwarding_parameter(&binding, false)?);
            forwarded_arguments.push(ForwardedArgument::Direct(binding));
        }
        let outer = self
            .context
            .factory()?
            .update_node_array(original, outer)?
            .array();
        Ok(AsyncParameterPlan {
            outer: Some(outer),
            inner: self.visit_optional_nodes(Some(parameters))?,
            forwarded_arguments,
        })
    }

    fn parameter_list_is_simple(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for parameter in self.array_nodes(parameters)? {
            if !self.async_parameter_fact(parameter)?.is_simple() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: isSimpleParameter/isSimpleParameterList @6.0.3
    /// tsc-hash: 0774a70cea3e7f3f20e799ad82504bce1fbaf781ca4089715c44187bdfb56e54
    /// tsc-span: _tsc.js:93236-93242
    ///
    /// tsc deliberately ignores the rest token. At an
    /// ES2015 target, `...args` can stay on the async wrapper and be closed
    /// over by the generator; only an initializer or binding pattern needs a
    /// separate inner parameter scope.
    fn async_parameter_fact(
        &self,
        parameter: TransformNode,
    ) -> Result<AsyncParameterFact, TransformError> {
        let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "parameter",
            });
        };
        let identifier = data.name.map(|name| self.node(name)).is_some_and(|name| {
            self.context
                .arena()
                .node(name)
                .is_ok_and(|name| name.kind == SyntaxKind::Identifier)
        });
        Ok(if data.initializer.is_some() || !identifier {
            AsyncParameterFact::RequiresInnerScope
        } else if data.dot_dot_dot_token.is_some() {
            AsyncParameterFact::RestIdentifier
        } else {
            AsyncParameterFact::PlainIdentifier
        })
    }

    /// tsc-port: transformAsyncFunctionBody @6.0.3 (CaptureArguments planning)
    /// tsc-hash: b28cef3793943b5703d941ce3a52182b72134cf3fa0e1bf8c577db0de7882489
    /// tsc-span: _tsc.js:101315-101330
    fn plan_async_lexical_arguments(
        &mut self,
        original: TransformNode,
    ) -> Result<AsyncLexicalArgumentsPlan, TransformError> {
        let uses_lexical_arguments = self.resolver.has_node_check_flag(
            self.resolver_node(original)?,
            NodeCheckFlags::CAPTURE_ARGUMENTS.bits() as u32,
        )?;
        if !uses_lexical_arguments {
            return Ok(AsyncLexicalArgumentsPlan::Unused);
        }
        if self.lexical_arguments_binding.is_some() {
            return Ok(AsyncLexicalArgumentsPlan::Inherited);
        }
        let binding = self.allocate_numbered_binding("arguments")?;
        self.lexical_arguments_binding = Some(binding.clone());
        Ok(AsyncLexicalArgumentsPlan::Capture(binding))
    }

    fn collect_parameter_names(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<BTreeSet<String>, TransformError> {
        let mut names = BTreeSet::new();
        for parameter in self.array_nodes(parameters)? {
            if let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data {
                if let Some(name) = data.name {
                    self.collect_binding_names(self.node(name), &mut names)?;
                }
            }
        }
        Ok(names)
    }

    fn collect_binding_names(
        &self,
        name: TransformNode,
        names: &mut BTreeSet<String>,
    ) -> Result<(), TransformError> {
        match &self.context.arena().node(name)?.data {
            NodeData::Identifier(identifier) => {
                names.insert(identifier.text.clone());
            }
            NodeData::ObjectBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if let NodeData::BindingElement(element) =
                        &self.context.arena().node(element)?.data
                    {
                        if let Some(name) = element.name {
                            self.collect_binding_names(self.node(name), names)?;
                        }
                    }
                }
            }
            NodeData::ArrayBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if let NodeData::BindingElement(element) =
                        &self.context.arena().node(element)?.data
                    {
                        if let Some(name) = element.name {
                            self.collect_binding_names(self.node(name), names)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn visit_async_body(
        &mut self,
        body: Option<NodeId>,
        kind: SyntaxKind,
    ) -> Result<TransformNode, TransformError> {
        let body = body.ok_or(TransformError::RequiredChildRemoved {
            parent: kind,
            field: "async function body",
        })?;
        let body = self.node(body);
        if self.context.arena().node(body)?.kind == SyntaxKind::Block {
            return self.visit(body.node())?.map(|body| self.node(body)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: kind,
                    field: "async function block",
                },
            );
        }
        let expression = self
            .visit(body.node())?
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: kind,
                field: "async arrow expression",
            })?;
        let statement = self.create_return_statement(Some(expression))?;
        self.context.factory()?.set_text_range(statement, body)?;
        let block = self.create_block(vec![statement], false)?;
        self.context.factory()?.set_text_range(block, body)
    }

    fn create_awaiter_call(
        &mut self,
        has_lexical_this: bool,
        arguments_expression: Option<TransformNode>,
        inner_parameters: Option<NodeArrayId>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .request_emit_helper(super::helpers::awaiter())?;
        let parameters = match inner_parameters {
            Some(parameters) => self.array(parameters),
            None => self
                .context
                .factory()?
                .create_node_array(self.source, Vec::new())?,
        };
        let asterisk = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::AsteriskToken,
            TransformFlags::NONE,
        )?;
        let flags = self.context.arena().array_transform_flags(parameters)
            | self.context.arena().propagate_child_flags(body)?;
        let generator = self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: Some(asterisk.node()),
                body: Some(body.node()),
                modifiers: None,
            }),
            flags,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(generator)
            .add_flags(EmitFlags::ASYNC_FUNCTION_BODY | EmitFlags::REUSE_TEMP_VARIABLE_SCOPE);
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Awaiter)?;
        let this_arg = if has_lexical_this {
            self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::CONTAINS_LEXICAL_THIS,
            )?
        } else {
            self.create_void_zero()?
        };
        let arguments = arguments_expression.unwrap_or(self.create_void_zero()?);
        let promise = self.create_void_zero()?;
        self.create_call(helper, vec![this_arg, arguments, promise, generator])
    }

    fn create_forwarded_arguments(
        &mut self,
        shape: FunctionShape,
        forwarded: &[ForwardedArgument],
    ) -> Result<Option<TransformNode>, TransformError> {
        if forwarded.is_empty() {
            return Ok(None);
        }
        if shape == FunctionShape::Ordinary {
            return self.create_identifier("arguments").map(Some);
        }
        let mut elements = Vec::with_capacity(forwarded.len());
        for argument in forwarded {
            match argument {
                ForwardedArgument::Direct(binding) => {
                    elements.push(self.create_generated_identifier(binding)?);
                }
                ForwardedArgument::Spread(binding) => {
                    let expression = self.create_generated_identifier(binding)?;
                    let flags = self.context.arena().propagate_child_flags(expression)?
                        | TransformFlags::CONTAINS_REST_OR_SPREAD;
                    elements.push(self.context.factory()?.create_node(
                        self.source,
                        NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                            expression: Some(expression.node()),
                        }),
                        flags,
                    )?);
                }
            }
        }
        self.create_array_literal(elements).map(Some)
    }

    fn create_forwarding_parameter(
        &mut self,
        binding: &TargetBinding,
        rest: bool,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_generated_identifier(binding)?;
        let dot_dot_dot_token = rest
            .then(|| {
                self.context.factory()?.create_token(
                    self.source,
                    SyntaxKind::DotDotDotToken,
                    TransformFlags::CONTAINS_REST_OR_SPREAD,
                )
            })
            .transpose()?;
        let mut children = vec![name];
        children.extend(dot_dot_dot_token);
        let flags = self.child_flags(&children)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(name.node()),
                modifiers: None,
                dot_dot_dot_token: dot_dot_dot_token.map(TransformNode::node),
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            flags,
        )
    }

    fn create_capture_arguments_statement(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_generated_identifier(binding)?;
        let arguments = self.create_identifier("arguments")?;
        let declaration = self.create_variable_declaration(name, Some(arguments))?;
        let list = self.create_variable_declaration_list(vec![declaration], NodeFlags::NONE)?;
        let statement = self.create_variable_statement_from_list(list)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::CUSTOM_PROLOGUE);
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_starts_on_new_line(true);
        Ok(statement)
    }

    fn modifiers_contain_async(
        &self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        Ok(self.array_nodes(modifiers)?.iter().any(|modifier| {
            self.context
                .arena()
                .node(*modifier)
                .is_ok_and(|modifier| modifier.kind == SyntaxKind::AsyncKeyword)
        }))
    }

    fn visit_modifier_array_without_async(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) = modifiers else {
            return Ok(None);
        };
        let original = self.array(modifiers);
        let mut retained = Vec::new();
        for modifier in self.array_nodes(Some(modifiers))? {
            if self.context.arena().node(modifier)?.kind == SyntaxKind::AsyncKeyword {
                continue;
            }
            if let Some(modifier) = self.visit(modifier.node())? {
                retained.push(self.node(modifier));
            }
        }
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .update_node_array(original, retained)?
                    .array(),
            ))
        }
    }

    fn allocate_numbered_binding(
        &mut self,
        source_name: &str,
    ) -> Result<TargetBinding, TransformError> {
        TargetBinding::allocate_numbered_reserved_in_nested_scopes(
            self.context,
            source_name.to_owned(),
            self.generated_bindings.allocate_local_numbered(source_name),
        )
    }

    fn allocate_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
        TargetBinding::allocate_reserved_in_nested_scopes(
            self.context,
            self.generated_bindings.allocate_local_temp(),
        )
    }

    fn allocate_preferred_binding(
        &mut self,
        preferred: &str,
    ) -> Result<TargetBinding, TransformError> {
        let preferred = preferred.to_owned();
        let provisional = self
            .generated_bindings
            .allocate_local_preferred_with_policy(preferred.clone(), true);
        TargetBinding::allocate_preferred_reserved_in_nested_scopes(
            self.context,
            preferred,
            provisional,
        )
    }

    fn create_async_super_statements(
        &mut self,
        capture: AsyncSuperCapture,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if !capture.owns_access {
            return Ok(Vec::new());
        }
        let mut statements = Vec::with_capacity(2);
        if capture.has_element_access {
            let index_binding =
                capture
                    .index_binding
                    .as_ref()
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ElementAccessExpression,
                        field: "captured super index binding",
                    })?;
            statements
                .push(self.create_super_index_statement(index_binding, capture.has_assignment)?);
        }
        if let Some(binding) = capture.binding {
            let mut accessors = Vec::with_capacity(capture.properties.len());
            for property in capture.properties {
                let super_token = self.context.factory()?.create_token(
                    self.source,
                    SyntaxKind::SuperKeyword,
                    TransformFlags::NONE,
                )?;
                let super_name = self.create_identifier(&property.text)?;
                let super_access = self.create_property_access(super_token, super_name)?;
                let getter = self.create_arrow(Vec::new(), super_access)?;
                let get_name = self.create_identifier("get")?;
                let get = self.create_property_assignment(get_name, getter)?;
                let mut getter_and_setter = vec![get];
                if capture.has_assignment {
                    let parameter = self.create_parameter("v")?;
                    let super_token = self.context.factory()?.create_token(
                        self.source,
                        SyntaxKind::SuperKeyword,
                        TransformFlags::NONE,
                    )?;
                    let super_name = self.create_identifier(&property.text)?;
                    let super_access = self.create_property_access(super_token, super_name)?;
                    let value = self.create_identifier("v")?;
                    let assignment =
                        self.create_binary(super_access, SyntaxKind::EqualsToken, value)?;
                    let setter = self.create_arrow(vec![parameter], assignment)?;
                    let set_name = self.create_identifier("set")?;
                    getter_and_setter.push(self.create_property_assignment(set_name, setter)?);
                }
                let descriptor = self.create_object_literal(getter_and_setter)?;
                let property_name = self.create_identifier(&property.text)?;
                accessors.push(self.create_property_assignment(property_name, descriptor)?);
            }
            let descriptors = self.create_object_literal(accessors)?;
            let descriptors = self.context.factory()?.set_multi_line(descriptors, true)?;
            let object = self.create_identifier("Object")?;
            let create_name = self.create_identifier("create")?;
            let create = self.create_property_access(object, create_name)?;
            let null = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::NullKeyword,
                TransformFlags::NONE,
            )?;
            let initializer = self.create_call(create, vec![null, descriptors])?;
            let name = self.create_generated_identifier(&binding)?;
            let declaration = self.create_variable_declaration(name, Some(initializer))?;
            let list =
                self.create_variable_declaration_list(vec![declaration], NodeFlags::CONST)?;
            statements.push(self.create_variable_statement_from_list(list)?);
        }
        Ok(statements)
    }

    fn create_super_index_statement(
        &mut self,
        binding: &TargetBinding,
        has_assignment: bool,
    ) -> Result<TransformNode, TransformError> {
        let initializer = if has_assignment {
            self.create_advanced_super_index_helper()?
        } else {
            let parameter = self.create_parameter("name")?;
            let super_token = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::SuperKeyword,
                TransformFlags::NONE,
            )?;
            let name = self.create_identifier("name")?;
            let access = self.create_element_access(super_token, name)?;
            self.create_arrow(vec![parameter], access)?
        };
        let name = self.create_generated_identifier(binding)?;
        let declaration = self.create_variable_declaration(name, Some(initializer))?;
        let list = self.create_variable_declaration_list(vec![declaration], NodeFlags::CONST)?;
        self.create_variable_statement_from_list(list)
    }

    fn create_advanced_super_index_helper(&mut self) -> Result<TransformNode, TransformError> {
        let object = self.create_identifier("Object")?;
        let create_name = self.create_identifier("create")?;
        let create = self.create_property_access(object, create_name)?;
        let null = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::NullKeyword,
            TransformFlags::NONE,
        )?;
        let cache_initializer = self.create_call(create, vec![null])?;
        let cache_name = self.create_identifier("cache")?;
        let cache_declaration =
            self.create_variable_declaration(cache_name, Some(cache_initializer))?;
        let cache_list =
            self.create_variable_declaration_list(vec![cache_declaration], NodeFlags::CONST)?;
        let cache_statement = self.create_variable_statement_from_list(cache_list)?;

        let cache = self.create_identifier("cache")?;
        let name = self.create_identifier("name")?;
        let cached = self.create_element_access(cache, name)?;
        let cache = self.create_identifier("cache")?;
        let name = self.create_identifier("name")?;
        let cache_target = self.create_element_access(cache, name)?;

        let geti = self.create_identifier("geti")?;
        let name = self.create_identifier("name")?;
        let get_call = self.create_call(geti, vec![name])?;
        let get_return = self.create_return_statement(Some(get_call))?;
        let get_body = self.create_block(vec![get_return], false)?;
        let getter = self.create_get_accessor("value", get_body)?;

        let seti = self.create_identifier("seti")?;
        let name = self.create_identifier("name")?;
        let value = self.create_identifier("v")?;
        let set_call = self.create_call(seti, vec![name, value])?;
        let set_statement = self.create_expression_statement(set_call)?;
        let set_body = self.create_block(vec![set_statement], false)?;
        let setter = self.create_set_accessor("value", "v", set_body)?;

        let entry = self.create_object_literal(vec![getter, setter])?;
        let assignment = self.create_binary(cache_target, SyntaxKind::EqualsToken, entry)?;
        let assignment = self.create_parenthesized(assignment)?;
        let cached_or_entry = self.create_binary(cached, SyntaxKind::BarBarToken, assignment)?;
        let name_parameter = self.create_parameter("name")?;
        let lookup = self.create_arrow(vec![name_parameter], cached_or_entry)?;
        let return_statement = self.create_return_statement(Some(lookup))?;
        let body = self.create_block(vec![cache_statement, return_statement], true)?;
        let geti_parameter = self.create_parameter("geti")?;
        let seti_parameter = self.create_parameter("seti")?;
        let function =
            self.create_function_expression(vec![geti_parameter, seti_parameter], body)?;
        let function = self.create_parenthesized(function)?;

        let name_parameter = self.create_parameter("name")?;
        let super_token = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::SuperKeyword,
            TransformFlags::NONE,
        )?;
        let name = self.create_identifier("name")?;
        let super_access = self.create_element_access(super_token, name)?;
        let getter = self.create_arrow(vec![name_parameter], super_access)?;

        let name_parameter = self.create_parameter("name")?;
        let value_parameter = self.create_parameter("value")?;
        let super_token = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::SuperKeyword,
            TransformFlags::NONE,
        )?;
        let name = self.create_identifier("name")?;
        let super_access = self.create_element_access(super_token, name)?;
        let value = self.create_identifier("value")?;
        let assignment = self.create_binary(super_access, SyntaxKind::EqualsToken, value)?;
        let setter = self.create_arrow(vec![name_parameter, value_parameter], assignment)?;
        self.create_call(function, vec![getter, setter])
    }

    fn merge_lexical_environment_into_block(
        &mut self,
        body: TransformNode,
        lexical_environment: LexicalEnvironment,
    ) -> Result<TransformNode, TransformError> {
        if lexical_environment.is_empty() {
            return Ok(body);
        }
        let NodeData::Block(mut data) = self.context.arena().node(body)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionExpression,
                field: "generator block",
            });
        };
        let mut statements = self.array_nodes(data.statements)?;
        let mut prologue_end = 0;
        while statements
            .get(prologue_end)
            .is_some_and(|statement| self.is_prologue_statement(*statement))
        {
            prologue_end += 1;
        }
        if !lexical_environment.variable_declarations().is_empty() {
            let declarations = lexical_environment
                .variable_declarations()
                .iter()
                .copied()
                .map(|name| self.create_variable_declaration(name, None))
                .collect::<Result<Vec<_>, _>>()?;
            let list = self.create_variable_declaration_list(declarations, NodeFlags::NONE)?;
            let statement = self.create_variable_statement_from_list(list)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.insert(prologue_end, statement);
        }
        if !lexical_environment.initialization_statements().is_empty() {
            statements.splice(
                prologue_end..prologue_end,
                lexical_environment
                    .initialization_statements()
                    .iter()
                    .copied(),
            );
        }
        if !lexical_environment.function_declarations().is_empty() {
            statements.splice(
                prologue_end..prologue_end,
                lexical_environment.function_declarations().iter().copied(),
            );
        }
        let statements = if let Some(original) = data.statements.map(|array| self.array(array)) {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(statements.array());
        let flags = flags_after_update(self.context.arena(), body, &NodeData::Block(data.clone()))?;
        self.context
            .factory()?
            .update_node(body, NodeData::Block(data), flags)
    }

    fn is_prologue_statement(&self, statement: TransformNode) -> bool {
        let Ok(NodeData::ExpressionStatement(data)) =
            self.context.arena().node(statement).map(|node| &node.data)
        else {
            return false;
        };
        data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|expression| matches!(expression.data, NodeData::StringLiteral(_)))
        })
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        try_visit_each_child(&mut data, self)?;
        self.update_without_visit(original, data)
    }

    fn update_without_visit(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        if self.context.arena().node(original)?.data == data {
            return Ok(original.node());
        }
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn visit_required(
        &mut self,
        node: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
    ) -> Result<TransformNode, TransformError> {
        let node = node.ok_or(TransformError::RequiredChildRemoved { parent, field })?;
        self.visit(node)?
            .map(|node| self.node(node))
            .ok_or(TransformError::RequiredChildRemoved { parent, field })
    }

    fn visit_optional_node(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.visit(node))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_optional_nodes(
        &mut self,
        nodes: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        nodes
            .map(|nodes| self.visit_node_array(nodes))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_node_array(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, TransformError> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(*mapped);
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(node) = self.visit(node)? {
                visited.push(self.node(node));
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        let mapped = Some(updated.array());
        self.arrays.insert(id, mapped);
        Ok(mapped)
    }

    fn array_nodes(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(array) =
            array.and_then(|array| self.context.arena().node_array_ref(self.source, array))
        else {
            return Ok(Vec::new());
        };
        self.context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .map(|node| {
                self.context
                    .arena()
                    .node_ref(self.source, *node)
                    .ok_or_else(|| TransformError::UnknownNode(self.node(*node)))
            })
            .collect()
    }

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_generated_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_identifier(binding.provisional_name())?;
        let metadata = self.context.arena_mut()?.metadata_mut(identifier);
        metadata.set_generated_binding_id(binding.id());
        if let Some(base) = binding.numbered_base() {
            metadata.set_generated_binding_base(base);
        }
        if let Some(base) = binding.preferred_base() {
            metadata.set_generated_binding_preferred_base(base);
        }
        if binding.reserve_in_nested_scopes() {
            metadata.reserve_generated_binding_in_nested_scopes();
        }
        Ok(identifier)
    }

    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        let flags = self.context.arena().array_transform_flags(elements);
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(elements.array()),
            }),
            flags,
        )
    }

    fn create_object_literal(
        &mut self,
        properties: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, properties)?;
        let flags = self.context.arena().array_transform_flags(properties);
        self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            flags,
        )
    }

    fn create_property_assignment(
        &mut self,
        name: TransformNode,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[name, initializer])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(initializer.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            flags,
        )
    }

    fn create_property_access(
        &mut self,
        expression: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[expression, name])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                name: Some(name.node()),
            }),
            flags,
        )
    }

    fn create_element_access(
        &mut self,
        expression: TransformNode,
        argument: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[expression, argument])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                argument_expression: Some(argument.node()),
            }),
            flags,
        )
    }

    fn create_parameter(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        let flags = self.context.arena().propagate_child_flags(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(name.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            flags,
        )
    }

    fn create_arrow(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        let arrow = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsGreaterThanToken,
            TransformFlags::NONE,
        )?;
        let flags = self.context.arena().array_transform_flags(parameters)
            | self.context.arena().propagate_child_flags(body)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrowFunction(tsc_syntax::nodes::ArrowFunctionData {
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
                equals_greater_than_token: Some(arrow.node()),
            }),
            flags,
        )
    }

    fn create_function_expression(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        let flags = self.context.arena().array_transform_flags(parameters)
            | self.context.arena().propagate_child_flags(body)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            flags,
        )
    }

    fn create_get_accessor(
        &mut self,
        name: &str,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        let flags = self.context.arena().propagate_child_flags(name)?
            | self.context.arena().propagate_child_flags(body)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::GetAccessor(tsc_syntax::nodes::GetAccessorData {
                name: Some(name.node()),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            flags,
        )
    }

    fn create_set_accessor(
        &mut self,
        name: &str,
        parameter: &str,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        let parameter = self.create_parameter(parameter)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![parameter])?;
        let flags = self.context.arena().propagate_child_flags(name)?
            | self.context.arena().array_transform_flags(parameters)
            | self.context.arena().propagate_child_flags(body)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::SetAccessor(tsc_syntax::nodes::SetAccessorData {
                name: Some(name.node()),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            flags,
        )
    }

    fn create_parenthesized(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let operator =
            self.context
                .factory()?
                .create_token(self.source, operator, TransformFlags::NONE)?;
        let flags = self.child_flags(&[left, operator, right])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(operator.node()),
                right: Some(right.node()),
            }),
            flags,
        )
    }

    fn inline_expressions(
        &mut self,
        expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut expressions = expressions.into_iter();
        let first = expressions
            .next()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CommaListExpression,
                field: "expression",
            })?;
        expressions.try_fold(first, |left, right| {
            self.create_binary(left, SyntaxKind::CommaToken, right)
        })
    }

    fn create_call(
        &mut self,
        callee: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        let flags = self.context.arena().propagate_child_flags(callee)?
            | self.context.arena().array_transform_flags(arguments);
        self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(callee.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            flags,
        )
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: "0".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        let flags = self.context.arena().propagate_child_flags(zero)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            flags,
        )
    }

    fn create_block(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        let flags = self.context.arena().array_transform_flags(statements);
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            flags,
        )?;
        self.context.factory()?.set_multi_line(block, multi_line)
    }

    fn create_return_statement(
        &mut self,
        expression: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let flags = expression
            .map(|expression| self.context.arena().propagate_child_flags(expression))
            .transpose()?
            .unwrap_or(TransformFlags::NONE)
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: expression.map(TransformNode::node),
            }),
            flags,
        )
    }

    fn create_variable_declaration(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut children = vec![name];
        children.extend(initializer);
        let flags = self.child_flags(&children)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(TransformNode::node),
            }),
            flags,
        )
    }

    fn create_variable_declaration_list(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let transform_flags = self.context.arena().array_transform_flags(declarations)
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        let list = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            transform_flags,
        )?;
        self.context.factory()?.set_node_flags(list, flags)
    }

    fn create_variable_statement_from_list(
        &mut self,
        list: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(list)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            flags,
        )
    }

    fn child_flags(&self, children: &[TransformNode]) -> Result<TransformFlags, TransformError> {
        children
            .iter()
            .try_fold(TransformFlags::NONE, |flags, child| {
                self.context
                    .arena()
                    .propagate_child_flags(*child)
                    .map(|child_flags| flags | child_flags)
            })
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        self.context.arena().require_parse_tree_resolver_node(node)
    }

    fn identifier_text(&self, node: TransformNode) -> Result<&str, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::Identifier(identifier) => Ok(&identifier.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAccessExpression,
                field: "identifier name",
            }),
        }
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for Es2017Visitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("ES2017 child belongs to the current transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        self.visit_node_array(id)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}

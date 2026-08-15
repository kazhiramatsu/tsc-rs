//! H2.5e ES2018-to-ES2017 lowering.
//!
//! `transformES2018` is used as the observable semantic specification.  The
//! Rust implementation separates function mode, destructuring plans, and
//! generated-binding ownership instead of reproducing the reference
//! implementation's nested mutable closures.

use std::collections::BTreeMap;

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    factory::EmitHelperName, EmitFlags, LexicalEnvironment, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer,
};

use super::{
    flags_after_update,
    generated_bindings::{
        AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes, GeneratedBindings,
    },
    initialize_transform_flags,
    target_bindings::{
        collect_untagged_identifier_texts, finalize_generated_binding_names, TargetBinding,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpressionValueUse {
    Required,
    Unused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FunctionMode {
    Ordinary,
    Async,
    Generator,
    AsyncGenerator,
}

impl FunctionMode {
    const fn is_async(self) -> bool {
        matches!(self, Self::Async | Self::AsyncGenerator)
    }

    const fn is_async_generator(self) -> bool {
        matches!(self, Self::AsyncGenerator)
    }
}

#[derive(Debug)]
struct ForAwaitLoweringPlan {
    mode: FunctionMode,
    done: TargetBinding,
    error_record: TargetBinding,
    return_method: TargetBinding,
    bound_value: TargetBinding,
    non_user_code: TargetBinding,
    iterator: TargetBinding,
    result: TargetBinding,
    catch_variable: TargetBinding,
    reset_error_record: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DestructuringMode {
    Binding,
    Assignment,
}

#[derive(Clone, Copy, Debug)]
struct DestructuringStep {
    target: TransformNode,
    value: TransformNode,
    original: Option<TransformNode>,
}

#[derive(Debug)]
struct DestructuringPlan {
    mode: DestructuringMode,
    helper_request_mode: HelperRequestMode,
    steps: Vec<DestructuringStep>,
    /// tsc's `hasTransformedPriorElement`: once an array element is
    /// deferred for object-rest lowering, every following non-simple
    /// element must be deferred as well so its initializer observes the
    /// bindings materialized by the earlier element.
    has_transformed_prior_array_element: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelperRequestMode {
    Immediate,
    AfterFunctionBody,
}

impl DestructuringPlan {
    fn new(mode: DestructuringMode) -> Self {
        Self {
            mode,
            helper_request_mode: HelperRequestMode::Immediate,
            steps: Vec::new(),
            has_transformed_prior_array_element: false,
        }
    }

    fn parameter_binding() -> Self {
        Self {
            mode: DestructuringMode::Binding,
            helper_request_mode: HelperRequestMode::AfterFunctionBody,
            steps: Vec::new(),
            has_transformed_prior_array_element: false,
        }
    }

    fn push(
        &mut self,
        target: TransformNode,
        value: TransformNode,
        original: Option<TransformNode>,
    ) {
        self.steps.push(DestructuringStep {
            target,
            value,
            original,
        });
    }
}

#[derive(Clone, Copy, Debug)]
struct PatternElement {
    original: TransformNode,
    target: TransformNode,
    property_name: Option<TransformNode>,
    initializer: Option<TransformNode>,
    rest: bool,
}

#[derive(Clone, Copy, Debug)]
enum ExcludedProperty {
    Named(TransformNode),
    Computed(TransformNode),
}

/// Runtime key classification after applying
/// `tryGetPropertyNameOfBindingOrAssignmentElement`. Bracketed string and
/// numeric literals are static property names in tsc; only the remaining
/// computed names own an evaluated key temporary.
#[derive(Clone, Copy, Debug)]
enum ObjectPatternPropertyKey {
    Static(TransformNode),
    Computed {
        wrapper: TransformNode,
        expression: TransformNode,
    },
}

struct TransformedFunction {
    modifiers: Option<NodeArrayId>,
    asterisk_token: Option<NodeId>,
    parameters: Option<NodeArrayId>,
    body: Option<NodeId>,
}

struct FunctionVisitOutcome {
    parameters: Option<NodeArrayId>,
    inner_parameters: Option<NodeArrayId>,
    body: Option<NodeId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParameterLoweringMode {
    Ordinary,
    AsyncGeneratorInner,
}

#[derive(Debug)]
struct AsyncGeneratorParameterPlan {
    outer: Option<NodeArrayId>,
    inner: Option<NodeArrayId>,
    deferred_helpers: DeferredParameterHelpers,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DeferredParameterHelpers {
    object_rest: bool,
}

impl DeferredParameterHelpers {
    fn request(self, context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.object_rest {
            context.request_emit_helper(super::helpers::object_rest())?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ParameterLoweringOutcome {
    parameters: Option<NodeArrayId>,
    deferred_helpers: DeferredParameterHelpers,
}

#[derive(Clone, Debug)]
struct CapturedSuperProperty {
    text: String,
}

#[derive(Debug, Default)]
struct AsyncGeneratorSuperCapture {
    binding: Option<TargetBinding>,
    index_binding: Option<TargetBinding>,
    properties: Vec<CapturedSuperProperty>,
    has_element_access: bool,
    has_assignment: bool,
    owns_access: bool,
}

#[derive(Debug, Default)]
struct AsyncGeneratorSuperFacts {
    captured_properties: Vec<CapturedSuperProperty>,
    has_element_access: bool,
    has_assignment: bool,
    owns_access: bool,
}

/// tsc-port: transformES2018 @6.0.3
/// tsc-hash: 945123a45837588d8ed155d8deaeaca7970bc2b50ce82768250a0de69a110b58
/// tsc-span: _tsc.js:101680-102905
pub(super) fn transform_es2018(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(Es2018Transformer {
        target: options.emit_script_target(),
    })
}

struct Es2018Transformer {
    target: ScriptTarget,
}

impl Transformer for Es2018Transformer {
    fn name(&self) -> &'static str {
        "transformES2018"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES2015 || self.target > ScriptTarget::ES2017 {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "transformES2018",
                detail: "H2.5g composes transformES2018 for ES2015 through ES2017 targets",
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
        context.start_lexical_environment()?;
        let current_root = context.arena().root(source)?;
        let mut visitor = Es2018Visitor::new(context, source, self.target, current_root)?;
        let transformed =
            visitor
                .visit(current_root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.node(transformed);
        let lexical_environment = visitor.context.end_lexical_environment()?;
        let generated_bindings = visitor.generated_bindings.source_bindings();
        visitor.assert_binding_plan(&generated_bindings, &lexical_environment);
        let transformed =
            visitor.merge_source_lexical_environment(transformed, lexical_environment)?;
        finalize_generated_binding_names(visitor.context, source, transformed)?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

struct Es2018Visitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    target: ScriptTarget,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    generated_bindings: GeneratedBindingScopes,
    value_use: ExpressionValueUse,
    function_stack: Vec<FunctionMode>,
    async_generator_super_captures: Vec<Option<AsyncGeneratorSuperCapture>>,
    iteration_depth: usize,
}

impl<'context> Es2018Visitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        target: ScriptTarget,
        root: TransformNode,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_untagged_identifier_texts(context.arena(), source, root)?,
                AncestorBindingPolicy::AllowShadow,
            ),
            context,
            source,
            target,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            value_use: ExpressionValueUse::Required,
            function_stack: Vec::new(),
            async_generator_super_captures: Vec::new(),
            iteration_depth: 0,
        })
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        // `discardedValueVisitor` applies to the expression at the current
        // boundary, not recursively to every descendant. For example, an
        // expression statement discards a call's result, while every call
        // argument (including an object-rest assignment used as a descriptor
        // value) is still required. Consume the pending mode exactly once and
        // restore ordinary required-value semantics for recursive children.
        let value_use = self.value_use;
        self.with_value_use(ExpressionValueUse::Required, |visitor| {
            visitor.visit_with_value_use(id, value_use)
        })
    }

    fn visit_with_value_use(
        &mut self,
        id: NodeId,
        value_use: ExpressionValueUse,
    ) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let requires_super_rewrite =
            self.super_capture_is_active() && self.node_is_direct_super_use(original)?;
        if !requires_super_rewrite
            && !self
                .context
                .arena()
                .transform_flags(original)
                .contains(TransformFlags::CONTAINS_ES_2018)
        {
            self.nodes.insert(id, Some(id));
            return Ok(Some(id));
        }

        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::ExpressionStatement(data) => {
                Some(self.visit_expression_statement(original, data)?)
            }
            NodeData::BinaryExpression(data)
                if self.binary_is_object_rest_destructuring(&data)? =>
            {
                Some(
                    self.flatten_destructuring_assignment(original, data, value_use)?
                        .node(),
                )
            }
            NodeData::BinaryExpression(data)
                if self.operator_kind(data.operator_token)? == Some(SyntaxKind::CommaToken) =>
            {
                Some(self.visit_comma_expression(original, data, value_use)?)
            }
            NodeData::CommaListExpression(data) => {
                Some(self.visit_comma_list_expression(original, data, value_use)?)
            }
            NodeData::ParenthesizedExpression(data) => {
                Some(self.visit_parenthesized_expression(original, data, value_use)?)
            }
            NodeData::VoidExpression(data) => Some(self.visit_void_expression(original, data)?),
            NodeData::VariableDeclarationList(data) => {
                Some(self.visit_variable_declaration_list(original, data)?)
            }
            NodeData::LabeledStatement(data)
                if self.labeled_statement_ends_in_for_await(&data)? =>
            {
                Some(self.visit_labeled_for_await_statement(original, data)?)
            }
            NodeData::ForOfStatement(data)
                if data.await_modifier.is_some() && self.current_function_is_async() =>
            {
                Some(self.visit_for_await_statement(original, data, Vec::new())?)
            }
            NodeData::ForOfStatement(data) if self.for_of_head_contains_object_rest(&data)? => {
                Some(self.visit_for_of_statement_with_object_rest(original, data)?)
            }
            NodeData::ForOfStatement(data) => {
                Some(self.visit_iteration_node(original, NodeData::ForOfStatement(data))?)
            }
            NodeData::ForInStatement(data) => {
                Some(self.visit_iteration_node(original, NodeData::ForInStatement(data))?)
            }
            NodeData::ForStatement(data) => Some(self.visit_for_statement(original, data)?),
            NodeData::WhileStatement(data) => {
                Some(self.visit_iteration_node(original, NodeData::WhileStatement(data))?)
            }
            NodeData::DoStatement(data) => {
                Some(self.visit_iteration_node(original, NodeData::DoStatement(data))?)
            }
            NodeData::AwaitExpression(data)
                if self
                    .current_function_mode()
                    .is_some_and(FunctionMode::is_async_generator) =>
            {
                Some(self.visit_async_generator_await(original, data)?)
            }
            NodeData::YieldExpression(data)
                if self
                    .current_function_mode()
                    .is_some_and(FunctionMode::is_async_generator) =>
            {
                Some(self.visit_async_generator_yield(original, data)?)
            }
            NodeData::ReturnStatement(data)
                if self
                    .current_function_mode()
                    .is_some_and(FunctionMode::is_async_generator) =>
            {
                Some(self.visit_async_generator_return(original, data)?)
            }
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
            NodeData::CatchClause(data) if self.catch_binding_contains_object_rest(&data)? => {
                Some(self.visit_catch_clause_with_object_rest(original, data)?)
            }
            NodeData::FunctionDeclaration(data) => {
                Some(self.visit_function_declaration(original, data)?)
            }
            NodeData::FunctionExpression(data) => {
                Some(self.visit_function_expression(original, data)?)
            }
            NodeData::ArrowFunction(data) => Some(self.visit_arrow_function(original, data)?),
            NodeData::ClassDeclaration(data) if self.super_capture_is_active() => {
                Some(self.visit_super_capture_boundary(original, NodeData::ClassDeclaration(data))?)
            }
            NodeData::ClassExpression(data) if self.super_capture_is_active() => {
                Some(self.visit_super_capture_boundary(original, NodeData::ClassExpression(data))?)
            }
            NodeData::MethodDeclaration(data) => {
                Some(self.visit_method_declaration(original, data)?)
            }
            NodeData::GetAccessor(data) => Some(self.visit_get_accessor(original, data)?),
            NodeData::SetAccessor(data) => Some(self.visit_set_accessor(original, data)?),
            NodeData::Constructor(data) => Some(self.visit_constructor(original, data)?),
            // `transformES2018` deliberately uses the propagated subtree bit,
            // not a scan for a direct `SpreadAssignment`. A prior transform
            // can synthesize an object literal around a value that still owns
            // object-rest syntax (class-field property descriptors are one
            // such composition). In that case tsc routes the one literal
            // chunk through `createAssignHelper`, preserving the observable
            // `Object.assign({...})` lookup and call.
            //
            // tsc-port: visitObjectLiteralExpression @6.0.3
            // tsc-span: _tsc.js:102002-102017
            NodeData::ObjectLiteralExpression(data)
                if self
                    .context
                    .arena()
                    .transform_flags(original)
                    .contains(TransformFlags::CONTAINS_OBJECT_REST_OR_SPREAD) =>
            {
                Some(self.visit_object_literal_expression(original, data)?.node())
            }
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_expression_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ExpressionStatementData,
    ) -> Result<NodeId, TransformError> {
        data.expression = self.with_value_use(ExpressionValueUse::Unused, |visitor| {
            visitor.visit_optional_node(data.expression)
        })?;
        self.update_without_visit(original, NodeData::ExpressionStatement(data))
    }

    fn visit_parenthesized_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ParenthesizedExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        data.expression = self.with_value_use(value_use, |visitor| {
            visitor.visit_optional_node(data.expression)
        })?;
        self.update_without_visit(original, NodeData::ParenthesizedExpression(data))
    }

    fn visit_comma_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BinaryExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        data.left = self.with_value_use(ExpressionValueUse::Unused, |visitor| {
            visitor.visit_optional_node(data.left)
        })?;
        data.right =
            self.with_value_use(value_use, |visitor| visitor.visit_optional_node(data.right))?;
        self.update_without_visit(original, NodeData::BinaryExpression(data))
    }

    fn visit_comma_list_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CommaListExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        if let Some(elements) = data.elements {
            let original_elements = self.array(elements);
            let nodes = self
                .context
                .arena()
                .node_array(original_elements)?
                .nodes
                .clone();
            let length = nodes.len();
            let mut visited = Vec::with_capacity(length);
            for (index, element) in nodes.into_iter().enumerate() {
                let element_value_use = if index + 1 < length {
                    ExpressionValueUse::Unused
                } else {
                    value_use
                };
                let element =
                    self.with_value_use(element_value_use, |visitor| visitor.visit(element))?;
                if let Some(element) = element {
                    visited.push(self.node(element));
                }
            }
            data.elements = Some(
                self.context
                    .factory()?
                    .update_node_array(original_elements, visited)?
                    .array(),
            );
        }
        self.update_without_visit(original, NodeData::CommaListExpression(data))
    }

    fn visit_void_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::VoidExpressionData,
    ) -> Result<NodeId, TransformError> {
        data.expression = self.with_value_use(ExpressionValueUse::Unused, |visitor| {
            visitor.visit_optional_node(data.expression)
        })?;
        self.update_without_visit(original, NodeData::VoidExpression(data))
    }

    fn visit_variable_declaration_list(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::VariableDeclarationListData,
    ) -> Result<NodeId, TransformError> {
        let declarations = self.array_nodes(data.declarations)?;
        let mut lowered = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let NodeData::VariableDeclaration(declaration_data) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclarationList,
                    field: "declaration",
                });
            };
            let name = declaration_data.name.map(|name| self.node(name)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclaration,
                    field: "name",
                },
            )?;
            if self.pattern_contains_object_rest(name)? {
                lowered.extend(self.flatten_destructuring_binding(
                    declaration,
                    declaration_data,
                    None,
                    false,
                    HelperRequestMode::Immediate,
                )?);
            } else if let Some(visited) = self.visit(declaration.node())? {
                lowered.push(self.node(visited));
            }
        }
        let original_array = data.declarations.map(|array| self.array(array));
        let declarations = if let Some(original_array) = original_array {
            self.context
                .factory()?
                .update_node_array(original_array, lowered)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, lowered)?
        };
        data.declarations = Some(declarations.array());
        self.update_without_visit(original, NodeData::VariableDeclarationList(data))
    }

    fn current_function_mode(&self) -> Option<FunctionMode> {
        self.function_stack.last().copied()
    }

    fn current_function_is_async(&self) -> bool {
        self.current_function_mode()
            .is_some_and(FunctionMode::is_async)
    }

    fn super_capture_is_active(&self) -> bool {
        self.async_generator_super_captures
            .last()
            .is_some_and(Option::is_some)
    }

    fn node_is_direct_super_use(&self, node: TransformNode) -> Result<bool, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::PropertyAccessExpression(data) => self.property_access_targets_super(data),
            NodeData::ElementAccessExpression(data) => self.element_access_targets_super(data),
            NodeData::CallExpression(data) => self.call_expression_targets_super(data),
            _ => Ok(false),
        }
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
        if !self.super_capture_is_active() {
            return Ok(false);
        }
        self.expression_is_super(data.expression)
    }

    fn element_access_targets_super(
        &self,
        data: &tsc_syntax::nodes::ElementAccessExpressionData,
    ) -> Result<bool, TransformError> {
        if !self.super_capture_is_active() {
            return Ok(false);
        }
        self.expression_is_super(data.expression)
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

    fn collect_async_generator_super_facts(
        &self,
        body: Option<NodeId>,
    ) -> Result<AsyncGeneratorSuperFacts, TransformError> {
        let mut facts = AsyncGeneratorSuperFacts::default();
        if let Some(body) = body {
            self.collect_async_generator_super_facts_from(
                self.node(body),
                true,
                false,
                &mut facts,
            )?;
        }
        Ok(facts)
    }

    fn collect_async_generator_super_facts_from(
        &self,
        node: TransformNode,
        owns_access: bool,
        assignment_target: bool,
        facts: &mut AsyncGeneratorSuperFacts,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(node)?.clone();
        let nested_mode = match &record.data {
            NodeData::FunctionDeclaration(data) => {
                Some(self.function_mode(data.modifiers, data.asterisk_token)?)
            }
            NodeData::FunctionExpression(data) => {
                Some(self.function_mode(data.modifiers, data.asterisk_token)?)
            }
            NodeData::MethodDeclaration(data) => {
                Some(self.function_mode(data.modifiers, data.asterisk_token)?)
            }
            _ => None,
        };
        if nested_mode.is_some_and(FunctionMode::is_async_generator) {
            return Ok(());
        }
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
                let property = self.identifier_text(name)?.to_owned();
                if !facts
                    .captured_properties
                    .iter()
                    .any(|captured| captured.text == property)
                {
                    facts
                        .captured_properties
                        .push(CapturedSuperProperty { text: property });
                }
                if owns_access {
                    facts.owns_access = true;
                    facts.has_assignment |= assignment_target;
                }
                return Ok(());
            }
            NodeData::ElementAccessExpression(data)
                if self.expression_is_super(data.expression)? =>
            {
                facts.has_element_access = true;
                if owns_access {
                    facts.owns_access = true;
                    facts.has_assignment |= assignment_target;
                }
                if let Some(argument) = data.argument_expression {
                    self.collect_async_generator_super_facts_from(
                        self.node(argument),
                        owns_access,
                        false,
                        facts,
                    )?;
                }
                return Ok(());
            }
            NodeData::BinaryExpression(data)
                if self
                    .operator_kind(data.operator_token)?
                    .is_some_and(|operator| {
                        operator.value() >= SyntaxKind::FirstAssignment.value()
                            && operator.value() <= SyntaxKind::LastAssignment.value()
                    }) =>
            {
                if let Some(left) = data.left {
                    self.collect_async_generator_super_facts_from(
                        self.node(left),
                        owns_access,
                        true,
                        facts,
                    )?;
                }
                if let Some(right) = data.right {
                    self.collect_async_generator_super_facts_from(
                        self.node(right),
                        owns_access,
                        false,
                        facts,
                    )?;
                }
                return Ok(());
            }
            NodeData::PrefixUnaryExpression(data)
                if matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ) =>
            {
                if let Some(operand) = data.operand {
                    self.collect_async_generator_super_facts_from(
                        self.node(operand),
                        owns_access,
                        true,
                        facts,
                    )?;
                }
                return Ok(());
            }
            NodeData::PostfixUnaryExpression(data)
                if matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ) =>
            {
                if let Some(operand) = data.operand {
                    self.collect_async_generator_super_facts_from(
                        self.node(operand),
                        owns_access,
                        true,
                        facts,
                    )?;
                }
                return Ok(());
            }
            _ => {}
        }

        let syntax = self.context.arena().source(self.source)?.syntax();
        let mut children = Vec::new();
        for_each_child(&syntax.arena, &record, |child| {
            children.push(child);
            false
        });
        for child in children {
            self.collect_async_generator_super_facts_from(
                self.node(child),
                owns_access,
                assignment_target,
                facts,
            )?;
        }
        Ok(())
    }

    fn visit_super_capture_boundary(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        self.async_generator_super_captures.push(None);
        let result = self.update_generic(original, data);
        let popped = self.async_generator_super_captures.pop();
        debug_assert!(matches!(popped, Some(None)));
        result
    }

    fn capture_super_property(
        &mut self,
        property: String,
    ) -> Result<TargetBinding, TransformError> {
        let needs_binding = self
            .async_generator_super_captures
            .last()
            .and_then(Option::as_ref)
            .is_some_and(|capture| capture.binding.is_none());
        let allocated = if needs_binding {
            Some(self.allocate_scoped_preferred_binding("_super")?)
        } else {
            None
        };
        let capture = self
            .async_generator_super_captures
            .last_mut()
            .and_then(Option::as_mut)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SuperKeyword,
                field: "active async-generator super capture",
            })?;
        if let Some(binding) = allocated {
            capture.binding = Some(binding);
        }
        if !capture
            .properties
            .iter()
            .any(|captured| captured.text == property)
        {
            capture
                .properties
                .push(CapturedSuperProperty { text: property });
        }
        capture
            .binding
            .clone()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SuperKeyword,
                field: "captured super binding",
            })
    }

    fn allocate_scoped_preferred_binding(
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

    fn capture_super_element(&mut self) -> Result<(TargetBinding, bool), TransformError> {
        let capture = self
            .async_generator_super_captures
            .last()
            .and_then(Option::as_ref)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SuperKeyword,
                field: "active async-generator super capture",
            })?;
        let binding =
            capture
                .index_binding
                .clone()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ElementAccessExpression,
                    field: "captured super index binding",
                })?;
        Ok((binding, capture.has_assignment))
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
        let binding = self.capture_super_property(property.clone())?;
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
        let (binding, has_assignment) = self.capture_super_element()?;
        let index = self.create_generated_identifier(&binding)?;
        let access = self.create_call(index, vec![argument])?;
        let access = if has_assignment {
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
            NodeData::PropertyAccessExpression(property_access) => {
                self.visit_captured_super_property(callee, property_access)?
            }
            NodeData::ElementAccessExpression(element_access) => {
                self.visit_captured_super_element(callee, element_access)?
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

    fn labeled_statement_ends_in_for_await(
        &self,
        data: &tsc_syntax::nodes::LabeledStatementData,
    ) -> Result<bool, TransformError> {
        if !self.current_function_is_async() {
            return Ok(false);
        }
        let Some(mut statement) = data.statement.map(|statement| self.node(statement)) else {
            return Ok(false);
        };
        loop {
            match &self.context.arena().node(statement)?.data {
                NodeData::LabeledStatement(data) => {
                    let Some(next) = data.statement else {
                        return Ok(false);
                    };
                    statement = self.node(next);
                }
                NodeData::ForOfStatement(data) => return Ok(data.await_modifier.is_some()),
                _ => return Ok(false),
            }
        }
    }

    fn visit_labeled_for_await_statement(
        &mut self,
        original: TransformNode,
        _data: tsc_syntax::nodes::LabeledStatementData,
    ) -> Result<NodeId, TransformError> {
        let mut labels = Vec::new();
        let mut statement = original;
        loop {
            match self.context.arena().node(statement)?.data.clone() {
                NodeData::LabeledStatement(data) => {
                    let label = data.label.map(|label| self.node(label)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::LabeledStatement,
                            field: "label",
                        },
                    )?;
                    labels.push((statement, label));
                    statement = data.statement.map(|statement| self.node(statement)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::LabeledStatement,
                            field: "statement",
                        },
                    )?;
                }
                NodeData::ForOfStatement(data) if data.await_modifier.is_some() => {
                    return self.visit_for_await_statement(statement, data, labels);
                }
                _ => {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::LabeledStatement,
                        field: "for-await statement",
                    });
                }
            }
        }
    }

    fn visit_iteration_node(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        self.iteration_depth += 1;
        let result = self.update_generic(original, data);
        self.iteration_depth -= 1;
        result
    }

    fn visit_for_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForStatementData,
    ) -> Result<NodeId, TransformError> {
        self.iteration_depth += 1;
        let result = (|| {
            data.initializer = self.with_value_use(ExpressionValueUse::Unused, |visitor| {
                visitor.visit_optional_node(data.initializer)
            })?;
            data.condition = self.with_value_use(ExpressionValueUse::Required, |visitor| {
                visitor.visit_optional_node(data.condition)
            })?;
            data.incrementor = self.with_value_use(ExpressionValueUse::Unused, |visitor| {
                visitor.visit_optional_node(data.incrementor)
            })?;
            data.statement = self.with_value_use(ExpressionValueUse::Required, |visitor| {
                visitor.visit_optional_node(data.statement)
            })?;
            self.update_without_visit(original, NodeData::ForStatement(data))
        })();
        self.iteration_depth -= 1;
        result
    }

    fn allocate_hoisted_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
        let binding =
            TargetBinding::allocate(self.context, self.generated_bindings.allocate_temp())?;
        let declaration = self.create_generated_identifier(&binding)?;
        self.context.hoist_variable_declaration(declaration)?;
        Ok(binding)
    }

    fn allocate_hoisted_numbered_binding(
        &mut self,
        source_name: &str,
    ) -> Result<TargetBinding, TransformError> {
        let name = self.generated_bindings.allocate_numbered(source_name);
        let binding = TargetBinding::allocate_numbered(self.context, source_name.to_owned(), name)?;
        let declaration = self.create_generated_identifier(&binding)?;
        self.context.hoist_variable_declaration(declaration)?;
        Ok(binding)
    }

    fn allocate_local_temp_binding(&mut self) -> Result<TargetBinding, TransformError> {
        TargetBinding::allocate(self.context, self.generated_bindings.allocate_local_temp())
    }

    fn allocate_local_numbered_binding(
        &mut self,
        source_name: &str,
    ) -> Result<TargetBinding, TransformError> {
        let name = self.generated_bindings.allocate_local_numbered(source_name);
        TargetBinding::allocate_numbered(self.context, source_name.to_owned(), name)
    }

    fn create_planned_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        self.create_generated_identifier(binding)
    }

    fn plan_for_await_lowering(
        &mut self,
        expression: TransformNode,
    ) -> Result<ForAwaitLoweringPlan, TransformError> {
        let mode = self
            .current_function_mode()
            .filter(|mode| mode.is_async())
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForOfStatement,
                field: "enclosing async function",
            })?;
        let done = self.allocate_hoisted_temp_binding()?;
        let error_record = self.allocate_hoisted_numbered_binding("e")?;
        let return_method = self.allocate_hoisted_temp_binding()?;
        let bound_value = self.allocate_hoisted_temp_binding()?;
        let non_user_code = self.allocate_local_temp_binding()?;

        let iterator_base = match &self.context.arena().node(expression)?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            _ => None,
        };
        let iterator = if let Some(iterator_base) = iterator_base {
            self.allocate_local_numbered_binding(&iterator_base)?
        } else {
            self.allocate_local_temp_binding()?
        };
        let result = if iterator.numbered_base().is_some() {
            self.allocate_local_numbered_binding(iterator.provisional_name())?
        } else {
            self.allocate_local_temp_binding()?
        };
        let catch_variable =
            self.allocate_local_numbered_binding(error_record.provisional_name())?;

        Ok(ForAwaitLoweringPlan {
            mode,
            done,
            error_record,
            return_method,
            bound_value,
            non_user_code,
            iterator,
            result,
            catch_variable,
            reset_error_record: self.iteration_depth > 0,
        })
    }

    fn visit_for_await_statement(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ForOfStatementData,
        labels: Vec<(TransformNode, TransformNode)>,
    ) -> Result<NodeId, TransformError> {
        let expression_original = data
            .expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForOfStatement,
                field: "expression",
            })?;
        let expression = self.with_value_use(ExpressionValueUse::Required, |visitor| {
            visitor.visit_required(
                Some(expression_original.node()),
                SyntaxKind::ForOfStatement,
                "expression",
            )
        })?;
        let plan = self.plan_for_await_lowering(expression)?;
        self.context
            .request_emit_helper(super::helpers::async_values())?;

        let async_values = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::AsyncValues)?;
        let iterator_source = self.create_call(async_values, vec![expression])?;
        self.context
            .factory()?
            .set_text_range(iterator_source, expression_original)?;
        let iterator_initializer = if plan.reset_error_record {
            let error = self.create_planned_identifier(&plan.error_record)?;
            let void_zero = self.create_void_zero()?;
            let reset = self.create_assignment(error, void_zero)?;
            let reset_then_iterator = self.inline_expressions(vec![reset, iterator_source])?;
            self.create_parenthesized(reset_then_iterator)?
        } else {
            iterator_source
        };

        let non_user_name = self.create_planned_identifier(&plan.non_user_code)?;
        let true_value = self.create_boolean(true)?;
        let non_user_declaration =
            self.create_variable_declaration(non_user_name, Some(true_value))?;
        let iterator_name = self.create_planned_identifier(&plan.iterator)?;
        let iterator_declaration =
            self.create_variable_declaration(iterator_name, Some(iterator_initializer))?;
        self.context
            .factory()?
            .set_text_range(iterator_declaration, expression_original)?;
        let result_name = self.create_planned_identifier(&plan.result)?;
        let result_declaration = self.create_variable_declaration(result_name, None)?;
        let loop_initializer = self.create_variable_declaration_list(
            vec![
                non_user_declaration,
                iterator_declaration,
                result_declaration,
            ],
            NodeFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(loop_initializer)
            .add_flags(EmitFlags::NO_HOISTING);
        self.context
            .factory()?
            .set_text_range(loop_initializer, expression_original)?;

        let iterator = self.create_planned_identifier(&plan.iterator)?;
        let next_name = self.create_identifier("next")?;
        let next = self.create_property_access(iterator, next_name)?;
        let next_call = self.create_call(next, Vec::new())?;
        let awaited_next = self.create_downlevel_await(plan.mode, next_call)?;
        let result = self.create_planned_identifier(&plan.result)?;
        let assign_result = self.create_assignment(result, awaited_next)?;
        let result = self.create_planned_identifier(&plan.result)?;
        let done_name = self.create_identifier("done")?;
        let done_property = self.create_property_access(result, done_name)?;
        let done = self.create_planned_identifier(&plan.done)?;
        let assign_done = self.create_assignment(done, done_property)?;
        let done = self.create_planned_identifier(&plan.done)?;
        let not_done = self.create_logical_not(done)?;
        let condition = self.inline_expressions(vec![assign_result, assign_done, not_done])?;

        let non_user = self.create_planned_identifier(&plan.non_user_code)?;
        let true_value = self.create_boolean(true)?;
        let incrementor = self.create_assignment(non_user, true_value)?;
        let body = self.create_for_await_body(original, &data, &plan)?;
        let for_statement = self.create_for_statement(
            Some(loop_initializer),
            Some(condition),
            Some(incrementor),
            body,
        )?;
        self.set_original_and_range(for_statement, original)?;
        self.context
            .arena_mut()?
            .metadata_mut(for_statement)
            .add_flags(EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS);

        let range_owner = labels.first().map(|(owner, _)| *owner).unwrap_or(original);
        self.mark_enclosing_block_multi_line(range_owner)?;
        let mut labeled = for_statement;
        for (label_owner, label) in labels.into_iter().rev() {
            labeled = self.create_labeled_statement(label, labeled)?;
            self.context
                .factory()?
                .set_text_range(labeled, label_owner)?;
            self.context
                .arena_mut()?
                .set_original_node(labeled, Some(label_owner))?;
        }

        let try_block = self.create_block(vec![labeled], true)?;
        let catch_clause = self.create_for_await_catch_clause(&plan)?;
        let finally_block = self.create_for_await_finally_block(&plan)?;
        let lowered =
            self.create_try_statement(try_block, Some(catch_clause), Some(finally_block))?;
        self.set_original_and_range(lowered, range_owner)
            .map(TransformNode::node)
    }

    fn mark_enclosing_block_multi_line(
        &mut self,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let mut current = node;
        while let Some(parent) = self.context.arena().node(current)?.parent {
            let parent = self.node(parent);
            if self.context.arena().node(parent)?.kind == SyntaxKind::Block {
                let owns_iteration_body = self
                    .context
                    .arena()
                    .node(parent)?
                    .parent
                    .and_then(|owner| self.context.arena().node_ref(self.source, owner))
                    .is_some_and(|owner| {
                        self.context.arena().node(owner).is_ok_and(|owner| {
                            matches!(
                                owner.kind,
                                SyntaxKind::DoStatement
                                    | SyntaxKind::ForInStatement
                                    | SyntaxKind::ForOfStatement
                                    | SyntaxKind::ForStatement
                                    | SyntaxKind::WhileStatement
                            )
                        })
                    });
                if owns_iteration_body {
                    self.context.factory()?.set_multi_line(parent, true)?;
                }
                break;
            }
            current = parent;
        }
        Ok(())
    }

    fn create_for_await_body(
        &mut self,
        original: TransformNode,
        data: &tsc_syntax::nodes::ForOfStatementData,
        plan: &ForAwaitLoweringPlan,
    ) -> Result<TransformNode, TransformError> {
        let result = self.create_planned_identifier(&plan.result)?;
        let value_name = self.create_identifier("value")?;
        let value = self.create_property_access(result, value_name)?;
        let bound = self.create_planned_identifier(&plan.bound_value)?;
        let assign_value = self.create_assignment(bound, value)?;
        let value_statement = self.create_expression_statement(assign_value)?;

        let non_user = self.create_planned_identifier(&plan.non_user_code)?;
        let false_value = self.create_boolean(false)?;
        let exit_non_user = self.create_assignment(non_user, false_value)?;
        let exit_statement = self.create_expression_statement(exit_non_user)?;

        let initializer = data
            .initializer
            .map(|initializer| self.node(initializer))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForOfStatement,
                field: "initializer",
            })?;
        let binding_value = self.create_planned_identifier(&plan.bound_value)?;
        let binding_statement =
            self.create_for_await_binding_statement(initializer, binding_value)?;
        let mut statements = vec![value_statement, exit_statement, binding_statement];

        self.iteration_depth += 1;
        let visited_body = data
            .statement
            .map(|statement| self.visit(statement))
            .transpose();
        self.iteration_depth -= 1;
        let visited_body = visited_body?.flatten().map(|body| self.node(body)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForOfStatement,
                field: "statement",
            },
        )?;
        if let NodeData::Block(block) = self.context.arena().node(visited_body)?.data.clone() {
            statements.extend(self.array_nodes(block.statements)?);
        } else {
            statements.push(visited_body);
        }
        let body = self.create_block(statements, true)?;
        self.context.factory()?.set_text_range(body, original)?;
        Ok(body)
    }

    fn create_for_await_binding_statement(
        &mut self,
        initializer: TransformNode,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let statement = match self.context.arena().node(initializer)?.data.clone() {
            NodeData::VariableDeclarationList(mut list) => {
                let declarations = self.array_nodes(list.declarations)?;
                let mut rebound = Vec::with_capacity(declarations.len());
                for (index, declaration) in declarations.into_iter().enumerate() {
                    let NodeData::VariableDeclaration(mut data) =
                        self.context.arena().node(declaration)?.data.clone()
                    else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::VariableDeclarationList,
                            field: "declaration",
                        });
                    };
                    data.initializer = (index == 0).then_some(value.node());
                    let flags = flags_after_update(
                        self.context.arena(),
                        declaration,
                        &NodeData::VariableDeclaration(data.clone()),
                    )?;
                    rebound.push(self.context.factory()?.update_node(
                        declaration,
                        NodeData::VariableDeclaration(data),
                        flags,
                    )?);
                }
                let declarations = self
                    .context
                    .factory()?
                    .create_node_array(self.source, rebound)?;
                list.declarations = Some(declarations.array());
                let flags = flags_after_update(
                    self.context.arena(),
                    initializer,
                    &NodeData::VariableDeclarationList(list.clone()),
                )?;
                let list = self.context.factory()?.update_node(
                    initializer,
                    NodeData::VariableDeclarationList(list),
                    flags,
                )?;
                self.create_variable_statement_from_list(list)?
            }
            _ => {
                let assignment = self.create_assignment(initializer, value)?;
                self.create_expression_statement(assignment)?
            }
        };
        self.visit(statement.node())?
            .map(|statement| self.node(statement))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForOfStatement,
                field: "binding statement",
            })
    }

    fn create_for_await_catch_clause(
        &mut self,
        plan: &ForAwaitLoweringPlan,
    ) -> Result<TransformNode, TransformError> {
        let catch_name = self.create_planned_identifier(&plan.catch_variable)?;
        let declaration = self.create_variable_declaration(catch_name, None)?;
        let error_name = self.create_identifier("error")?;
        let catch_value = self.create_planned_identifier(&plan.catch_variable)?;
        let error_property = self.create_property_assignment(error_name, catch_value)?;
        let error_object = self.create_object_literal(vec![error_property])?;
        let error_record = self.create_planned_identifier(&plan.error_record)?;
        let assignment = self.create_assignment(error_record, error_object)?;
        let statement = self.create_expression_statement(assignment)?;
        let block = self.create_block(vec![statement], false)?;
        self.context
            .arena_mut()?
            .metadata_mut(block)
            .add_flags(EmitFlags::SINGLE_LINE);
        self.create_catch_clause(declaration, block)
    }

    fn create_for_await_finally_block(
        &mut self,
        plan: &ForAwaitLoweringPlan,
    ) -> Result<TransformNode, TransformError> {
        let non_user = self.create_planned_identifier(&plan.non_user_code)?;
        let not_non_user = self.create_logical_not(non_user)?;
        let done = self.create_planned_identifier(&plan.done)?;
        let not_done = self.create_logical_not(done)?;
        let condition =
            self.create_binary(not_non_user, SyntaxKind::AmpersandAmpersandToken, not_done)?;
        let iterator = self.create_planned_identifier(&plan.iterator)?;
        let return_name = self.create_identifier("return")?;
        let return_property = self.create_property_access(iterator, return_name)?;
        let return_method = self.create_planned_identifier(&plan.return_method)?;
        let assign_return = self.create_assignment(return_method, return_property)?;
        let assign_return = self.create_parenthesized(assign_return)?;
        let condition = self.create_binary(
            condition,
            SyntaxKind::AmpersandAmpersandToken,
            assign_return,
        )?;

        let return_method = self.create_planned_identifier(&plan.return_method)?;
        let call_name = self.create_identifier("call")?;
        let call = self.create_property_access(return_method, call_name)?;
        let iterator = self.create_planned_identifier(&plan.iterator)?;
        let call = self.create_call(call, vec![iterator])?;
        let await_return = self.create_downlevel_await(plan.mode, call)?;
        let return_statement = self.create_expression_statement(await_return)?;
        self.context
            .arena_mut()?
            .metadata_mut(return_statement)
            .add_flags(EmitFlags::SINGLE_LINE);
        let if_return = self.create_if_statement(condition, return_statement)?;
        self.context
            .arena_mut()?
            .metadata_mut(if_return)
            .add_flags(EmitFlags::SINGLE_LINE);
        let inner_try_block = self.create_block(vec![if_return], true)?;

        let error_record = self.create_planned_identifier(&plan.error_record)?;
        let error_value = self.create_planned_identifier(&plan.error_record)?;
        let error_name = self.create_identifier("error")?;
        let error = self.create_property_access(error_value, error_name)?;
        let throw = self.create_throw_statement(error)?;
        self.context
            .arena_mut()?
            .metadata_mut(throw)
            .add_flags(EmitFlags::SINGLE_LINE);
        let if_error = self.create_if_statement(error_record, throw)?;
        self.context
            .arena_mut()?
            .metadata_mut(if_error)
            .add_flags(EmitFlags::SINGLE_LINE);
        let inner_finally = self.create_block(vec![if_error], false)?;
        self.context
            .arena_mut()?
            .metadata_mut(inner_finally)
            .add_flags(EmitFlags::SINGLE_LINE);
        let inner_try = self.create_try_statement(inner_try_block, None, Some(inner_finally))?;
        self.create_block(vec![inner_try], true)
    }

    fn for_of_head_contains_object_rest(
        &self,
        data: &tsc_syntax::nodes::ForOfStatementData,
    ) -> Result<bool, TransformError> {
        let Some(initializer) = data.initializer.map(|initializer| self.node(initializer)) else {
            return Ok(false);
        };
        match &self.context.arena().node(initializer)?.data {
            NodeData::VariableDeclarationList(list) => {
                for declaration in self.array_nodes(list.declarations)? {
                    if let NodeData::VariableDeclaration(declaration) =
                        &self.context.arena().node(declaration)?.data
                    {
                        if let Some(name) = declaration.name.map(|name| self.node(name)) {
                            if self.pattern_contains_object_rest(name)? {
                                return Ok(true);
                            }
                        }
                    }
                }
                Ok(false)
            }
            _ => self.pattern_contains_object_rest(initializer),
        }
    }

    fn visit_for_of_statement_with_object_rest(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForOfStatementData,
    ) -> Result<NodeId, TransformError> {
        let initializer = data
            .initializer
            .map(|initializer| self.node(initializer))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ForOfStatement,
                field: "initializer",
            })?;
        let binding =
            TargetBinding::allocate(self.context, self.generated_bindings.allocate_local_temp())?;
        let declaration_name = self.create_generated_identifier(&binding)?;
        let value_name = self.create_generated_identifier(&binding)?;
        let loop_declaration = self.create_variable_declaration(declaration_name, None)?;
        let loop_list =
            self.create_variable_declaration_list(vec![loop_declaration], NodeFlags::LET)?;
        self.context
            .factory()?
            .set_text_range(loop_list, initializer)?;
        data.initializer = Some(loop_list.node());
        data.await_modifier = self.visit_optional_node(data.await_modifier)?;
        data.expression = self.with_value_use(ExpressionValueUse::Required, |visitor| {
            visitor.visit_optional_node(data.expression)
        })?;

        let binding_statement = match self.context.arena().node(initializer)?.data.clone() {
            NodeData::VariableDeclarationList(list) => {
                let declarations = self.array_nodes(list.declarations)?;
                let mut rebound = Vec::with_capacity(declarations.len());
                for (index, declaration) in declarations.into_iter().enumerate() {
                    let NodeData::VariableDeclaration(mut declaration_data) =
                        self.context.arena().node(declaration)?.data.clone()
                    else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::VariableDeclarationList,
                            field: "declaration",
                        });
                    };
                    declaration_data.initializer = (index == 0).then_some(value_name.node());
                    let flags = flags_after_update(
                        self.context.arena(),
                        declaration,
                        &NodeData::VariableDeclaration(declaration_data.clone()),
                    )?;
                    rebound.push(self.context.factory()?.update_node(
                        declaration,
                        NodeData::VariableDeclaration(declaration_data),
                        flags,
                    )?);
                }
                let rebound = self
                    .context
                    .factory()?
                    .create_node_array(self.source, rebound)?;
                let mut list_data = list;
                list_data.declarations = Some(rebound.array());
                let flags = flags_after_update(
                    self.context.arena(),
                    initializer,
                    &NodeData::VariableDeclarationList(list_data.clone()),
                )?;
                let list = self.context.factory()?.update_node(
                    initializer,
                    NodeData::VariableDeclarationList(list_data),
                    flags,
                )?;
                let statement = self.create_variable_statement_from_list(list)?;
                self.visit(statement.node())?
                    .map(|statement| self.node(statement))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ForOfStatement,
                        field: "binding statement",
                    })?
            }
            _ => {
                let assignment = self.create_assignment(initializer, value_name)?;
                let statement = self.create_expression_statement(assignment)?;
                self.visit(statement.node())?
                    .map(|statement| self.node(statement))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ForOfStatement,
                        field: "assignment binding statement",
                    })?
            }
        };

        let mut statements = vec![binding_statement];
        if let Some(statement) = data.statement {
            let visited = self
                .visit(statement)?
                .map(|statement| self.node(statement))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForOfStatement,
                    field: "statement",
                })?;
            if let NodeData::Block(block) = self.context.arena().node(visited)?.data.clone() {
                statements.extend(self.array_nodes(block.statements)?);
            } else {
                statements.push(visited);
            }
        }
        let body = self.create_block(statements, true)?;
        data.statement = Some(body.node());
        self.update_without_visit(original, NodeData::ForOfStatement(data))
    }

    fn visit_async_generator_await(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::AwaitExpressionData,
    ) -> Result<NodeId, TransformError> {
        let expression =
            self.visit_required(data.expression, SyntaxKind::AwaitExpression, "expression")?;
        let lowered = self.create_downlevel_await(FunctionMode::AsyncGenerator, expression)?;
        self.set_original_and_range(lowered, original)
            .map(TransformNode::node)
    }

    fn visit_async_generator_yield(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::YieldExpressionData,
    ) -> Result<NodeId, TransformError> {
        let expression = data
            .expression
            .map(|expression| {
                self.visit_required(Some(expression), SyntaxKind::YieldExpression, "expression")
            })
            .transpose()?;
        let lowered = if data.asterisk_token.is_some() {
            let expression = expression.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::YieldExpression,
                field: "yield-star expression",
            })?;
            self.context
                .request_emit_helper(super::helpers::async_values())?;
            self.context
                .request_emit_helper(super::helpers::async_delegator())?;
            let async_values = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::AsyncValues)?;
            let values = self.create_call(async_values, vec![expression])?;
            self.context.factory()?.set_text_range(values, expression)?;
            let async_delegator = self
                .context
                .factory()?
                .create_unscoped_helper_identifier(self.source, EmitHelperName::AsyncDelegator)?;
            let delegated = self.create_call(async_delegator, vec![values])?;
            self.context
                .factory()?
                .set_text_range(delegated, expression)?;
            let asterisk = data.asterisk_token.map(|asterisk| self.node(asterisk));
            let delegated_yield = self.create_yield_expression(asterisk, Some(delegated))?;
            self.create_downlevel_await(FunctionMode::AsyncGenerator, delegated_yield)?
        } else {
            let expression = expression.unwrap_or(self.create_void_zero()?);
            let awaited = self.create_downlevel_await(FunctionMode::AsyncGenerator, expression)?;
            self.create_yield_expression(None, Some(awaited))?
        };
        self.set_original_and_range(lowered, original)
            .map(TransformNode::node)
    }

    fn visit_async_generator_return(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ReturnStatementData,
    ) -> Result<NodeId, TransformError> {
        let expression = data
            .expression
            .map(|expression| {
                self.visit_required(Some(expression), SyntaxKind::ReturnStatement, "expression")
            })
            .transpose()?
            .unwrap_or(self.create_void_zero()?);
        let expression = self.create_downlevel_await(FunctionMode::AsyncGenerator, expression)?;
        data.expression = Some(expression.node());
        self.update_without_visit(original, NodeData::ReturnStatement(data))
    }

    fn catch_binding_contains_object_rest(
        &self,
        data: &tsc_syntax::nodes::CatchClauseData,
    ) -> Result<bool, TransformError> {
        let Some(variable) = data
            .variable_declaration
            .map(|declaration| self.node(declaration))
        else {
            return Ok(false);
        };
        let NodeData::VariableDeclaration(data) = &self.context.arena().node(variable)?.data else {
            return Ok(false);
        };
        let Some(name) = data.name else {
            return Ok(false);
        };
        self.pattern_contains_object_rest(self.node(name))
    }

    fn visit_catch_clause_with_object_rest(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CatchClauseData,
    ) -> Result<NodeId, TransformError> {
        let variable = data
            .variable_declaration
            .map(|declaration| self.node(declaration))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CatchClause,
                field: "variable_declaration",
            })?;
        let NodeData::VariableDeclaration(mut variable_data) =
            self.context.arena().node(variable)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CatchClause,
                field: "variable declaration data",
            });
        };
        let pattern = variable_data.name.map(|name| self.node(name)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::VariableDeclaration,
                field: "name",
            },
        )?;
        let binding =
            TargetBinding::allocate(self.context, self.generated_bindings.allocate_local_temp())?;
        let catch_name = self.create_generated_identifier(&binding)?;
        let binding_value = self.create_generated_identifier(&binding)?;
        let declaration = tsc_syntax::nodes::VariableDeclarationData {
            name: Some(pattern.node()),
            exclamation_token: None,
            r#type: None,
            initializer: None,
        };
        let declarations = self.flatten_destructuring_binding(
            variable,
            declaration,
            Some(binding_value),
            true,
            HelperRequestMode::Immediate,
        )?;
        variable_data.name = Some(catch_name.node());
        variable_data.exclamation_token = None;
        variable_data.r#type = None;
        variable_data.initializer = None;
        let variable_flags = flags_after_update(
            self.context.arena(),
            variable,
            &NodeData::VariableDeclaration(variable_data.clone()),
        )?;
        let variable = self.context.factory()?.update_node(
            variable,
            NodeData::VariableDeclaration(variable_data),
            variable_flags,
        )?;
        data.variable_declaration = Some(variable.node());

        let block = self.visit_required(data.block, SyntaxKind::CatchClause, "block")?;
        let NodeData::Block(mut block_data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CatchClause,
                field: "block data",
            });
        };
        if !declarations.is_empty() {
            let statement = self.create_variable_statement(declarations)?;
            let mut statements = vec![statement];
            statements.extend(self.array_nodes(block_data.statements)?);
            let original_statements = block_data.statements.map(|array| self.array(array));
            let statements = if let Some(original_statements) = original_statements {
                self.context
                    .factory()?
                    .update_node_array(original_statements, statements)?
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, statements)?
            };
            block_data.statements = Some(statements.array());
        }
        let block_flags = flags_after_update(
            self.context.arena(),
            block,
            &NodeData::Block(block_data.clone()),
        )?;
        let block =
            self.context
                .factory()?
                .update_node(block, NodeData::Block(block_data), block_flags)?;
        data.block = Some(block.node());
        self.update_without_visit(original, NodeData::CatchClause(data))
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
            SyntaxKind::FunctionDeclaration,
            data.name.map(|name| self.node(name)),
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
            SyntaxKind::FunctionExpression,
            data.name.map(|name| self.node(name)),
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
            SyntaxKind::ArrowFunction,
            None,
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
            SyntaxKind::MethodDeclaration,
            data.name.map(|name| self.node(name)),
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
            SyntaxKind::GetAccessor,
            None,
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
            SyntaxKind::SetAccessor,
            None,
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
            SyntaxKind::Constructor,
            None,
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

    fn transform_function(
        &mut self,
        kind: SyntaxKind,
        name: Option<TransformNode>,
        modifiers: Option<NodeArrayId>,
        asterisk_token: Option<NodeId>,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
    ) -> Result<TransformedFunction, TransformError> {
        let mode = self.function_mode(modifiers, asterisk_token)?;
        let (previous_scope, scope) = self
            .generated_bindings
            .enter(GeneratedBindingOwner::FunctionBody);
        self.context.start_lexical_environment()?;
        self.function_stack.push(mode);
        let owns_super_capture = mode.is_async_generator();
        let introduces_super_boundary = owns_super_capture || kind != SyntaxKind::ArrowFunction;
        if introduces_super_boundary {
            let capture = if owns_super_capture {
                let facts = self.collect_async_generator_super_facts(body)?;
                let binding = facts
                    .owns_access
                    .then(|| self.allocate_scoped_preferred_binding("_super"))
                    .transpose()?;
                let index_binding = (facts.owns_access && facts.has_element_access)
                    .then(|| self.allocate_scoped_preferred_binding("_superIndex"))
                    .transpose()?;
                Some(AsyncGeneratorSuperCapture {
                    binding,
                    index_binding,
                    properties: facts.captured_properties,
                    has_element_access: facts.has_element_access,
                    has_assignment: facts.has_assignment,
                    owns_access: facts.owns_access,
                })
            } else {
                None
            };
            self.async_generator_super_captures.push(capture);
        }
        let operation: Result<FunctionVisitOutcome, TransformError> = (|| {
            if mode.is_async_generator() {
                let plan = self.plan_async_generator_parameters(parameters)?;
                let body = self.visit_optional_node(body)?;
                plan.deferred_helpers.request(self.context)?;
                Ok(FunctionVisitOutcome {
                    parameters: plan.outer,
                    inner_parameters: plan.inner,
                    body,
                })
            } else {
                let outcome = self.visit_parameter_list(parameters)?;
                let body = self.visit_optional_node(body)?;
                outcome.deferred_helpers.request(self.context)?;
                Ok(FunctionVisitOutcome {
                    parameters: outcome.parameters,
                    inner_parameters: None,
                    body,
                })
            }
        })();
        let super_capture = introduces_super_boundary
            .then(|| self.async_generator_super_captures.pop())
            .flatten();
        debug_assert_eq!(
            super_capture.is_some(),
            introduces_super_boundary,
            "each function-owned super boundary is balanced",
        );
        let popped = self.function_stack.pop();
        debug_assert_eq!(popped, Some(mode));
        let lexical_environment = self.context.end_lexical_environment();
        let generated_bindings = self.generated_bindings.exit(previous_scope, scope);
        let FunctionVisitOutcome {
            parameters,
            inner_parameters,
            body,
        } = operation?;
        let lexical_environment = lexical_environment?;
        self.assert_binding_plan(&generated_bindings, &lexical_environment);
        let body = self.merge_function_lexical_environment(kind, body, lexical_environment)?;

        if mode.is_async_generator() {
            let modifiers = self.visit_modifier_array_without_async(modifiers)?;
            let capture = super_capture
                .flatten()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: kind,
                    field: "async-generator super capture",
                })?;
            let body =
                self.create_async_generator_outer_body(name, inner_parameters, body, capture)?;
            Ok(TransformedFunction {
                modifiers,
                asterisk_token: None,
                parameters,
                body: Some(body.node()),
            })
        } else {
            Ok(TransformedFunction {
                modifiers: self.visit_optional_nodes(modifiers)?,
                asterisk_token: self.visit_optional_node(asterisk_token)?,
                parameters,
                body,
            })
        }
    }

    fn function_mode(
        &self,
        modifiers: Option<NodeArrayId>,
        asterisk_token: Option<NodeId>,
    ) -> Result<FunctionMode, TransformError> {
        let is_async = self.array_nodes(modifiers)?.iter().any(|modifier| {
            self.context
                .arena()
                .node(*modifier)
                .is_ok_and(|modifier| modifier.kind == SyntaxKind::AsyncKeyword)
        });
        Ok(match (is_async, asterisk_token.is_some()) {
            (false, false) => FunctionMode::Ordinary,
            (true, false) => FunctionMode::Async,
            (false, true) => FunctionMode::Generator,
            (true, true) => FunctionMode::AsyncGenerator,
        })
    }

    fn visit_parameter_list(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<ParameterLoweringOutcome, TransformError> {
        self.visit_parameter_list_with_mode(parameters, ParameterLoweringMode::Ordinary)
    }

    fn plan_async_generator_parameters(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<AsyncGeneratorParameterPlan, TransformError> {
        if self.parameter_list_is_simple(parameters)? {
            let outcome = self.visit_parameter_list(parameters)?;
            return Ok(AsyncGeneratorParameterPlan {
                outer: outcome.parameters,
                inner: None,
                deferred_helpers: outcome.deferred_helpers,
            });
        }

        let outer = self.create_async_generator_forwarding_parameters(parameters)?;
        let outcome = self.visit_parameter_list_with_mode(
            parameters,
            ParameterLoweringMode::AsyncGeneratorInner,
        )?;
        Ok(AsyncGeneratorParameterPlan {
            outer,
            inner: outcome.parameters,
            deferred_helpers: outcome.deferred_helpers,
        })
    }

    fn parameter_list_is_simple(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for parameter in self.array_nodes(parameters)? {
            let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "parameter",
                });
            };
            let simple_name = data.name.map(|name| self.node(name)).is_some_and(|name| {
                self.context
                    .arena()
                    .node(name)
                    .is_ok_and(|name| name.kind == SyntaxKind::Identifier)
            });
            if data.initializer.is_some() || data.dot_dot_dot_token.is_some() || !simple_name {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn create_async_generator_forwarding_parameters(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let original = self.array(parameters);
        let mut forwarded = Vec::new();
        for parameter in self.array_nodes(Some(parameters))? {
            let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "parameter",
                });
            };
            if data.initializer.is_some() || data.dot_dot_dot_token.is_some() {
                break;
            }
            let name = data.name.map(|name| self.node(name)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "name",
                },
            )?;
            let binding =
                if let NodeData::Identifier(identifier) = &self.context.arena().node(name)?.data {
                    let provisional = self
                        .generated_bindings
                        .allocate_local_numbered(&identifier.text);
                    TargetBinding::allocate_numbered_reserved_in_nested_scopes(
                        self.context,
                        identifier.text.clone(),
                        provisional,
                    )?
                } else {
                    let provisional = self.generated_bindings.allocate_local_temp();
                    TargetBinding::allocate_reserved_in_nested_scopes(self.context, provisional)?
                };
            let name = self.create_generated_identifier(&binding)?;
            let flags = self.context.arena().propagate_child_flags(name)?;
            forwarded.push(self.context.factory()?.create_node(
                self.source,
                NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                    modifiers: None,
                    dot_dot_dot_token: None,
                    name: Some(name.node()),
                    question_token: None,
                    r#type: None,
                    initializer: None,
                }),
                flags,
            )?);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, forwarded)?
                .array(),
        ))
    }

    fn visit_parameter_list_with_mode(
        &mut self,
        parameters: Option<NodeArrayId>,
        mode: ParameterLoweringMode,
    ) -> Result<ParameterLoweringOutcome, TransformError> {
        let Some(parameters) = parameters else {
            return Ok(ParameterLoweringOutcome {
                parameters: None,
                deferred_helpers: DeferredParameterHelpers::default(),
            });
        };
        let original = self.array(parameters);
        let mut visited = Vec::new();
        let mut follows_object_rest = false;
        let mut deferred_helpers = DeferredParameterHelpers::default();
        for parameter in self.array_nodes(Some(parameters))? {
            let NodeData::Parameter(mut data) = self.context.arena().node(parameter)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "parameter",
                });
            };
            let name = data.name.map(|name| self.node(name)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "name",
                },
            )?;
            if follows_object_rest {
                deferred_helpers.object_rest |= self.pattern_contains_object_rest(name)?;
                visited.push(self.visit_parameter_after_object_rest(parameter, data, name, mode)?);
            } else if self.pattern_contains_object_rest(name)? {
                follows_object_rest = true;
                deferred_helpers.object_rest = true;
                let binding = TargetBinding::allocate(
                    self.context,
                    self.generated_bindings.allocate_local_temp(),
                )?;
                let parameter_name = self.create_generated_identifier(&binding)?;
                let binding_value = self.create_generated_identifier(&binding)?;
                data.name = Some(parameter_name.node());
                data.modifiers = self.visit_optional_nodes(data.modifiers)?;
                data.dot_dot_dot_token = self.visit_optional_node(data.dot_dot_dot_token)?;
                data.question_token = self.visit_optional_node(data.question_token)?;
                data.r#type = self.visit_optional_node(data.r#type)?;
                data.initializer = self
                    .with_value_use(ExpressionValueUse::Required, |visitor| {
                        visitor.visit_optional_node(data.initializer)
                    })?;
                let declaration = tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                };
                let declarations = self.flatten_destructuring_binding(
                    parameter,
                    declaration,
                    Some(binding_value),
                    true,
                    HelperRequestMode::AfterFunctionBody,
                )?;
                if !declarations.is_empty() {
                    let statement = self.create_variable_statement(declarations)?;
                    self.context
                        .arena_mut()?
                        .metadata_mut(statement)
                        .add_flags(EmitFlags::CUSTOM_PROLOGUE);
                    self.context.add_initialization_statement(statement)?;
                }
                let updated = self.update_without_visit(parameter, NodeData::Parameter(data))?;
                visited.push(self.node(updated));
            } else if let Some(parameter) = self.visit(parameter.node())? {
                visited.push(self.node(parameter));
            }
        }
        Ok(ParameterLoweringOutcome {
            parameters: Some(
                self.context
                    .factory()?
                    .update_node_array(original, visited)?
                    .array(),
            ),
            deferred_helpers,
        })
    }

    fn visit_parameter_after_object_rest(
        &mut self,
        parameter: TransformNode,
        mut data: tsc_syntax::nodes::ParameterData,
        name: TransformNode,
        mode: ParameterLoweringMode,
    ) -> Result<TransformNode, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.dot_dot_dot_token = self.visit_optional_node(data.dot_dot_dot_token)?;
        data.question_token = self.visit_optional_node(data.question_token)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        let initializer = self.with_value_use(ExpressionValueUse::Required, |visitor| {
            data.initializer
                .map(|initializer| {
                    visitor.visit_required(Some(initializer), SyntaxKind::Parameter, "initializer")
                })
                .transpose()
        })?;
        if self.is_pattern_target(name)? {
            let binding = TargetBinding::allocate(
                self.context,
                self.generated_bindings.allocate_local_temp(),
            )?;
            let parameter_name = self.create_generated_identifier(&binding)?;
            let binding_value = self.create_generated_identifier(&binding)?;
            let binding_value = if let Some(initializer) = initializer {
                let condition_name = self.create_generated_identifier(&binding)?;
                let condition = self.create_strict_undefined_check(condition_name)?;
                self.create_conditional(condition, initializer, binding_value)?
            } else {
                binding_value
            };
            let declaration = tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: None,
            };
            let declarations = self.flatten_destructuring_binding(
                parameter,
                declaration,
                Some(binding_value),
                true,
                HelperRequestMode::AfterFunctionBody,
            )?;
            if !declarations.is_empty() {
                let statement = self.create_variable_statement(declarations)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .add_flags(EmitFlags::CUSTOM_PROLOGUE);
                self.context.add_initialization_statement(statement)?;
            }
            data.name = Some(parameter_name.node());
            data.initializer = None;
        } else {
            data.name = self.visit_optional_node(data.name)?;
            if let Some(initializer) = initializer {
                let name_text = self.identifier_text(name)?.to_owned();
                let condition_name = self.create_identifier(&name_text)?;
                let condition = self.create_strict_undefined_check(condition_name)?;
                let assignment_name = self.create_identifier(&name_text)?;
                let assignment = self.create_assignment(assignment_name, initializer)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(assignment)
                    .add_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP);
                let statement = self.create_expression_statement(assignment)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .add_flags(EmitFlags::NO_COMMENTS);
                let block = self.create_block(vec![statement], false)?;
                self.context.arena_mut()?.metadata_mut(block).add_flags(
                    EmitFlags::SINGLE_LINE
                        | EmitFlags::NO_TRAILING_SOURCE_MAP
                        | EmitFlags::NO_TOKEN_SOURCE_MAPS
                        | EmitFlags::NO_COMMENTS,
                );
                let if_statement = self.create_if_statement(condition, block)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(if_statement)
                    .add_flags(
                        EmitFlags::CUSTOM_PROLOGUE
                            | EmitFlags::NO_TOKEN_SOURCE_MAPS
                            | EmitFlags::NO_TRAILING_SOURCE_MAP
                            | EmitFlags::NO_COMMENTS,
                    );
                self.context
                    .arena_mut()?
                    .metadata_mut(if_statement)
                    .set_starts_on_new_line(true);
                self.context.add_initialization_statement(if_statement)?;
                data.initializer = match mode {
                    ParameterLoweringMode::Ordinary => None,
                    ParameterLoweringMode::AsyncGeneratorInner => Some(initializer.node()),
                };
            }
        }
        let flags = flags_after_update(
            self.context.arena(),
            parameter,
            &NodeData::Parameter(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(parameter, NodeData::Parameter(data), flags)
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

    fn create_async_generator_outer_body(
        &mut self,
        name: Option<TransformNode>,
        inner_parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
        super_capture: AsyncGeneratorSuperCapture,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .request_emit_helper(super::helpers::async_generator())?;
        let body =
            body.map(|body| self.node(body))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::FunctionDeclaration,
                    field: "async generator body",
                })?;
        let outer_body_is_multi_line = self.context.arena().node(body)?.multi_line == Some(true)
            || super_capture.owns_access && super_capture.has_element_access;
        let inner_name = name
            .and_then(|name| self.identifier_text(name).ok())
            .map(str::to_owned)
            .map(|name| self.generated_bindings.allocate_local_numbered(&name))
            .map(|name| self.create_identifier(&name))
            .transpose()?;
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
        let inner_flags = self.child_flags(&[asterisk, body])?;
        let inner = self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: inner_name.map(TransformNode::node),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                asterisk_token: Some(asterisk.node()),
                body: Some(body.node()),
                modifiers: None,
            }),
            inner_flags,
        )?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::AsyncGenerator)?;
        let this_arg = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        let arguments = self.create_identifier("arguments")?;
        let call = self.create_call(helper, vec![this_arg, arguments, inner])?;
        let return_statement = self.create_return_statement(Some(call))?;
        let mut statements = self.create_async_generator_super_statements(super_capture)?;
        statements.push(return_statement);
        self.create_block(statements, outer_body_is_multi_line)
    }

    fn create_async_generator_super_statements(
        &mut self,
        capture: AsyncGeneratorSuperCapture,
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
            statements.push(self.create_async_generator_super_index_statement(
                index_binding,
                capture.has_assignment,
            )?);
        }
        let binding = capture
            .binding
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SuperKeyword,
                field: "captured super binding",
            })?;
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
                let assignment = self.create_assignment(super_access, value)?;
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
        let list = self.create_variable_declaration_list(vec![declaration], NodeFlags::CONST)?;
        statements.push(self.create_variable_statement_from_list(list)?);
        Ok(statements)
    }

    fn create_async_generator_super_index_statement(
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
        let assignment = self.create_assignment(cache_target, entry)?;
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
        let assignment = self.create_assignment(super_access, value)?;
        let setter = self.create_arrow(vec![name_parameter, value_parameter], assignment)?;
        self.create_call(function, vec![getter, setter])
    }

    fn binary_is_object_rest_destructuring(
        &self,
        data: &tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<bool, TransformError> {
        if self.operator_kind(data.operator_token)? != Some(SyntaxKind::EqualsToken) {
            return Ok(false);
        }
        let Some(left) = data.left.map(|left| self.node(left)) else {
            return Ok(false);
        };
        Ok(matches!(
            self.context.arena().node(left)?.kind,
            SyntaxKind::ObjectLiteralExpression | SyntaxKind::ArrayLiteralExpression
        ) && self.pattern_contains_object_rest(left)?)
    }

    fn pattern_contains_object_rest(&self, pattern: TransformNode) -> Result<bool, TransformError> {
        match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    let element = self.pattern_element(element)?;
                    if element.rest || self.pattern_contains_object_rest(element.target)? {
                        return Ok(true);
                    }
                    if let Some(initializer) = element.initializer {
                        if self.expression_contains_object_rest_assignment(initializer)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            NodeData::ObjectLiteralExpression(data) => {
                for element in self.array_nodes(data.properties)? {
                    let element = self.pattern_element(element)?;
                    if element.rest || self.pattern_contains_object_rest(element.target)? {
                        return Ok(true);
                    }
                    if let Some(initializer) = element.initializer {
                        if self.expression_contains_object_rest_assignment(initializer)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            NodeData::ArrayBindingPattern(data) => {
                for element in self.array_nodes(data.elements)? {
                    if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                        continue;
                    }
                    let element = self.pattern_element(element)?;
                    let initializer_contains_rest = if let Some(initializer) = element.initializer {
                        self.expression_contains_object_rest_assignment(initializer)?
                    } else {
                        false
                    };
                    if self.pattern_contains_object_rest(element.target)?
                        || initializer_contains_rest
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            NodeData::ArrayLiteralExpression(data) => {
                for element in self.array_nodes(data.elements)? {
                    if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                        continue;
                    }
                    let element = self.pattern_element(element)?;
                    let initializer_contains_rest = if let Some(initializer) = element.initializer {
                        self.expression_contains_object_rest_assignment(initializer)?
                    } else {
                        false
                    };
                    if self.pattern_contains_object_rest(element.target)?
                        || initializer_contains_rest
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn pattern_contains_nonliteral_computed_name(
        &self,
        pattern: TransformNode,
    ) -> Result<bool, TransformError> {
        let elements = match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectBindingPattern(data) => self.array_nodes(data.elements)?,
            NodeData::ObjectLiteralExpression(data) => self.array_nodes(data.properties)?,
            NodeData::ArrayBindingPattern(data) => self.array_nodes(data.elements)?,
            NodeData::ArrayLiteralExpression(data) => self.array_nodes(data.elements)?,
            _ => return Ok(false),
        };
        for element in elements {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            let element = self.pattern_element(element)?;
            if let Some(property_name) = element.property_name {
                if let NodeData::ComputedPropertyName(data) =
                    &self.context.arena().node(property_name)?.data
                {
                    let expression = data.expression.map(|expression| self.node(expression));
                    let literal = expression.is_some_and(|expression| {
                        self.context.arena().node(expression).is_ok_and(|node| {
                            node.kind.value() >= SyntaxKind::FirstLiteralToken.value()
                                && node.kind.value() <= SyntaxKind::LastLiteralToken.value()
                        })
                    });
                    if !literal {
                        return Ok(true);
                    }
                }
            }
            if self.pattern_contains_nonliteral_computed_name(element.target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn pattern_assigns_to_identifier(
        &self,
        pattern: TransformNode,
        identifier: &str,
    ) -> Result<bool, TransformError> {
        if let NodeData::Identifier(data) = &self.context.arena().node(pattern)?.data {
            return Ok(data.text == identifier);
        }
        let elements = match &self.context.arena().node(pattern)?.data {
            NodeData::ObjectBindingPattern(data) => self.array_nodes(data.elements)?,
            NodeData::ObjectLiteralExpression(data) => self.array_nodes(data.properties)?,
            NodeData::ArrayBindingPattern(data) => self.array_nodes(data.elements)?,
            NodeData::ArrayLiteralExpression(data) => self.array_nodes(data.elements)?,
            _ => return Ok(false),
        };
        for element in elements {
            if self.context.arena().node(element)?.kind == SyntaxKind::OmittedExpression {
                continue;
            }
            if self
                .pattern_assigns_to_identifier(self.pattern_element(element)?.target, identifier)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn expression_contains_object_rest_assignment(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        match &self.context.arena().node(expression)?.data {
            NodeData::BinaryExpression(data)
                if self.operator_kind(data.operator_token)? == Some(SyntaxKind::EqualsToken) =>
            {
                let Some(left) = data.left.map(|left| self.node(left)) else {
                    return Ok(false);
                };
                self.pattern_contains_object_rest(left)
            }
            NodeData::ParenthesizedExpression(data) => {
                let Some(expression) = data.expression else {
                    return Ok(false);
                };
                self.expression_contains_object_rest_assignment(self.node(expression))
            }
            _ => Ok(false),
        }
    }

    fn operator_kind(
        &self,
        operator: Option<NodeId>,
    ) -> Result<Option<SyntaxKind>, TransformError> {
        operator
            .map(|operator| {
                self.context
                    .arena()
                    .node(self.node(operator))
                    .map(|node| node.kind)
            })
            .transpose()
    }

    fn pattern_element(&self, element: TransformNode) -> Result<PatternElement, TransformError> {
        match &self.context.arena().node(element)?.data {
            NodeData::BindingElement(data) => {
                let target = data.name.map(|name| self.node(name)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BindingElement,
                        field: "name",
                    },
                )?;
                Ok(PatternElement {
                    original: element,
                    target,
                    property_name: data
                        .property_name
                        .map(|name| self.node(name))
                        .or_else(|| data.dot_dot_dot_token.is_none().then_some(target)),
                    initializer: data.initializer.map(|initializer| self.node(initializer)),
                    rest: data.dot_dot_dot_token.is_some(),
                })
            }
            NodeData::PropertyAssignment(data) => {
                let initializer = data
                    .initializer
                    .map(|initializer| self.node(initializer))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAssignment,
                        field: "initializer",
                    })?;
                let (target, default) = self.split_assignment_default(initializer)?;
                Ok(PatternElement {
                    original: element,
                    target,
                    property_name: data.name.map(|name| self.node(name)),
                    initializer: default,
                    rest: false,
                })
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                let target = data.name.map(|name| self.node(name)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ShorthandPropertyAssignment,
                        field: "name",
                    },
                )?;
                Ok(PatternElement {
                    original: element,
                    target,
                    property_name: Some(target),
                    initializer: data
                        .object_assignment_initializer
                        .map(|initializer| self.node(initializer)),
                    rest: false,
                })
            }
            NodeData::SpreadAssignment(data) => Ok(PatternElement {
                original: element,
                target: data
                    .expression
                    .map(|expression| self.node(expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::SpreadAssignment,
                        field: "expression",
                    })?,
                property_name: None,
                initializer: None,
                rest: true,
            }),
            NodeData::SpreadElement(data) => Ok(PatternElement {
                original: element,
                target: data
                    .expression
                    .map(|expression| self.node(expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::SpreadElement,
                        field: "expression",
                    })?,
                property_name: None,
                initializer: None,
                rest: true,
            }),
            NodeData::BinaryExpression(_) => {
                let (target, initializer) = self.split_assignment_default(element)?;
                Ok(PatternElement {
                    original: element,
                    target,
                    property_name: None,
                    initializer,
                    rest: false,
                })
            }
            NodeData::OmittedExpression(_) => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::OmittedExpression,
                field: "pattern target",
            }),
            _ => Ok(PatternElement {
                original: element,
                target: element,
                property_name: None,
                initializer: None,
                rest: false,
            }),
        }
    }

    fn split_assignment_default(
        &self,
        expression: TransformNode,
    ) -> Result<(TransformNode, Option<TransformNode>), TransformError> {
        let NodeData::BinaryExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok((expression, None));
        };
        if self.operator_kind(data.operator_token)? != Some(SyntaxKind::EqualsToken) {
            return Ok((expression, None));
        }
        let target =
            data.left
                .map(|left| self.node(left))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::BinaryExpression,
                    field: "left",
                })?;
        let initializer = data.right.map(|right| self.node(right)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::BinaryExpression,
                field: "right",
            },
        )?;
        Ok((target, Some(initializer)))
    }

    fn flatten_destructuring_assignment(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<TransformNode, TransformError> {
        let pattern =
            data.left
                .map(|left| self.node(left))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::BinaryExpression,
                    field: "left",
                })?;
        let right = self.with_value_use(ExpressionValueUse::Required, |visitor| {
            visitor.visit_required(data.right, SyntaxKind::BinaryExpression, "right")
        })?;
        let mut plan = DestructuringPlan::new(DestructuringMode::Assignment);
        // The right-hand identifier cannot be reused when this pattern later
        // assigns to the same name: every property read and computed key must
        // continue to observe the pre-assignment value. Non-literal computed
        // names likewise force the source value ahead of key evaluation.
        // This is tsc's
        // `bindingOrAssignmentElementAssignsToName ||
        //  bindingOrAssignmentElementContainsNonLiteralComputedName` arm.
        let force_fresh_value = match &self.context.arena().node(right)?.data {
            NodeData::Identifier(data) => {
                self.pattern_assigns_to_identifier(pattern, &data.text)?
            }
            _ => false,
        } || self.pattern_contains_nonliteral_computed_name(pattern)?;
        let value = if force_fresh_value {
            self.ensure_destructuring_identifier(&mut plan, right, false, Some(original))?
        } else if value_use == ExpressionValueUse::Required {
            self.ensure_destructuring_identifier(&mut plan, right, true, Some(original))?
        } else {
            right
        };
        self.flatten_pattern_target(&mut plan, pattern, value, Some(original))?;
        let mut expressions = self.materialize_assignment_plan(plan)?;
        if value_use == ExpressionValueUse::Required {
            expressions.push(value);
        }
        let expression = if expressions.is_empty() {
            value
        } else {
            self.inline_expressions(expressions)?
        };
        self.set_original_and_range(expression, original)
    }

    fn flatten_destructuring_binding(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::VariableDeclarationData,
        supplied_value: Option<TransformNode>,
        skip_initializer: bool,
        helper_request_mode: HelperRequestMode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let pattern =
            data.name
                .map(|name| self.node(name))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclaration,
                    field: "name",
                })?;
        let initializer_original = data.initializer.map(|initializer| self.node(initializer));
        let force_fresh_initializer = if supplied_value.is_none() && !skip_initializer {
            let assigns_initializer = match initializer_original {
                Some(initializer) => match &self.context.arena().node(initializer)?.data {
                    NodeData::Identifier(data) => {
                        self.pattern_assigns_to_identifier(pattern, &data.text)?
                    }
                    _ => false,
                },
                None => false,
            };
            assigns_initializer || self.pattern_contains_nonliteral_computed_name(pattern)?
        } else {
            false
        };
        let value = if let Some(value) = supplied_value {
            value
        } else if skip_initializer {
            self.create_void_zero()?
        } else {
            self.with_value_use(ExpressionValueUse::Required, |visitor| {
                data.initializer
                    .map(|initializer| {
                        visitor.visit_required(
                            Some(initializer),
                            SyntaxKind::VariableDeclaration,
                            "initializer",
                        )
                    })
                    .transpose()
            })?
            .unwrap_or(self.create_void_zero()?)
        };
        let mut plan = match helper_request_mode {
            HelperRequestMode::Immediate => DestructuringPlan::new(DestructuringMode::Binding),
            HelperRequestMode::AfterFunctionBody => DestructuringPlan::parameter_binding(),
        };
        let value = if force_fresh_initializer {
            self.ensure_destructuring_identifier(&mut plan, value, false, initializer_original)?
        } else {
            value
        };
        self.flatten_pattern_target(&mut plan, pattern, value, Some(original))?;
        self.materialize_binding_plan(plan)
    }

    fn flatten_pattern_target(
        &mut self,
        plan: &mut DestructuringPlan,
        target: TransformNode,
        value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        match self.context.arena().node(target)?.data.clone() {
            NodeData::ObjectBindingPattern(data) => {
                self.flatten_object_pattern(plan, target, data.elements, value, original)
            }
            NodeData::ObjectLiteralExpression(data) => {
                self.flatten_object_pattern(plan, target, data.properties, value, original)
            }
            NodeData::ArrayBindingPattern(data) => {
                self.flatten_array_pattern(plan, target, data.elements, value, original)
            }
            NodeData::ArrayLiteralExpression(data) => {
                self.flatten_array_pattern(plan, target, data.elements, value, original)
            }
            _ => {
                let target = match plan.mode {
                    DestructuringMode::Binding => target,
                    DestructuringMode::Assignment => {
                        self.with_value_use(ExpressionValueUse::Required, |visitor| {
                            visitor.visit_required(
                                Some(target.node()),
                                SyntaxKind::BinaryExpression,
                                "assignment target",
                            )
                        })?
                    }
                };
                plan.push(target, value, original);
                Ok(())
            }
        }
    }

    fn flatten_pattern_element(
        &mut self,
        plan: &mut DestructuringPlan,
        element: PatternElement,
        mut value: TransformNode,
    ) -> Result<(), TransformError> {
        if let Some(initializer) = element.initializer {
            let initializer = self.with_value_use(ExpressionValueUse::Required, |visitor| {
                visitor.visit_required(
                    Some(initializer.node()),
                    SyntaxKind::BindingElement,
                    "initializer",
                )
            })?;
            let initializer_is_simple = self.is_simple_inlineable_expression(initializer)?;
            value = self.create_default_value_check(plan, value, initializer, element.original)?;
            if self.is_pattern_target(element.target)? && !initializer_is_simple {
                value = self.ensure_destructuring_identifier(
                    plan,
                    value,
                    true,
                    Some(element.original),
                )?;
            }
        }
        self.flatten_pattern_target(plan, element.target, value, Some(element.original))
    }

    fn flatten_object_pattern(
        &mut self,
        plan: &mut DestructuringPlan,
        pattern: TransformNode,
        elements: Option<NodeArrayId>,
        mut value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let elements = self.array_nodes(elements)?;
        if elements.len() != 1 {
            value = self.ensure_destructuring_identifier(plan, value, true, original)?;
        }

        let mut retained = Vec::new();
        let mut excluded = Vec::new();
        for (index, node) in elements.iter().copied().enumerate() {
            let element = self.pattern_element(node)?;
            if element.rest {
                if index + 1 == elements.len() {
                    self.flush_object_pattern_chunk(plan, pattern, &mut retained, value, original)?;
                    let rest = self.create_object_rest_call(
                        plan.helper_request_mode,
                        value,
                        &excluded,
                        pattern,
                    )?;
                    self.flatten_pattern_element(plan, element, rest)?;
                } else if let Some(property) =
                    self.non_last_object_rest_recovery_property(plan.mode, element)?
                {
                    excluded.push(ExcludedProperty::Named(property));
                }
                continue;
            }

            let property_key = element
                .property_name
                .map(|name| self.object_pattern_property_key(name))
                .transpose()?;
            let computed = matches!(
                property_key,
                Some(ObjectPatternPropertyKey::Computed { .. })
            );
            let target_requires_lowering = self.pattern_contains_object_rest(element.target)?;
            let initializer_requires_lowering = match element.initializer {
                Some(initializer) => {
                    self.expression_contains_object_rest_assignment(initializer)?
                }
                None => false,
            };
            if !computed && !target_requires_lowering && !initializer_requires_lowering {
                if let Some(visited) = self.visit(node.node())? {
                    retained.push(self.node(visited));
                }
                if let Some(ObjectPatternPropertyKey::Static(name)) = property_key {
                    excluded.push(ExcludedProperty::Named(name));
                }
                continue;
            }

            self.flush_object_pattern_chunk(plan, pattern, &mut retained, value, original)?;
            let property_key = property_key.ok_or(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(element.original)?.kind,
                field: "property name",
            })?;
            let (property_value, exclusion) =
                self.create_destructuring_property_access(plan, value, property_key)?;
            if let Some(exclusion) = exclusion {
                excluded.push(exclusion);
            }
            self.flatten_pattern_element(plan, element, property_value)?;
        }
        self.flush_object_pattern_chunk(plan, pattern, &mut retained, value, original)
    }

    fn flush_object_pattern_chunk(
        &mut self,
        plan: &mut DestructuringPlan,
        original_pattern: TransformNode,
        retained: &mut Vec<TransformNode>,
        value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        if retained.is_empty() {
            return Ok(());
        }
        let pattern = self.create_object_pattern(plan.mode, std::mem::take(retained))?;
        self.set_original_and_range(pattern, original_pattern)?;
        plan.push(pattern, value, original);
        Ok(())
    }

    fn flatten_array_pattern(
        &mut self,
        plan: &mut DestructuringPlan,
        pattern: TransformNode,
        elements: Option<NodeArrayId>,
        value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let elements = self.array_nodes(elements)?;
        // flattenArrayBindingOrAssignmentPattern's zero-element arm calls
        // ensureIdentifier but never creates an empty bindingElements list.
        // In assignment mode this deliberately leaves only the temporary
        // assignment (`_a = value`); emitting `[] = value` is a different
        // runtime operation and diverges for an object-rest target `...[]`.
        if elements.is_empty() {
            let _ = self.ensure_destructuring_identifier(plan, value, false, original)?;
            return Ok(());
        }
        let mut retained = Vec::with_capacity(elements.len());
        let mut deferred = Vec::new();
        for node in elements {
            if self.context.arena().node(node)?.kind == SyntaxKind::OmittedExpression {
                retained.push(node);
                continue;
            }
            let element = self.pattern_element(node)?;
            let initializer_contains_object_rest = match element.initializer {
                Some(initializer) => {
                    self.expression_contains_object_rest_assignment(initializer)?
                }
                None => false,
            };
            let contains_object_rest = self.pattern_contains_object_rest(element.target)?
                || initializer_contains_object_rest;
            let defer_after_prior = plan.has_transformed_prior_array_element
                && !self.is_simple_pattern_element(element)?;
            if contains_object_rest || defer_after_prior {
                plan.has_transformed_prior_array_element = true;
                let binding = self.allocate_destructuring_temp(plan.mode)?;
                let pattern_name = self.create_generated_identifier(&binding)?;
                let read = self.create_generated_identifier(&binding)?;
                retained.push(match plan.mode {
                    DestructuringMode::Binding => self.create_binding_element(pattern_name)?,
                    DestructuringMode::Assignment => pattern_name,
                });
                deferred.push((element, read));
            } else if let Some(visited) = self.visit(node.node())? {
                retained.push(self.node(visited));
            }
        }
        let retained_pattern = self.create_array_pattern(plan.mode, retained)?;
        self.set_original_and_range(retained_pattern, pattern)?;
        plan.push(retained_pattern, value, original);
        for (element, read) in deferred {
            self.flatten_pattern_element(plan, element, read)?;
        }
        Ok(())
    }

    /// tsc `isSimpleBindingOrAssignmentElement` @6.0.3. Identifiers are
    /// deliberately *not* simple inlineable initializers: retaining
    /// `b = a` after a prior object-rest element would evaluate `a` before
    /// that earlier element has been materialized.
    fn is_simple_pattern_element(&self, element: PatternElement) -> Result<bool, TransformError> {
        if self.context.arena().node(element.target)?.kind == SyntaxKind::OmittedExpression {
            return Ok(true);
        }
        if let Some(property_name) = element.property_name {
            if matches!(
                self.object_pattern_property_key(property_name)?,
                ObjectPatternPropertyKey::Computed { .. }
            ) {
                return Ok(false);
            }
        }
        if let Some(initializer) = element.initializer {
            if !self.is_simple_inlineable_expression(initializer)? {
                return Ok(false);
            }
        }
        if self.is_pattern_target(element.target)? {
            for child in match &self.context.arena().node(element.target)?.data {
                NodeData::ObjectBindingPattern(data) => self.array_nodes(data.elements)?,
                NodeData::ObjectLiteralExpression(data) => self.array_nodes(data.properties)?,
                NodeData::ArrayBindingPattern(data) => self.array_nodes(data.elements)?,
                NodeData::ArrayLiteralExpression(data) => self.array_nodes(data.elements)?,
                _ => Vec::new(),
            } {
                if self.context.arena().node(child)?.kind == SyntaxKind::OmittedExpression {
                    continue;
                }
                if !self.is_simple_pattern_element(self.pattern_element(child)?)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(self.context.arena().node(element.target)?.kind == SyntaxKind::Identifier)
    }

    fn create_default_value_check(
        &mut self,
        plan: &mut DestructuringPlan,
        value: TransformNode,
        initializer: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let value = self.ensure_destructuring_identifier(plan, value, true, Some(original))?;
        let undefined = self.create_void_zero()?;
        let condition =
            self.create_binary(value, SyntaxKind::EqualsEqualsEqualsToken, undefined)?;
        self.create_conditional(condition, initializer, value)
    }

    fn ensure_destructuring_identifier(
        &mut self,
        plan: &mut DestructuringPlan,
        value: TransformNode,
        reuse_identifier: bool,
        original: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if reuse_identifier && self.context.arena().node(value)?.kind == SyntaxKind::Identifier {
            return Ok(value);
        }
        let binding = self.allocate_destructuring_temp(plan.mode)?;
        let target = self.create_generated_identifier(&binding)?;
        let read = self.create_generated_identifier(&binding)?;
        plan.push(target, value, original);
        Ok(read)
    }

    fn allocate_destructuring_temp(
        &mut self,
        mode: DestructuringMode,
    ) -> Result<TargetBinding, TransformError> {
        match mode {
            DestructuringMode::Binding => {
                TargetBinding::allocate(self.context, self.generated_bindings.allocate_local_temp())
            }
            DestructuringMode::Assignment => {
                let binding =
                    TargetBinding::allocate(self.context, self.generated_bindings.allocate_temp())?;
                let declaration = self.create_generated_identifier(&binding)?;
                self.context.hoist_variable_declaration(declaration)?;
                Ok(binding)
            }
        }
    }

    fn materialize_binding_plan(
        &mut self,
        plan: DestructuringPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        debug_assert_eq!(plan.mode, DestructuringMode::Binding);
        plan.steps
            .into_iter()
            .map(|step| {
                let declaration =
                    self.create_variable_declaration(step.target, Some(step.value))?;
                if let Some(original) = step.original {
                    self.context
                        .factory()?
                        .set_text_range(declaration, original)?;
                    self.context
                        .arena_mut()?
                        .set_original_node(declaration, Some(original))?;
                }
                Ok(declaration)
            })
            .collect()
    }

    fn materialize_assignment_plan(
        &mut self,
        plan: DestructuringPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        debug_assert_eq!(plan.mode, DestructuringMode::Assignment);
        plan.steps
            .into_iter()
            .map(|step| {
                let assignment = self.create_assignment(step.target, step.value)?;
                if let Some(original) = step.original {
                    self.context
                        .factory()?
                        .set_text_range(assignment, original)?;
                    self.context
                        .arena_mut()?
                        .set_original_node(assignment, Some(original))?;
                }
                Ok(assignment)
            })
            .collect()
    }

    fn visit_object_literal_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ObjectLiteralExpressionData,
    ) -> Result<TransformNode, TransformError> {
        let properties = self.array_nodes(data.properties)?;
        let mut operands = Vec::new();
        let mut chunk = Vec::new();
        for property in properties {
            match self.context.arena().node(property)?.data.clone() {
                NodeData::SpreadAssignment(spread) => {
                    if !chunk.is_empty() {
                        operands.push(self.create_object_literal(std::mem::take(&mut chunk))?);
                    }
                    operands.push(self.visit_required(
                        spread.expression,
                        SyntaxKind::SpreadAssignment,
                        "expression",
                    )?);
                }
                NodeData::PropertyAssignment(mut assignment) => {
                    assignment.name = self.visit_optional_node(assignment.name)?;
                    assignment.initializer = self.visit_optional_node(assignment.initializer)?;
                    let flags = flags_after_update(
                        self.context.arena(),
                        property,
                        &NodeData::PropertyAssignment(assignment.clone()),
                    )?;
                    chunk.push(self.context.factory()?.update_node(
                        property,
                        NodeData::PropertyAssignment(assignment),
                        flags,
                    )?);
                }
                _ => {
                    if let Some(visited) = self.visit(property.node())? {
                        chunk.push(self.node(visited));
                    }
                }
            }
        }
        if !chunk.is_empty() {
            operands.push(self.create_object_literal(chunk)?);
        }
        if operands.first().is_some_and(|operand| {
            self.context
                .arena()
                .node(*operand)
                .is_ok_and(|node| node.kind != SyntaxKind::ObjectLiteralExpression)
        }) {
            operands.insert(0, self.create_object_literal(Vec::new())?);
        }
        let mut operands = operands.into_iter();
        let Some(mut expression) = operands.next() else {
            return self.create_object_literal(Vec::new());
        };
        let Some(first_source) = operands.next() else {
            // createAssignHelper retains the call even when a spread happens
            // to contain an object literal and therefore produces only one
            // chunk. Besides matching emit shape, this preserves the
            // observable Object.assign lookup/call required by object spread.
            expression = self.create_object_assign_call(vec![expression])?;
            return self.set_original_and_range(expression, original);
        };
        expression = self.create_object_assign(expression, first_source)?;
        for operand in operands {
            expression = self.create_object_assign(expression, operand)?;
        }
        self.set_original_and_range(expression, original)
    }

    fn create_object_assign(
        &mut self,
        target: TransformNode,
        source: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_object_assign_call(vec![target, source])
    }

    fn create_object_assign_call(
        &mut self,
        operands: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        debug_assert!(self.target >= ScriptTarget::ES2015);
        let object = self.create_identifier("Object")?;
        let assign = self.create_identifier("assign")?;
        let callee = self.create_property_access(object, assign)?;
        self.create_call(callee, operands)
    }

    fn object_pattern_property_key(
        &self,
        property_name: TransformNode,
    ) -> Result<ObjectPatternPropertyKey, TransformError> {
        let NodeData::ComputedPropertyName(data) =
            self.context.arena().node(property_name)?.data.clone()
        else {
            return Ok(ObjectPatternPropertyKey::Static(property_name));
        };
        let expression = data
            .expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        if matches!(
            self.context.arena().node(expression)?.kind,
            SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral
        ) {
            Ok(ObjectPatternPropertyKey::Static(expression))
        } else {
            Ok(ObjectPatternPropertyKey::Computed {
                wrapper: property_name,
                expression,
            })
        }
    }

    /// In a declaration recovery tree, `{ ...a, x, ...b }` represents the
    /// first rest as a BindingElement whose target is also its recoverable
    /// property name. `createRestHelper` scans every element before the final
    /// rest, so `a` remains excluded even though that invalid non-final rest
    /// emits no binding. Assignment spreads do not expose such a property
    /// name and therefore deliberately return `None`.
    fn non_last_object_rest_recovery_property(
        &self,
        mode: DestructuringMode,
        element: PatternElement,
    ) -> Result<Option<TransformNode>, TransformError> {
        if mode != DestructuringMode::Binding
            || self.context.arena().node(element.original)?.kind != SyntaxKind::BindingElement
        {
            return Ok(None);
        }
        Ok(matches!(
            self.context.arena().node(element.target)?.kind,
            SyntaxKind::Identifier
                | SyntaxKind::PrivateIdentifier
                | SyntaxKind::StringLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::ComputedPropertyName
        )
        .then_some(element.target))
    }

    fn clone_property_name_literal(
        &mut self,
        property_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let clone = self.context.factory()?.clone_node(property_name)?;
        if self.context.arena().node(property_name)?.kind == SyntaxKind::StringLiteral {
            self.context
                .arena_mut()?
                .metadata_mut(clone)
                .set_string_literal_text_source(property_name);
        }
        Ok(clone)
    }

    fn create_destructuring_property_access(
        &mut self,
        plan: &mut DestructuringPlan,
        value: TransformNode,
        property_key: ObjectPatternPropertyKey,
    ) -> Result<(TransformNode, Option<ExcludedProperty>), TransformError> {
        if let ObjectPatternPropertyKey::Computed {
            wrapper,
            expression,
        } = property_key
        {
            let expression = self.with_value_use(ExpressionValueUse::Required, |visitor| {
                visitor.visit_required(
                    Some(expression.node()),
                    SyntaxKind::ComputedPropertyName,
                    "expression",
                )
            })?;
            let argument =
                self.ensure_destructuring_identifier(plan, expression, false, Some(wrapper))?;
            let access = self.create_element_access(value, argument)?;
            return Ok((access, Some(ExcludedProperty::Computed(argument))));
        }
        let ObjectPatternPropertyKey::Static(property_name) = property_key else {
            unreachable!("computed property key returned above")
        };
        let kind = self.context.arena().node(property_name)?.kind;
        if matches!(
            kind,
            SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
        ) {
            let argument = self.clone_property_name_literal(property_name)?;
            return Ok((
                self.create_element_access(value, argument)?,
                Some(ExcludedProperty::Named(property_name)),
            ));
        }
        let name_text = self.property_name_text(property_name)?.to_owned();
        let name = self.create_identifier(&name_text)?;
        Ok((
            self.create_property_access(value, name)?,
            Some(ExcludedProperty::Named(property_name)),
        ))
    }

    fn create_object_rest_call(
        &mut self,
        helper_request_mode: HelperRequestMode,
        value: TransformNode,
        excluded: &[ExcludedProperty],
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if helper_request_mode == HelperRequestMode::Immediate {
            self.context
                .request_emit_helper(super::helpers::object_rest())?;
        }
        let mut properties = Vec::with_capacity(excluded.len());
        for property in excluded {
            properties.push(match *property {
                ExcludedProperty::Named(name) => {
                    self.create_string_literal_from_property_name(name)?
                }
                ExcludedProperty::Computed(temp) => {
                    let kind = self.create_string_literal("symbol")?;
                    let type_of = self.create_typeof(temp)?;
                    let condition =
                        self.create_binary(type_of, SyntaxKind::EqualsEqualsEqualsToken, kind)?;
                    let empty = self.create_string_literal("")?;
                    let as_string = self.create_binary(temp, SyntaxKind::PlusToken, empty)?;
                    self.create_conditional(condition, temp, as_string)?
                }
            });
        }
        let excluded = self.create_array_literal(properties)?;
        self.context.factory()?.set_text_range(excluded, original)?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Rest)?;
        self.create_call(helper, vec![value, excluded])
    }

    fn create_string_literal_from_property_name(
        &mut self,
        property_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let text = self.property_name_text(property_name)?.to_owned();
        let literal = self.create_string_literal(&text)?;
        if self.context.arena().node(property_name)?.kind == SyntaxKind::StringLiteral {
            self.context
                .arena_mut()?
                .metadata_mut(literal)
                .set_string_literal_text_source(property_name);
        }
        Ok(literal)
    }

    fn create_object_pattern(
        &mut self,
        mode: DestructuringMode,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        let flags = self.context.arena().array_transform_flags(elements);
        match mode {
            DestructuringMode::Binding => self.context.factory()?.create_node(
                self.source,
                NodeData::ObjectBindingPattern(tsc_syntax::nodes::ObjectBindingPatternData {
                    elements: Some(elements.array()),
                }),
                flags,
            ),
            DestructuringMode::Assignment => self.context.factory()?.create_node(
                self.source,
                NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                    properties: Some(elements.array()),
                }),
                flags,
            ),
        }
    }

    fn create_array_pattern(
        &mut self,
        mode: DestructuringMode,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        let flags = self.context.arena().array_transform_flags(elements);
        match mode {
            DestructuringMode::Binding => self.context.factory()?.create_node(
                self.source,
                NodeData::ArrayBindingPattern(tsc_syntax::nodes::ArrayBindingPatternData {
                    elements: Some(elements.array()),
                }),
                flags,
            ),
            DestructuringMode::Assignment => self.context.factory()?.create_node(
                self.source,
                NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                    elements: Some(elements.array()),
                }),
                flags,
            ),
        }
    }

    fn create_binding_element(
        &mut self,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BindingElement(tsc_syntax::nodes::BindingElementData {
                name: Some(name.node()),
                property_name: None,
                dot_dot_dot_token: None,
                initializer: None,
            }),
            flags,
        )
    }

    fn property_name_text(&self, name: TransformNode) -> Result<&str, TransformError> {
        match &self.context.arena().node(name)?.data {
            NodeData::Identifier(data) => Ok(&data.text),
            NodeData::StringLiteral(data) => Ok(&data.text),
            NodeData::NumericLiteral(data) => Ok(&data.text),
            NodeData::BigIntLiteral(data) => Ok(&data.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(name)?.kind,
                field: "literal property name",
            }),
        }
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
        binding.write_generated_metadata(self.context.arena_mut()?, identifier);
        Ok(identifier)
    }

    fn create_string_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text: text.to_owned(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )
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

    fn create_boolean(&mut self, value: bool) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            if value {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            },
            TransformFlags::NONE,
        )
    }

    fn create_logical_not(
        &mut self,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(operand)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                operator: SyntaxKind::ExclamationToken,
                operand: Some(operand.node()),
            }),
            flags,
        )
    }

    fn create_strict_undefined_check(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let undefined = self.create_void_zero()?;
        self.create_binary(expression, SyntaxKind::EqualsEqualsEqualsToken, undefined)
    }

    fn create_downlevel_await(
        &mut self,
        mode: FunctionMode,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match mode {
            FunctionMode::Async => {
                let flags = self.context.arena().propagate_child_flags(expression)?
                    | TransformFlags::CONTAINS_ES_2018;
                self.context.factory()?.create_node(
                    self.source,
                    NodeData::AwaitExpression(tsc_syntax::nodes::AwaitExpressionData {
                        expression: Some(expression.node()),
                    }),
                    flags,
                )
            }
            FunctionMode::AsyncGenerator => {
                self.context
                    .request_emit_helper(super::helpers::async_await())?;
                let helper = self
                    .context
                    .factory()?
                    .create_unscoped_helper_identifier(self.source, EmitHelperName::Await)?;
                let awaited = self.create_call(helper, vec![expression])?;
                let flags = self.context.arena().propagate_child_flags(awaited)?;
                self.context.factory()?.create_node(
                    self.source,
                    NodeData::YieldExpression(tsc_syntax::nodes::YieldExpressionData {
                        asterisk_token: None,
                        expression: Some(awaited.node()),
                    }),
                    flags,
                )
            }
            FunctionMode::Ordinary | FunctionMode::Generator => {
                Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ForOfStatement,
                    field: "async await mode",
                })
            }
        }
    }

    fn create_yield_expression(
        &mut self,
        asterisk_token: Option<TransformNode>,
        expression: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut children = Vec::new();
        children.extend(asterisk_token);
        children.extend(expression);
        let flags = self.child_flags(&children)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::YieldExpression(tsc_syntax::nodes::YieldExpressionData {
                asterisk_token: asterisk_token.map(TransformNode::node),
                expression: expression.map(TransformNode::node),
            }),
            flags,
        )
    }

    fn create_for_statement(
        &mut self,
        initializer: Option<TransformNode>,
        condition: Option<TransformNode>,
        incrementor: Option<TransformNode>,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut children = Vec::new();
        children.extend(initializer);
        children.extend(condition);
        children.extend(incrementor);
        children.push(statement);
        let flags = self.child_flags(&children)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ForStatement(tsc_syntax::nodes::ForStatementData {
                initializer: initializer.map(TransformNode::node),
                condition: condition.map(TransformNode::node),
                incrementor: incrementor.map(TransformNode::node),
                statement: Some(statement.node()),
            }),
            flags,
        )
    }

    fn create_labeled_statement(
        &mut self,
        label: TransformNode,
        statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[label, statement])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::LabeledStatement(tsc_syntax::nodes::LabeledStatementData {
                label: Some(label.node()),
                statement: Some(statement.node()),
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

    fn create_parameter(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
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
            TransformFlags::NONE,
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
        let flags = self.context.arena().propagate_child_flags(body)?;
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
        let flags = self.context.arena().array_transform_flags(parameters)
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

    fn create_catch_clause(
        &mut self,
        variable_declaration: TransformNode,
        block: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[variable_declaration, block])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::CatchClause(tsc_syntax::nodes::CatchClauseData {
                variable_declaration: Some(variable_declaration.node()),
                block: Some(block.node()),
            }),
            flags,
        )
    }

    fn create_if_statement(
        &mut self,
        expression: TransformNode,
        then_statement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[expression, then_statement])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                expression: Some(expression.node()),
                then_statement: Some(then_statement.node()),
                else_statement: None,
            }),
            flags,
        )
    }

    fn create_parenthesized(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[expression])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_throw_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(expression)?
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ThrowStatement(tsc_syntax::nodes::ThrowStatementData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_try_statement(
        &mut self,
        try_block: TransformNode,
        catch_clause: Option<TransformNode>,
        finally_block: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut children = vec![try_block];
        children.extend(catch_clause);
        children.extend(finally_block);
        let flags = self.child_flags(&children)?
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        self.context.factory()?.create_node(
            self.source,
            NodeData::TryStatement(tsc_syntax::nodes::TryStatementData {
                try_block: Some(try_block.node()),
                catch_clause: catch_clause.map(TransformNode::node),
                finally_block: finally_block.map(TransformNode::node),
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

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsToken, right)
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

    fn create_conditional(
        &mut self,
        condition: TransformNode,
        when_true: TransformNode,
        when_false: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let when_true = self.parenthesize_comma_expression(when_true)?;
        let when_false = self.parenthesize_comma_expression(when_false)?;
        let question = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::QuestionToken,
            TransformFlags::NONE,
        )?;
        let colon = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ColonToken,
            TransformFlags::NONE,
        )?;
        let flags = self.child_flags(&[condition, when_true, when_false])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            flags,
        )
    }

    fn parenthesize_comma_expression(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let is_comma = match &self.context.arena().node(expression)?.data {
            NodeData::BinaryExpression(data) => {
                self.operator_kind(data.operator_token)? == Some(SyntaxKind::CommaToken)
            }
            NodeData::CommaListExpression(_) => true,
            _ => false,
        };
        if !is_comma {
            return Ok(expression);
        }
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn is_pattern_target(&self, target: TransformNode) -> Result<bool, TransformError> {
        Ok(matches!(
            self.context.arena().node(target)?.kind,
            SyntaxKind::ObjectBindingPattern
                | SyntaxKind::ArrayBindingPattern
                | SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::ArrayLiteralExpression
        ))
    }

    fn is_simple_inlineable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.context.arena().node(expression)?.kind;
        Ok(matches!(
            kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::TrueKeyword
                | SyntaxKind::FalseKeyword
                | SyntaxKind::NullKeyword
                | SyntaxKind::ThisKeyword
        ))
    }

    fn create_typeof(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.context.arena().propagate_child_flags(expression)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
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

    fn inline_expressions(
        &mut self,
        mut expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let first = expressions
            .first()
            .copied()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CommaListExpression,
                field: "expression",
            })?;
        expressions.remove(0);
        expressions.into_iter().try_fold(first, |left, right| {
            self.create_binary(left, SyntaxKind::CommaToken, right)
        })
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

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        try_visit_each_child(&mut data, self)?;
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

    fn with_value_use<T>(
        &mut self,
        value_use: ExpressionValueUse,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<T, TransformError> {
        let previous = std::mem::replace(&mut self.value_use, value_use);
        let result = operation(self);
        self.value_use = previous;
        result
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

    fn merge_source_lexical_environment(
        &mut self,
        root: TransformNode,
        lexical_environment: LexicalEnvironment,
    ) -> Result<TransformNode, TransformError> {
        if lexical_environment.is_empty() {
            return Ok(root);
        }
        let NodeData::SourceFile(mut data) = self.context.arena().node(root)?.data.clone() else {
            return Err(TransformError::RootKindExpected {
                actual: self.context.arena().node(root)?.kind,
            });
        };
        data.statements = self.merge_statement_array(data.statements, lexical_environment)?;
        let flags = flags_after_update(
            self.context.arena(),
            root,
            &NodeData::SourceFile(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn merge_function_lexical_environment(
        &mut self,
        function_kind: SyntaxKind,
        body: Option<NodeId>,
        lexical_environment: LexicalEnvironment,
    ) -> Result<Option<NodeId>, TransformError> {
        if lexical_environment.is_empty() {
            return Ok(body);
        }
        let body =
            body.map(|body| self.node(body))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: function_kind,
                    field: "body for lexical declarations",
                })?;
        let body = if self.context.arena().node(body)?.kind == SyntaxKind::Block {
            body
        } else if function_kind == SyntaxKind::ArrowFunction {
            let return_statement = self.create_return_statement(Some(body))?;
            self.context
                .factory()?
                .set_text_range(return_statement, body)?;
            let block = self.create_block(vec![return_statement], true)?;
            self.context.factory()?.set_text_range(block, body)?;
            block
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: function_kind,
                field: "block function body for lexical declarations",
            });
        };
        let NodeData::Block(mut data) = self.context.arena().node(body)?.data.clone() else {
            unreachable!("function body was normalized to a block")
        };
        data.statements = self.merge_statement_array(data.statements, lexical_environment)?;
        let flags = flags_after_update(self.context.arena(), body, &NodeData::Block(data.clone()))?;
        Ok(Some(
            self.context
                .factory()?
                .update_node(body, NodeData::Block(data), flags)?
                .node(),
        ))
    }

    fn merge_statement_array(
        &mut self,
        statements: Option<NodeArrayId>,
        lexical_environment: LexicalEnvironment,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let original = statements.map(|statements| self.array(statements));
        let mut statements = self.array_nodes(statements)?;
        let mut prologue_end = 0;
        for statement in &statements {
            if !self.is_prologue_statement(*statement)? {
                break;
            }
            prologue_end += 1;
        }
        let function_end = prologue_end
            + statements[prologue_end..]
                .iter()
                .take_while(|statement| self.is_hoisted_function(**statement))
                .count();
        let variable_end = function_end
            + statements[function_end..]
                .iter()
                .take_while(|statement| self.is_hoisted_variable_statement(**statement))
                .count();

        if !lexical_environment.initialization_statements().is_empty() {
            statements.splice(
                variable_end..variable_end,
                lexical_environment
                    .initialization_statements()
                    .iter()
                    .copied(),
            );
        }
        if !lexical_environment.variable_declarations().is_empty() {
            let declarations = lexical_environment
                .variable_declarations()
                .iter()
                .copied()
                .map(|name| self.create_variable_declaration(name, None))
                .collect::<Result<Vec<_>, _>>()?;
            let statement = self.create_variable_statement(declarations)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.insert(function_end, statement);
        }
        if !lexical_environment.function_declarations().is_empty() {
            statements.splice(
                prologue_end..prologue_end,
                lexical_environment.function_declarations().iter().copied(),
            );
        }

        let updated = if let Some(original) = original {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        Ok(Some(updated.array()))
    }

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let list_flags = self.context.arena().array_transform_flags(declarations)
            | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
        let list = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            list_flags,
        )?;
        self.context
            .factory()?
            .set_node_flags(list, NodeFlags::NONE)?;
        let statement_flags = self.context.arena().propagate_child_flags(list)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            statement_flags,
        )
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

    fn is_prologue_statement(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        Ok(data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|expression| matches!(expression.data, NodeData::StringLiteral(_)))
        }))
    }

    fn is_hoisted_function(&self, statement: TransformNode) -> bool {
        self.is_custom_prologue(statement)
            && self
                .context
                .arena()
                .node(statement)
                .is_ok_and(|node| node.kind == SyntaxKind::FunctionDeclaration)
    }

    fn is_hoisted_variable_statement(&self, statement: TransformNode) -> bool {
        self.is_custom_prologue(statement)
            && self
                .context
                .arena()
                .node(statement)
                .is_ok_and(|node| node.kind == SyntaxKind::VariableStatement)
    }

    fn identifier_text(&self, node: TransformNode) -> Result<&str, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Ok(&data.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionDeclaration,
                field: "function identifier",
            }),
        }
    }

    fn is_custom_prologue(&self, statement: TransformNode) -> bool {
        self.context
            .arena()
            .metadata(statement)
            .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::CUSTOM_PROLOGUE))
    }

    fn assert_binding_plan(
        &self,
        generated_bindings: &GeneratedBindings,
        lexical_environment: &LexicalEnvironment,
    ) {
        debug_assert_eq!(
            generated_bindings.names().len(),
            lexical_environment.variable_declarations().len(),
            "each generated binding is materialized by its lexical owner",
        );
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

impl NodeDataChildVisitor for Es2018Visitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("ES2018 child belongs to the current transform source")
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

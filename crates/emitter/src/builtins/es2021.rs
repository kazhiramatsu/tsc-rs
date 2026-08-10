//! H2.5b ES2021 logical-assignment lowering.
//!
//! The pinned TypeScript transformer defines evaluation order and observable
//! output. Rust owns that behavior through explicit access-stabilization and
//! lexical-scope plans rather than mirroring TypeScript's nested closures.

use std::collections::BTreeMap;

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::{
    EmitFlags, LexicalEnvironment, LexicalEnvironmentFlags, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformRoot, TransformSourceId, TransformationContext,
    Transformer,
};

use super::{
    flags_after_update,
    generated_bindings::{
        AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes, GeneratedBindings,
    },
    initialize_transform_flags,
    system::collect_identifier_texts,
};

/// tsc-port: transformES2021 @6.0.3
/// tsc-hash: 9f18d49525c22011f2b39fd966d1d6bb59ebe1fb9b2099d72314a94fbddf8e1c
/// tsc-span: _tsc.js:103205-103275
pub(super) fn transform_es2021(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(Es2021Transformer {
        target: options.emit_script_target(),
    })
}

struct Es2021Transformer {
    target: ScriptTarget,
}

impl Transformer for Es2021Transformer {
    fn name(&self) -> &'static str {
        "transformES2021"
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES2020 || self.target >= ScriptTarget::ES2021 {
            return Err(TransformError::UnsupportedCompilerOption {
                option: "ES2021 transform",
                detail: "H2.5b admits transformES2021 for the ES2020 target boundary",
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

        // Earlier transforms may synthesize a new ownership tree around an
        // original logical assignment. Reclassify that completed tree at the
        // target-pass boundary so subtree gating is an invariant of the
        // current arena, not an incidental promise made by every producer.
        initialize_transform_flags(context.arena_mut()?, source)?;
        context.start_lexical_environment()?;
        let current_root = context.arena().root(source)?;
        let mut visitor = Es2021Visitor::new(context, source);
        let visited = visitor.visit(current_root.node());
        let lexical_environment = visitor.context.end_lexical_environment();
        let generated_bindings = visitor.generated_bindings.source_bindings();

        let transformed = visited?.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::SourceFile,
            field: "root",
        })?;
        let lexical_environment = lexical_environment?;
        visitor.assert_binding_plan(&generated_bindings, &lexical_environment);
        let transformed = visitor
            .merge_source_lexical_environment(visitor.node(transformed), lexical_environment)?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[derive(Clone, Debug)]
struct ParameterHoistPlan {
    binding_aliases: Vec<Option<String>>,
}

struct Es2021Visitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    generated_bindings: GeneratedBindingScopes,
}

impl<'context> Es2021Visitor<'context> {
    fn new(context: &'context mut TransformationContext, source: TransformSourceId) -> Self {
        Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_identifier_texts(context.arena(), source),
                AncestorBindingPolicy::AllowShadow,
            ),
            context,
            source,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        if !self
            .context
            .arena()
            .transform_flags(original)
            .contains(TransformFlags::CONTAINS_ES_2021)
        {
            self.nodes.insert(id, Some(id));
            return Ok(Some(id));
        }

        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::BinaryExpression(data) => Some(self.visit_binary_expression(original, data)?),
            NodeData::FunctionDeclaration(data) => {
                Some(self.visit_function_declaration(original, data)?)
            }
            NodeData::FunctionExpression(data) => {
                Some(self.visit_function_expression(original, data)?)
            }
            NodeData::ArrowFunction(data) => Some(self.visit_arrow_function(original, data)?),
            NodeData::MethodDeclaration(data) => {
                Some(self.visit_method_declaration(original, data)?)
            }
            NodeData::GetAccessor(data) => Some(self.visit_get_accessor(original, data)?),
            NodeData::SetAccessor(data) => Some(self.visit_set_accessor(original, data)?),
            NodeData::Constructor(data) => Some(self.visit_constructor(original, data)?),
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_binary_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<NodeId, TransformError> {
        let operator = data
            .operator_token
            .map(|operator| {
                self.context
                    .arena()
                    .node(self.node(operator))
                    .map(|node| node.kind)
            })
            .transpose()?;
        let Some(operator) = operator.filter(|operator| Self::is_logical_assignment(*operator))
        else {
            return self.update_generic(original, NodeData::BinaryExpression(data));
        };

        let left = self.visit_required(data.left, SyntaxKind::BinaryExpression, "left")?;
        let mut left = self.skip_parentheses(left)?;
        let right = self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?;
        let right = self.skip_parentheses(right)?;
        let mut assignment_target = left;

        match self.context.arena().node(left)?.data.clone() {
            NodeData::PropertyAccessExpression(access) => {
                let receiver = access
                    .expression
                    .map(|receiver| self.node(receiver))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "expression",
                    })?;
                let name = access.name.map(|name| self.node(name)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "name",
                    },
                )?;
                let stabilized = self.stabilize_access_operand(receiver)?;
                assignment_target = self.create_property_access(stabilized.read(), name)?;
                let initialized_receiver = match stabilized {
                    StabilizedAccessOperand::Copied(receiver) => receiver,
                    StabilizedAccessOperand::Hoisted { initialization, .. } => {
                        self.create_parenthesized(initialization)?
                    }
                };
                left = self.create_property_access(initialized_receiver, name)?;
            }
            NodeData::ElementAccessExpression(access) => {
                let receiver = access
                    .expression
                    .map(|receiver| self.node(receiver))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ElementAccessExpression,
                        field: "expression",
                    })?;
                let argument = access
                    .argument_expression
                    .map(|argument| self.node(argument))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ElementAccessExpression,
                        field: "argument_expression",
                    })?;
                let receiver = self.stabilize_access_operand(receiver)?;
                let argument = self.stabilize_access_operand(argument)?;
                assignment_target = self.create_element_access(receiver.read(), argument.read())?;
                let initialized_receiver = match receiver {
                    StabilizedAccessOperand::Copied(receiver) => receiver,
                    StabilizedAccessOperand::Hoisted { initialization, .. } => {
                        self.create_parenthesized(initialization)?
                    }
                };
                left =
                    self.create_element_access(initialized_receiver, argument.initialization())?;
            }
            _ => {}
        }

        let assignment = self.create_assignment(assignment_target, right)?;
        let assignment = self.create_parenthesized(assignment)?;
        let result =
            self.create_binary(left, Self::non_assignment_operator(operator), assignment)?;
        Ok(self.set_original_and_range(result, original)?.node())
    }

    fn stabilize_access_operand(
        &mut self,
        operand: TransformNode,
    ) -> Result<StabilizedAccessOperand, TransformError> {
        if self.is_simple_copiable_expression(operand)? {
            return Ok(StabilizedAccessOperand::Copied(operand));
        }
        let name = self.allocate_hoisted_temp()?;
        let read = self.create_identifier(&name)?;
        let initialized = self.create_identifier(&name)?;
        let initialization = self.create_assignment(initialized, operand)?;
        Ok(StabilizedAccessOperand::Hoisted {
            read,
            initialization,
        })
    }

    fn visit_function_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::FunctionDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.asterisk_token = self.visit_optional_node(data.asterisk_token)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_function_scope(
            SyntaxKind::FunctionDeclaration,
            data.parameters,
            data.body,
            false,
        )?;
        self.update_without_visit(original, NodeData::FunctionDeclaration(data))
    }

    fn visit_function_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::FunctionExpressionData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.asterisk_token = self.visit_optional_node(data.asterisk_token)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_function_scope(
            SyntaxKind::FunctionExpression,
            data.parameters,
            data.body,
            false,
        )?;
        self.update_without_visit(original, NodeData::FunctionExpression(data))
    }

    fn visit_arrow_function(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ArrowFunctionData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        data.equals_greater_than_token =
            self.visit_optional_node(data.equals_greater_than_token)?;
        (data.parameters, data.body) =
            self.visit_function_scope(SyntaxKind::ArrowFunction, data.parameters, data.body, true)?;
        self.update_without_visit(original, NodeData::ArrowFunction(data))
    }

    fn visit_method_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::MethodDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.asterisk_token = self.visit_optional_node(data.asterisk_token)?;
        data.name = self.visit_optional_node(data.name)?;
        data.question_token = self.visit_optional_node(data.question_token)?;
        data.exclamation_token = self.visit_optional_node(data.exclamation_token)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) = self.visit_function_scope(
            SyntaxKind::MethodDeclaration,
            data.parameters,
            data.body,
            false,
        )?;
        self.update_without_visit(original, NodeData::MethodDeclaration(data))
    }

    fn visit_get_accessor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::GetAccessorData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) =
            self.visit_function_scope(SyntaxKind::GetAccessor, data.parameters, data.body, false)?;
        self.update_without_visit(original, NodeData::GetAccessor(data))
    }

    fn visit_set_accessor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::SetAccessorData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) =
            self.visit_function_scope(SyntaxKind::SetAccessor, data.parameters, data.body, false)?;
        self.update_without_visit(original, NodeData::SetAccessor(data))
    }

    fn visit_constructor(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ConstructorData,
    ) -> Result<NodeId, TransformError> {
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.r#type = self.visit_optional_node(data.r#type)?;
        (data.parameters, data.body) =
            self.visit_function_scope(SyntaxKind::Constructor, data.parameters, data.body, false)?;
        self.update_without_visit(original, NodeData::Constructor(data))
    }

    fn visit_function_scope(
        &mut self,
        kind: SyntaxKind,
        parameters: Option<NodeArrayId>,
        body: Option<NodeId>,
        concise_body: bool,
    ) -> Result<(Option<NodeArrayId>, Option<NodeId>), TransformError> {
        let (previous, scope) = self
            .generated_bindings
            .enter(GeneratedBindingOwner::FunctionBody);
        self.context.start_lexical_environment()?;
        let operation: Result<(Option<NodeArrayId>, Option<NodeId>), TransformError> = (|| {
            let plan = self.plan_parameter_hoists(parameters)?;
            self.context
                .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, true)?;
            let parameters = self.visit_parameter_list(parameters, &plan)?;
            self.context
                .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, false)?;
            let body = self.visit_optional_node(body)?;
            Ok((parameters, body))
        })();
        let lexical_environment = self.context.end_lexical_environment();
        let generated_bindings = self.generated_bindings.exit(previous, scope);

        let (parameters, body) = operation?;
        let lexical_environment = lexical_environment?;
        self.assert_binding_plan(&generated_bindings, &lexical_environment);
        let body =
            self.merge_function_lexical_environment(kind, body, concise_body, lexical_environment)?;
        Ok((parameters, body))
    }

    fn plan_parameter_hoists(
        &mut self,
        parameters: Option<NodeArrayId>,
    ) -> Result<ParameterHoistPlan, TransformError> {
        let nodes = self.array_nodes(parameters)?;
        let requires_hoist = nodes
            .iter()
            .map(|parameter| self.subtree_requires_hoisted_temp(*parameter, true))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|required| required);
        let mut binding_aliases = Vec::with_capacity(nodes.len());
        for parameter in nodes {
            let alias = if requires_hoist && self.parameter_has_binding_pattern(parameter)? {
                Some(self.generated_bindings.allocate_local_temp())
            } else {
                None
            };
            binding_aliases.push(alias);
        }
        Ok(ParameterHoistPlan { binding_aliases })
    }

    fn subtree_requires_hoisted_temp(
        &self,
        node: TransformNode,
        root: bool,
    ) -> Result<bool, TransformError> {
        let record = self.context.arena().node(node)?.clone();
        if !root && Self::is_function_scope_kind(record.kind) {
            return Ok(false);
        }
        if let NodeData::BinaryExpression(data) = &record.data {
            let operator = data
                .operator_token
                .map(|operator| {
                    self.context
                        .arena()
                        .node(self.node(operator))
                        .map(|node| node.kind)
                })
                .transpose()?;
            if operator.is_some_and(Self::is_logical_assignment) {
                let left = data.left.map(|left| self.node(left)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BinaryExpression,
                        field: "left",
                    },
                )?;
                let left = self.skip_parentheses(left)?;
                match &self.context.arena().node(left)?.data {
                    NodeData::PropertyAccessExpression(access) => {
                        let receiver = access
                            .expression
                            .map(|receiver| self.node(receiver))
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::PropertyAccessExpression,
                                field: "expression",
                            })?;
                        if !self.is_simple_copiable_expression(receiver)? {
                            return Ok(true);
                        }
                    }
                    NodeData::ElementAccessExpression(access) => {
                        let receiver = access
                            .expression
                            .map(|receiver| self.node(receiver))
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ElementAccessExpression,
                                field: "expression",
                            })?;
                        let argument = access
                            .argument_expression
                            .map(|argument| self.node(argument))
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ElementAccessExpression,
                                field: "argument_expression",
                            })?;
                        if !self.is_simple_copiable_expression(receiver)?
                            || !self.is_simple_copiable_expression(argument)?
                        {
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
            }
        }
        let syntax = self.context.arena().source(self.source)?.syntax();
        let mut children = Vec::new();
        for_each_child(&syntax.arena, &record, |child| {
            children.push(child);
            false
        });
        for child in children {
            if self.subtree_requires_hoisted_temp(self.node(child), false)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn visit_parameter_list(
        &mut self,
        parameters: Option<NodeArrayId>,
        plan: &ParameterHoistPlan,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let original = self.array(parameters);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        if nodes.len() != plan.binding_aliases.len() {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "parameter hoist plan",
            });
        }
        let mut visited = Vec::with_capacity(nodes.len());
        for node in nodes {
            let node = self
                .visit(node)?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "parameter",
                })?;
            visited.push(self.node(node));
        }
        if self
            .context
            .lexical_environment_flags()
            .contains(LexicalEnvironmentFlags::VARIABLES_HOISTED_IN_PARAMETERS)
        {
            for (parameter, alias) in visited.iter_mut().zip(&plan.binding_aliases) {
                *parameter = self.lower_parameter_default(*parameter, alias.as_deref())?;
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        self.arrays.insert(parameters, Some(updated.array()));
        Ok(Some(updated.array()))
    }

    fn lower_parameter_default(
        &mut self,
        parameter: TransformNode,
        binding_alias: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Parameter(mut data) = self.context.arena().node(parameter)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "parameter data",
            });
        };
        if data.dot_dot_dot_token.is_some() {
            return Ok(parameter);
        }
        let name =
            data.name
                .map(|name| self.node(name))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "name",
                })?;
        let name_kind = self.context.arena().node(name)?.kind;

        if matches!(
            name_kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ) {
            let alias = binding_alias.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "planned binding-pattern alias",
            })?;
            let value = if let Some(initializer) = data.initializer.map(|id| self.node(id)) {
                let condition_name = self.create_identifier(alias)?;
                let condition = self.create_strict_undefined_check(condition_name)?;
                let fallback_name = self.create_identifier(alias)?;
                self.create_conditional(condition, initializer, fallback_name)?
            } else {
                self.create_identifier(alias)?
            };
            let declaration = self.create_variable_declaration(name, Some(value))?;
            let statement = self.create_variable_statement(vec![declaration])?;
            self.context.add_initialization_statement(statement)?;
            let alias_name = self.create_identifier(alias)?;
            data.name = Some(alias_name.node());
            data.initializer = None;
        } else if let Some(initializer) = data.initializer.map(|id| self.node(id)) {
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
            let flags = self.child_flags(&[condition, block])?;
            let if_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                    expression: Some(condition.node()),
                    then_statement: Some(block.node()),
                    else_statement: None,
                }),
                flags,
            )?;
            self.context.add_initialization_statement(if_statement)?;
            data.initializer = None;
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
        concise_body: bool,
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
            let NodeData::Block(mut data) = self.context.arena().node(body)?.data.clone() else {
                unreachable!("block kind has block data")
            };
            data.statements = self.merge_statement_array(data.statements, lexical_environment)?;
            let flags =
                flags_after_update(self.context.arena(), body, &NodeData::Block(data.clone()))?;
            self.context
                .factory()?
                .update_node(body, NodeData::Block(data), flags)?
        } else if concise_body && function_kind == SyntaxKind::ArrowFunction {
            let return_flags = self.child_flags(&[body])?
                | TransformFlags::CONTAINS_HOISTED_DECLARATION_OR_COMPLETION;
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
                return_flags,
            )?;
            let statements = self
                .context
                .factory()?
                .create_node_array(self.source, vec![return_statement])?;
            let block_flags = self.context.arena().array_transform_flags(statements);
            let block = self.context.factory()?.create_node(
                self.source,
                NodeData::Block(tsc_syntax::nodes::BlockData {
                    statements: Some(statements.array()),
                }),
                block_flags,
            )?;
            self.context.factory()?.set_multi_line(block, false)?;
            let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
                unreachable!("created block has block data")
            };
            data.statements = self.merge_statement_array(data.statements, lexical_environment)?;
            let flags = self.context.arena().transform_flags(block);
            self.context
                .factory()?
                .update_node(block, NodeData::Block(data), flags)?
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: function_kind,
                field: "block function body for lexical declarations",
            });
        };
        Ok(Some(body.node()))
    }

    fn merge_statement_array(
        &mut self,
        statements: Option<NodeArrayId>,
        lexical_environment: LexicalEnvironment,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let original = statements.map(|statements| self.array(statements));
        let mut statements = self.array_nodes(statements)?;
        let standard_end = statements
            .iter()
            .take_while(|statement| self.is_prologue_statement(**statement).unwrap_or(false))
            .count();
        let function_end = standard_end
            + statements[standard_end..]
                .iter()
                .take_while(|statement| self.is_hoisted_function(**statement).unwrap_or(false))
                .count();
        let variable_end = function_end
            + statements[function_end..]
                .iter()
                .take_while(|statement| {
                    self.is_hoisted_variable_statement(**statement)
                        .unwrap_or(false)
                })
                .count();

        let initialization = lexical_environment.initialization_statements().to_vec();
        if !initialization.is_empty() {
            statements.splice(variable_end..variable_end, initialization);
        }

        if !lexical_environment.variable_declarations().is_empty() {
            let mut declarations =
                Vec::with_capacity(lexical_environment.variable_declarations().len());
            for name in lexical_environment.variable_declarations() {
                let declaration = self.create_variable_declaration(*name, None)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(declaration)
                    .add_flags(EmitFlags::NO_NESTED_SOURCE_MAPS);
                declarations.push(declaration);
            }
            let statement = self.create_variable_statement(declarations)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::CUSTOM_PROLOGUE);
            statements.insert(function_end, statement);
        }

        if !lexical_environment.function_declarations().is_empty() {
            statements.splice(
                standard_end..standard_end,
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

    fn allocate_hoisted_temp(&mut self) -> Result<String, TransformError> {
        let name = self.generated_bindings.allocate_temp();
        let declaration = self.create_identifier(&name)?;
        self.context.hoist_variable_declaration(declaration)?;
        Ok(name)
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

    fn parameter_has_binding_pattern(
        &self,
        parameter: TransformNode,
    ) -> Result<bool, TransformError> {
        let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
            return Ok(false);
        };
        let Some(name) = data.name.map(|name| self.node(name)) else {
            return Ok(false);
        };
        Ok(matches!(
            self.context.arena().node(name)?.kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ))
    }

    fn is_simple_copiable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.context.arena().node(expression)?.kind;
        Ok(matches!(
            kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
                | SyntaxKind::Identifier
        ) || kind.value() >= SyntaxKind::FirstKeyword.value()
            && kind.value() <= SyntaxKind::LastKeyword.value())
    }

    fn skip_parentheses(
        &self,
        mut expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let NodeData::ParenthesizedExpression(data) =
                &self.context.arena().node(expression)?.data
            else {
                return Ok(expression);
            };
            expression = data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ParenthesizedExpression,
                    field: "expression",
                })?;
        }
    }

    const fn is_logical_assignment(operator: SyntaxKind) -> bool {
        matches!(
            operator,
            SyntaxKind::BarBarEqualsToken
                | SyntaxKind::AmpersandAmpersandEqualsToken
                | SyntaxKind::QuestionQuestionEqualsToken
        )
    }

    const fn non_assignment_operator(operator: SyntaxKind) -> SyntaxKind {
        match operator {
            SyntaxKind::BarBarEqualsToken => SyntaxKind::BarBarToken,
            SyntaxKind::AmpersandAmpersandEqualsToken => SyntaxKind::AmpersandAmpersandToken,
            SyntaxKind::QuestionQuestionEqualsToken => SyntaxKind::QuestionQuestionToken,
            _ => operator,
        }
    }

    const fn is_function_scope_kind(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ArrowFunction
                | SyntaxKind::Constructor
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::GetAccessor
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::SetAccessor
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

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let right = if self.is_comma_sequence(right)? {
            self.create_parenthesized(right)?
        } else {
            right
        };
        self.create_binary(left, SyntaxKind::EqualsToken, right)
    }

    fn is_comma_sequence(&self, expression: TransformNode) -> Result<bool, TransformError> {
        let record = self.context.arena().node(expression)?;
        Ok(match &record.data {
            NodeData::CommaListExpression(_) => true,
            NodeData::BinaryExpression(data) => data
                .operator_token
                .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                .is_some_and(|operator| {
                    self.context
                        .arena()
                        .node(operator)
                        .is_ok_and(|operator| operator.kind == SyntaxKind::CommaToken)
                }),
            _ => false,
        })
    }

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let operator_token =
            self.context
                .factory()?
                .create_token(self.source, operator, TransformFlags::NONE)?;
        let mut flags = self.child_flags(&[left, operator_token, right])?;
        flags |= match operator {
            SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken => TransformFlags::CONTAINS_ES_2021,
            SyntaxKind::QuestionQuestionToken => TransformFlags::CONTAINS_ES_2020,
            _ => TransformFlags::NONE,
        };
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(operator_token.node()),
                right: Some(right.node()),
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

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[expression])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(expression.node()),
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

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: "0".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        let flags = self.child_flags(&[zero])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
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
        let statement_flags = self.child_flags(&[list])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            statement_flags,
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
        let block_flags = self.context.arena().array_transform_flags(statements);
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            block_flags,
        )?;
        self.context.factory()?.set_multi_line(block, multi_line)
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
        self.update_without_visit(original, data)
    }

    fn update_without_visit(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
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

    fn identifier_text(&self, node: TransformNode) -> Result<&str, TransformError> {
        match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Ok(&data.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "identifier parameter name",
            }),
        }
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

    fn is_hoisted_function(&self, statement: TransformNode) -> Result<bool, TransformError> {
        Ok(self.is_custom_prologue(statement)
            && self.context.arena().node(statement)?.kind == SyntaxKind::FunctionDeclaration)
    }

    fn is_hoisted_variable_statement(
        &self,
        statement: TransformNode,
    ) -> Result<bool, TransformError> {
        if !self.is_custom_prologue(statement) {
            return Ok(false);
        }
        let NodeData::VariableStatement(data) = &self.context.arena().node(statement)?.data else {
            return Ok(false);
        };
        let Some(list) = data
            .declaration_list
            .and_then(|list| self.context.arena().node_ref(self.source, list))
        else {
            return Ok(false);
        };
        let NodeData::VariableDeclarationList(list) = &self.context.arena().node(list)?.data else {
            return Ok(false);
        };
        Ok(self.array_nodes(list.declarations)?.iter().all(|declaration| {
            matches!(
                self.context.arena().node(*declaration).ok().map(|node| &node.data),
                Some(NodeData::VariableDeclaration(data))
                    if data.initializer.is_none()
                        && data.name.is_some_and(|name| self.context.arena().node(self.node(name)).is_ok_and(|name| name.kind == SyntaxKind::Identifier))
            )
        }))
    }

    fn is_custom_prologue(&self, statement: TransformNode) -> bool {
        self.context
            .arena()
            .metadata(statement)
            .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::CUSTOM_PROLOGUE))
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

#[derive(Clone, Copy)]
enum StabilizedAccessOperand {
    Copied(TransformNode),
    Hoisted {
        read: TransformNode,
        initialization: TransformNode,
    },
}

impl StabilizedAccessOperand {
    const fn read(self) -> TransformNode {
        match self {
            Self::Copied(operand) | Self::Hoisted { read: operand, .. } => operand,
        }
    }

    const fn initialization(self) -> TransformNode {
        match self {
            Self::Copied(operand)
            | Self::Hoisted {
                initialization: operand,
                ..
            } => operand,
        }
    }
}

impl NodeDataChildVisitor for Es2021Visitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("ES2021 child belongs to the current transform source")
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

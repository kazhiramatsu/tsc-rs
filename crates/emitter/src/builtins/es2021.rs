//! H2.5 target-ladder lowering shared by the ES2021, ES2020, and ES2019 passes.
//!
//! The pinned TypeScript transformer defines evaluation order and observable
//! output. Rust owns that behavior through explicit pass, optional-chain,
//! synthetic-reference, access-stabilization, and lexical-scope plans rather
//! than mirroring TypeScript's nested closures or synthetic internal nodes.

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
    target_bindings::{
        collect_untagged_identifier_texts, finalize_generated_binding_names,
        is_function_scope_kind, TargetBinding,
    },
};

/// tsc-port: transformES2021 @6.0.3
/// tsc-hash: 9f18d49525c22011f2b39fd966d1d6bb59ebe1fb9b2099d72314a94fbddf8e1c
/// tsc-span: _tsc.js:103205-103275
pub(super) fn transform_es2021(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(TargetTransformer {
        target: options.emit_script_target(),
        pass: TargetPass::Es2021,
    })
}

/// tsc-port: transformES2020 @6.0.3
/// tsc-hash: d4fd052da60bf3b0c5743c0994e4afe4ac556af672203c828734f67325124c7d
/// tsc-span: _tsc.js:102943-103202
pub(super) fn transform_es2020(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(TargetTransformer {
        target: options.emit_script_target(),
        pass: TargetPass::Es2020,
    })
}

/// tsc-port: transformES2019 @6.0.3
/// tsc-hash: 929becb4a2bc7973a7c0750516971f7a655363890cdddd825d00b53f37ee1e56
/// tsc-span: _tsc.js:102907-102940
pub(super) fn transform_es2019(options: &CompilerOptions) -> Box<dyn Transformer> {
    Box::new(TargetTransformer {
        target: options.emit_script_target(),
        pass: TargetPass::Es2019,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetPass {
    Es2021,
    Es2020,
    Es2019,
}

impl TargetPass {
    const fn name(self) -> &'static str {
        match self {
            Self::Es2021 => "transformES2021",
            Self::Es2020 => "transformES2020",
            Self::Es2019 => "transformES2019",
        }
    }

    const fn feature_flag(self) -> TransformFlags {
        match self {
            Self::Es2021 => TransformFlags::CONTAINS_ES_2021,
            Self::Es2020 => TransformFlags::CONTAINS_ES_2020,
            Self::Es2019 => TransformFlags::CONTAINS_ES_2019,
        }
    }

    const fn upper_target(self) -> ScriptTarget {
        match self {
            Self::Es2021 => ScriptTarget::ES2021,
            Self::Es2020 => ScriptTarget::ES2020,
            Self::Es2019 => ScriptTarget::ES2019,
        }
    }

    const fn unsupported_detail(self) -> &'static str {
        match self {
            Self::Es2021 => {
                "H2.5b/H2.5c admit transformES2021 for the ES2019 and ES2020 target boundaries"
            }
            Self::Es2020 => "H2.5c admits transformES2020 for the ES2019 target boundary",
            Self::Es2019 => "H2.5d admits transformES2019 for the ES2018 target boundary",
        }
    }

    fn is_final_for_target(self, target: ScriptTarget) -> bool {
        match self {
            Self::Es2021 => target >= ScriptTarget::ES2020,
            Self::Es2020 => target >= ScriptTarget::ES2019,
            Self::Es2019 => target >= ScriptTarget::ES2018,
        }
    }
}

struct TargetTransformer {
    target: ScriptTarget,
    pass: TargetPass,
}

impl Transformer for TargetTransformer {
    fn name(&self) -> &'static str {
        self.pass.name()
    }

    fn initialize(&mut self, _context: &mut TransformationContext) -> Result<(), TransformError> {
        if self.target < ScriptTarget::ES2016 || self.target >= self.pass.upper_target() {
            return Err(TransformError::UnsupportedCompilerOption {
                option: self.pass.name(),
                detail: self.pass.unsupported_detail(),
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
        let mut visitor = TargetVisitor::new(context, source, self.pass, current_root)?;
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
        if self.pass.is_final_for_target(self.target) {
            finalize_generated_binding_names(visitor.context, source, transformed)?;
        }
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

#[derive(Clone, Debug)]
struct ParameterHoistPlan {
    binding_aliases: Vec<Option<TargetBinding>>,
}

/// A call target that carries the receiver required by JavaScript's method
/// call semantics. TypeScript models this with a synthetic AST node that is
/// never printed; Rust keeps the transient state outside the syntax arena.
#[derive(Clone, Copy, Debug)]
struct SyntheticReference {
    expression: TransformNode,
    receiver: CallReceiver,
}

#[derive(Clone, Copy, Debug)]
enum CallReceiver {
    Source(TransformNode),
    Generated(TransformNode),
}

#[derive(Clone, Copy, Debug)]
enum VisitedExpression {
    Value(TransformNode),
    Reference(SyntheticReference),
}

impl VisitedExpression {
    fn into_value(self, field: &'static str) -> Result<TransformNode, TransformError> {
        match self {
            Self::Value(expression) => Ok(expression),
            Self::Reference(_) => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::SyntheticReferenceExpression,
                field,
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct OptionalChainPlan {
    base: TransformNode,
    segments: Vec<OptionalChainSegment>,
}

#[derive(Clone, Copy, Debug)]
enum OptionalChainSegment {
    Property {
        original: TransformNode,
        name: NodeId,
    },
    Element {
        original: TransformNode,
        argument: NodeId,
    },
    Call {
        original: TransformNode,
        arguments: Option<NodeArrayId>,
    },
}

#[derive(Clone, Debug)]
enum AccessExpression {
    Property(tsc_syntax::nodes::PropertyAccessExpressionData),
    Element(tsc_syntax::nodes::ElementAccessExpressionData),
}

impl AccessExpression {
    const fn kind(&self) -> SyntaxKind {
        match self {
            Self::Property(_) => SyntaxKind::PropertyAccessExpression,
            Self::Element(_) => SyntaxKind::ElementAccessExpression,
        }
    }

    const fn expression(&self) -> Option<NodeId> {
        match self {
            Self::Property(data) => data.expression,
            Self::Element(data) => data.expression,
        }
    }
}

impl OptionalChainSegment {
    const fn original(self) -> TransformNode {
        match self {
            Self::Property { original, .. }
            | Self::Element { original, .. }
            | Self::Call { original, .. } => original,
        }
    }
}

struct TargetVisitor<'context> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    pass: TargetPass,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    generated_bindings: GeneratedBindingScopes,
}

impl<'context> TargetVisitor<'context> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        pass: TargetPass,
        root: TransformNode,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_untagged_identifier_texts(context.arena(), source, root)?,
                AncestorBindingPolicy::AllowShadow,
            ),
            context,
            source,
            pass,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
        })
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
            .contains(self.pass.feature_flag())
        {
            self.nodes.insert(id, Some(id));
            return Ok(Some(id));
        }

        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::BinaryExpression(data) => Some(self.visit_binary_expression(original, data)?),
            NodeData::CallExpression(data) if self.pass == TargetPass::Es2020 => Some(
                self.visit_non_optional_call_expression(original, data, false)?
                    .into_value("top-level ES2020 call")?
                    .node(),
            ),
            NodeData::PropertyAccessExpression(data) if self.pass == TargetPass::Es2020 => Some(
                self.visit_non_optional_property_or_element_access_expression(
                    original,
                    AccessExpression::Property(data),
                    false,
                    false,
                )?
                .into_value("top-level ES2020 property access")?
                .node(),
            ),
            NodeData::ElementAccessExpression(data) if self.pass == TargetPass::Es2020 => Some(
                self.visit_non_optional_property_or_element_access_expression(
                    original,
                    AccessExpression::Element(data),
                    false,
                    false,
                )?
                .into_value("top-level ES2020 element access")?
                .node(),
            ),
            NodeData::DeleteExpression(data) if self.pass == TargetPass::Es2020 => {
                Some(self.visit_delete_expression(original, data)?)
            }
            NodeData::CatchClause(data) if self.pass == TargetPass::Es2019 => {
                Some(self.visit_catch_clause(original, data)?)
            }
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
        if self.pass == TargetPass::Es2020 && operator == Some(SyntaxKind::QuestionQuestionToken) {
            return self.transform_nullish_coalescing_expression(original, data);
        }
        if self.pass != TargetPass::Es2021 {
            return self.update_generic(original, NodeData::BinaryExpression(data));
        }
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

    fn visit_catch_clause(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CatchClauseData,
    ) -> Result<NodeId, TransformError> {
        data.variable_declaration = if let Some(variable) = data.variable_declaration {
            self.visit(variable)?
        } else {
            let binding = self.allocate_local_binding()?;
            let name = self.create_generated_identifier(&binding)?;
            Some(self.create_variable_declaration(name, None)?.node())
        };
        data.block = self.visit_optional_node(data.block)?;
        self.update_without_visit(original, NodeData::CatchClause(data))
    }

    fn stabilize_access_operand(
        &mut self,
        operand: TransformNode,
    ) -> Result<StabilizedAccessOperand, TransformError> {
        if self.is_simple_copiable_expression(operand)? {
            return Ok(StabilizedAccessOperand::Copied(operand));
        }
        let binding = self.allocate_hoisted_temp()?;
        let read = self.create_generated_identifier(&binding)?;
        let initialized = self.create_generated_identifier(&binding)?;
        let initialization = self.create_assignment(initialized, operand)?;
        Ok(StabilizedAccessOperand::Hoisted {
            read,
            initialization,
        })
    }

    fn transform_nullish_coalescing_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<NodeId, TransformError> {
        let visited_left = self.visit_required(
            data.left,
            SyntaxKind::BinaryExpression,
            "left of nullish coalescing expression",
        )?;
        let (left, repeated_left) = if self.is_simple_copiable_expression(visited_left)? {
            (visited_left, visited_left)
        } else {
            let temporary = self.allocate_hoisted_temp()?;
            let assignment_target = self.create_generated_identifier(&temporary)?;
            let repeated_left = self.create_generated_identifier(&temporary)?;
            let assignment = self.create_assignment(assignment_target, visited_left)?;
            (self.create_parenthesized(assignment)?, repeated_left)
        };
        let condition = self.create_not_null_condition(left, repeated_left, false)?;
        let when_true = repeated_left;
        let when_false = self.visit_required(
            data.right,
            SyntaxKind::BinaryExpression,
            "right of nullish coalescing expression",
        )?;
        let result = self.create_conditional(condition, when_true, when_false)?;
        Ok(self.set_original_and_range(result, original)?.node())
    }

    fn visit_delete_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::DeleteExpressionData,
    ) -> Result<NodeId, TransformError> {
        let expression = data
            .expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::DeleteExpression,
                field: "expression",
            })?;
        let unwrapped = self.skip_parentheses(expression)?;
        if self.is_optional_chain(unwrapped)? {
            let transformed = self
                .visit_non_optional_expression(expression, false, true)?
                .into_value("delete optional-chain result")?;
            self.context
                .arena_mut()?
                .set_original_node(transformed, Some(original))?;
            return Ok(transformed.node());
        }
        data.expression = Some(
            self.visit_required(data.expression, SyntaxKind::DeleteExpression, "expression")?
                .node(),
        );
        self.update_without_visit(original, NodeData::DeleteExpression(data))
    }

    fn visit_non_optional_expression(
        &mut self,
        original: TransformNode,
        capture_receiver: bool,
        is_delete: bool,
    ) -> Result<VisitedExpression, TransformError> {
        let record = self.context.arena().node(original)?.clone();
        match record.data {
            NodeData::ParenthesizedExpression(data) => self
                .visit_non_optional_parenthesized_expression(
                    original,
                    data,
                    capture_receiver,
                    is_delete,
                ),
            NodeData::PropertyAccessExpression(data) => self
                .visit_non_optional_property_or_element_access_expression(
                    original,
                    AccessExpression::Property(data),
                    capture_receiver,
                    is_delete,
                ),
            NodeData::ElementAccessExpression(data) => self
                .visit_non_optional_property_or_element_access_expression(
                    original,
                    AccessExpression::Element(data),
                    capture_receiver,
                    is_delete,
                ),
            NodeData::CallExpression(data) => {
                self.visit_non_optional_call_expression(original, data, capture_receiver)
            }
            _ => self
                .visit(original.node())?
                .map(|node| VisitedExpression::Value(self.node(node)))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "ES2020 expression",
                }),
        }
    }

    fn visit_non_optional_parenthesized_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ParenthesizedExpressionData,
        capture_receiver: bool,
        is_delete: bool,
    ) -> Result<VisitedExpression, TransformError> {
        let expression = data
            .expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ParenthesizedExpression,
                field: "expression",
            })?;
        match self.visit_non_optional_expression(expression, capture_receiver, is_delete)? {
            VisitedExpression::Value(expression) => {
                data.expression = Some(expression.node());
                let updated =
                    self.update_without_visit(original, NodeData::ParenthesizedExpression(data))?;
                Ok(VisitedExpression::Value(self.node(updated)))
            }
            VisitedExpression::Reference(reference) => {
                data.expression = Some(reference.expression.node());
                let updated =
                    self.update_without_visit(original, NodeData::ParenthesizedExpression(data))?;
                Ok(VisitedExpression::Reference(SyntheticReference {
                    expression: self.node(updated),
                    receiver: reference.receiver,
                }))
            }
        }
    }

    fn visit_non_optional_property_or_element_access_expression(
        &mut self,
        original: TransformNode,
        access: AccessExpression,
        capture_receiver: bool,
        is_delete: bool,
    ) -> Result<VisitedExpression, TransformError> {
        if self.is_optional_chain(original)? {
            return self.visit_optional_expression(original, capture_receiver, is_delete);
        }

        let receiver = access.expression().map(|id| self.node(id)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: access.kind(),
                field: "expression",
            },
        )?;
        let mut receiver = self
            .visit(receiver.node())?
            .map(|node| self.node(node))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: access.kind(),
                field: "expression",
            })?;
        let call_receiver = if capture_receiver {
            if self.is_simple_copiable_expression(receiver)? {
                Some(CallReceiver::Source(receiver))
            } else {
                let temporary = self.allocate_hoisted_temp()?;
                let assignment_target = self.create_generated_identifier(&temporary)?;
                let receiver_read = self.create_generated_identifier(&temporary)?;
                let assignment = self.create_assignment(assignment_target, receiver)?;
                receiver = self.create_parenthesized(assignment)?;
                Some(CallReceiver::Generated(receiver_read))
            }
        } else {
            None
        };
        let receiver = if self.requires_left_side_parentheses(receiver)? {
            self.create_parenthesized(receiver)?
        } else {
            receiver
        };

        let expression = match access {
            AccessExpression::Property(mut data) => {
                data.expression = Some(receiver.node());
                data.name = Some(
                    self.visit_required(data.name, SyntaxKind::PropertyAccessExpression, "name")?
                        .node(),
                );
                let updated =
                    self.update_without_visit(original, NodeData::PropertyAccessExpression(data))?;
                self.node(updated)
            }
            AccessExpression::Element(mut data) => {
                data.expression = Some(receiver.node());
                data.argument_expression = Some(
                    self.visit_required(
                        data.argument_expression,
                        SyntaxKind::ElementAccessExpression,
                        "argument_expression",
                    )?
                    .node(),
                );
                let updated =
                    self.update_without_visit(original, NodeData::ElementAccessExpression(data))?;
                self.node(updated)
            }
        };
        Ok(match call_receiver {
            Some(receiver) => VisitedExpression::Reference(SyntheticReference {
                expression,
                receiver,
            }),
            None => VisitedExpression::Value(expression),
        })
    }

    fn visit_non_optional_call_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CallExpressionData,
        capture_receiver: bool,
    ) -> Result<VisitedExpression, TransformError> {
        if self.is_optional_chain(original)? {
            return self.visit_optional_expression(original, capture_receiver, false);
        }
        let callee = data
            .expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::CallExpression,
                field: "expression",
            })?;
        let parenthesized_optional_chain = self.context.arena().node(callee)?.kind
            == SyntaxKind::ParenthesizedExpression
            && self.is_optional_chain(self.skip_parentheses(callee)?)?;
        if !parenthesized_optional_chain {
            let updated = self.update_generic(original, NodeData::CallExpression(data))?;
            return Ok(VisitedExpression::Value(self.node(updated)));
        }

        let callee = self.visit_non_optional_expression(callee, true, false)?;
        data.arguments = self.visit_optional_nodes(data.arguments)?;
        data.type_arguments = None;
        match callee {
            VisitedExpression::Reference(reference) => {
                let receiver = self.prepare_call_receiver(reference.receiver)?;
                let call =
                    self.create_function_call_call(reference.expression, receiver, data.arguments)?;
                let call = self.context.factory()?.set_text_range(call, original)?;
                Ok(VisitedExpression::Value(call))
            }
            VisitedExpression::Value(callee) => {
                data.expression = Some(callee.node());
                let updated =
                    self.update_without_visit(original, NodeData::CallExpression(data))?;
                Ok(VisitedExpression::Value(self.node(updated)))
            }
        }
    }

    fn visit_optional_expression(
        &mut self,
        original: TransformNode,
        capture_receiver: bool,
        is_delete: bool,
    ) -> Result<VisitedExpression, TransformError> {
        let plan = self.flatten_optional_chain(original)?;
        let base = self.skip_partially_emitted_expressions(plan.base)?;
        let first_is_call = matches!(
            plan.segments.first(),
            Some(OptionalChainSegment::Call { .. })
        );
        let visited_base = self.visit_non_optional_expression(base, first_is_call, false)?;
        let (captured_base, left_receiver) = match visited_base {
            VisitedExpression::Value(expression) => (expression, None),
            VisitedExpression::Reference(reference) => {
                (reference.expression, Some(reference.receiver))
            }
        };
        let mut left_expression =
            self.restore_partially_emitted_expressions(plan.base, captured_base)?;
        let captured_base = if self.is_simple_copiable_expression(captured_base)? {
            captured_base
        } else {
            let temporary = self.allocate_hoisted_temp()?;
            let assignment_target = self.create_generated_identifier(&temporary)?;
            let captured_base = self.create_generated_identifier(&temporary)?;
            let assignment = self.create_assignment(assignment_target, left_expression)?;
            left_expression = self.create_parenthesized(assignment)?;
            captured_base
        };

        let mut right_expression = captured_base;
        let mut result_receiver = None;
        let last = plan.segments.len().saturating_sub(1);
        for (index, segment) in plan.segments.into_iter().enumerate() {
            match segment {
                OptionalChainSegment::Property { name, .. } => {
                    if index == last && capture_receiver {
                        let (initialized, receiver) =
                            self.capture_optional_result_receiver(right_expression)?;
                        right_expression = initialized;
                        result_receiver = Some(receiver);
                    }
                    let name = self.visit_required(
                        Some(name),
                        SyntaxKind::PropertyAccessExpression,
                        "name",
                    )?;
                    right_expression = self.create_property_access(right_expression, name)?;
                }
                OptionalChainSegment::Element { argument, .. } => {
                    if index == last && capture_receiver {
                        let (initialized, receiver) =
                            self.capture_optional_result_receiver(right_expression)?;
                        right_expression = initialized;
                        result_receiver = Some(receiver);
                    }
                    let argument = self.visit_required(
                        Some(argument),
                        SyntaxKind::ElementAccessExpression,
                        "argument_expression",
                    )?;
                    self.context
                        .arena_mut()?
                        .metadata_mut(argument)
                        .add_flags(EmitFlags::NO_LEADING_COMMENTS);
                    right_expression = self.create_element_access(right_expression, argument)?;
                }
                OptionalChainSegment::Call { arguments, .. } => {
                    let arguments = self.visit_optional_nodes(arguments)?;
                    right_expression = if index == 0 {
                        if let Some(receiver) = left_receiver {
                            let receiver = self.prepare_call_receiver(receiver)?;
                            self.create_function_call_call(right_expression, receiver, arguments)?
                        } else {
                            self.create_call(right_expression, arguments)?
                        }
                    } else {
                        self.create_call(right_expression, arguments)?
                    };
                }
            }
            self.context
                .arena_mut()?
                .set_original_node(right_expression, Some(segment.original()))?;
        }

        let condition = self.create_not_null_condition(left_expression, captured_base, true)?;
        let target = if is_delete {
            let when_true = self.create_true()?;
            let when_false = self.create_delete(right_expression)?;
            self.create_conditional(condition, when_true, when_false)?
        } else {
            let when_true = self.create_void_zero()?;
            self.create_conditional(condition, when_true, right_expression)?
        };
        let target = self.context.factory()?.set_text_range(target, original)?;
        Ok(match result_receiver {
            Some(receiver) => VisitedExpression::Reference(SyntheticReference {
                expression: target,
                receiver,
            }),
            None => VisitedExpression::Value(target),
        })
    }

    fn flatten_optional_chain(
        &self,
        original: TransformNode,
    ) -> Result<OptionalChainPlan, TransformError> {
        let mut current = original;
        let mut segments = Vec::new();
        loop {
            let record = self.context.arena().node(current)?;
            let (segment, expression, has_question_dot) = match &record.data {
                NodeData::PropertyAccessExpression(data) => (
                    OptionalChainSegment::Property {
                        original: current,
                        name: data.name.ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyAccessExpression,
                            field: "name",
                        })?,
                    },
                    data.expression,
                    data.question_dot_token.is_some(),
                ),
                NodeData::ElementAccessExpression(data) => (
                    OptionalChainSegment::Element {
                        original: current,
                        argument: data.argument_expression.ok_or(
                            TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ElementAccessExpression,
                                field: "argument_expression",
                            },
                        )?,
                    },
                    data.expression,
                    data.question_dot_token.is_some(),
                ),
                NodeData::CallExpression(data) => (
                    OptionalChainSegment::Call {
                        original: current,
                        arguments: data.arguments,
                    },
                    data.expression,
                    data.question_dot_token.is_some(),
                ),
                _ => {
                    return Err(TransformError::RequiredChildRemoved {
                        parent: record.kind,
                        field: "optional-chain segment",
                    })
                }
            };
            segments.push(segment);
            let expression = expression.map(|expression| self.node(expression)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "expression",
                },
            )?;
            if has_question_dot {
                segments.reverse();
                return Ok(OptionalChainPlan {
                    base: expression,
                    segments,
                });
            }
            current = self.skip_partially_emitted_expressions(expression)?;
            if !self.is_optional_chain(current)? {
                return Err(TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "optional-chain root",
                });
            }
        }
    }

    fn capture_optional_result_receiver(
        &mut self,
        expression: TransformNode,
    ) -> Result<(TransformNode, CallReceiver), TransformError> {
        if self.is_simple_copiable_expression(expression)? {
            return Ok((expression, CallReceiver::Source(expression)));
        }
        let temporary = self.allocate_hoisted_temp()?;
        let assignment_target = self.create_generated_identifier(&temporary)?;
        let receiver = self.create_generated_identifier(&temporary)?;
        let assignment = self.create_assignment(assignment_target, expression)?;
        Ok((
            self.create_parenthesized(assignment)?,
            CallReceiver::Generated(receiver),
        ))
    }

    fn prepare_call_receiver(
        &mut self,
        receiver: CallReceiver,
    ) -> Result<TransformNode, TransformError> {
        let receiver = match receiver {
            CallReceiver::Generated(receiver) => receiver,
            CallReceiver::Source(receiver) => {
                let clone = self.context.factory()?.clone_node(receiver)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(clone)
                    .add_flags(EmitFlags::NO_COMMENTS);
                clone
            }
        };
        if self.context.arena().node(receiver)?.kind == SyntaxKind::SuperKeyword {
            self.create_this()
        } else {
            Ok(receiver)
        }
    }

    fn skip_partially_emitted_expressions(
        &self,
        mut expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let NodeData::PartiallyEmittedExpression(data) =
                &self.context.arena().node(expression)?.data
            else {
                return Ok(expression);
            };
            expression = data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PartiallyEmittedExpression,
                    field: "expression",
                })?;
        }
    }

    fn restore_partially_emitted_expressions(
        &mut self,
        original: TransformNode,
        replacement: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let mut wrappers = Vec::new();
        let mut current = original;
        loop {
            let NodeData::PartiallyEmittedExpression(data) =
                &self.context.arena().node(current)?.data
            else {
                break;
            };
            wrappers.push(current);
            current = data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PartiallyEmittedExpression,
                    field: "expression",
                })?;
        }
        wrappers
            .into_iter()
            .rev()
            .try_fold(replacement, |inner, wrapper| {
                let data = tsc_syntax::nodes::PartiallyEmittedExpressionData {
                    expression: Some(inner.node()),
                };
                self.update_without_visit(wrapper, NodeData::PartiallyEmittedExpression(data))
                    .map(|node| self.node(node))
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
                Some(self.allocate_local_binding()?)
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
        if !root && is_function_scope_kind(record.kind) {
            return Ok(false);
        }
        match self.pass {
            TargetPass::Es2020 => {
                if self.es2020_node_requires_hoisted_temp(node, &record)? {
                    return Ok(true);
                }
            }
            TargetPass::Es2021 => {
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
            }
            TargetPass::Es2019 => {}
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

    fn es2020_node_requires_hoisted_temp(
        &self,
        node: TransformNode,
        record: &tsc_syntax::Node,
    ) -> Result<bool, TransformError> {
        if self.is_optional_chain(node)? {
            return self.optional_chain_requires_hoisted_temp(node, false);
        }
        match &record.data {
            NodeData::BinaryExpression(data) => {
                let operator = data
                    .operator_token
                    .map(|operator| {
                        self.context
                            .arena()
                            .node(self.node(operator))
                            .map(|operator| operator.kind)
                    })
                    .transpose()?;
                if operator == Some(SyntaxKind::QuestionQuestionToken) {
                    let left = data.left.map(|left| self.node(left)).ok_or(
                        TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::BinaryExpression,
                            field: "left",
                        },
                    )?;
                    return Ok(!self.is_simple_copiable_expression(left)?);
                }
            }
            NodeData::CallExpression(data) => {
                let Some(callee) = data.expression.map(|callee| self.node(callee)) else {
                    return Ok(false);
                };
                if self.context.arena().node(callee)?.kind == SyntaxKind::ParenthesizedExpression {
                    let inner = self.skip_parentheses(callee)?;
                    if self.is_optional_chain(inner)? {
                        return self.optional_chain_requires_hoisted_temp(inner, true);
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn optional_chain_requires_hoisted_temp(
        &self,
        chain: TransformNode,
        capture_receiver: bool,
    ) -> Result<bool, TransformError> {
        let plan = self.flatten_optional_chain(chain)?;
        let base = self.skip_partially_emitted_expressions(plan.base)?;
        if !self.is_simple_copiable_expression(base)? {
            return Ok(true);
        }
        Ok(capture_receiver
            && plan.segments.len() > 1
            && matches!(
                plan.segments.last(),
                Some(OptionalChainSegment::Property { .. } | OptionalChainSegment::Element { .. })
            ))
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
                *parameter = self.lower_parameter_default(*parameter, alias.as_ref())?;
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
        binding_alias: Option<&TargetBinding>,
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
                let condition_name = self.create_generated_identifier(alias)?;
                let condition = self.create_strict_undefined_check(condition_name)?;
                let fallback_name = self.create_generated_identifier(alias)?;
                self.create_conditional(condition, initializer, fallback_name)?
            } else {
                self.create_generated_identifier(alias)?
            };
            let declaration = self.create_variable_declaration(name, Some(value))?;
            let statement = self.create_variable_statement(vec![declaration])?;
            self.context.add_initialization_statement(statement)?;
            let alias_name = self.create_generated_identifier(alias)?;
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

    fn allocate_hoisted_temp(&mut self) -> Result<TargetBinding, TransformError> {
        let binding =
            TargetBinding::allocate(self.context, self.generated_bindings.allocate_temp())?;
        let declaration = self.create_generated_identifier(&binding)?;
        self.context.hoist_variable_declaration(declaration)?;
        Ok(binding)
    }

    fn allocate_local_binding(&mut self) -> Result<TargetBinding, TransformError> {
        TargetBinding::allocate(self.context, self.generated_bindings.allocate_local_temp())
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

    fn is_optional_chain(&self, expression: TransformNode) -> Result<bool, TransformError> {
        let record = self.context.arena().node(expression)?;
        Ok(
            NodeFlags::from_bits(record.flags).contains(NodeFlags::OPTIONAL_CHAIN)
                && matches!(
                    record.kind,
                    SyntaxKind::PropertyAccessExpression
                        | SyntaxKind::ElementAccessExpression
                        | SyntaxKind::CallExpression
                        | SyntaxKind::NonNullExpression
                ),
        )
    }

    fn requires_left_side_parentheses(
        &self,
        mut expression: TransformNode,
    ) -> Result<bool, TransformError> {
        loop {
            let record = self.context.arena().node(expression)?;
            if let NodeData::PartiallyEmittedExpression(data) = &record.data {
                expression = data
                    .expression
                    .map(|expression| self.node(expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PartiallyEmittedExpression,
                        field: "expression",
                    })?;
                continue;
            }
            return Ok(!matches!(
                record.kind,
                SyntaxKind::ArrayLiteralExpression
                    | SyntaxKind::ObjectLiteralExpression
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::Identifier
                    | SyntaxKind::StringLiteral
                    | SyntaxKind::NumericLiteral
                    | SyntaxKind::BigIntLiteral
                    | SyntaxKind::RegularExpressionLiteral
                    | SyntaxKind::NoSubstitutionTemplateLiteral
                    | SyntaxKind::ThisKeyword
                    | SyntaxKind::SuperKeyword
                    | SyntaxKind::ParenthesizedExpression
                    | SyntaxKind::PropertyAccessExpression
                    | SyntaxKind::ElementAccessExpression
                    | SyntaxKind::CallExpression
                    | SyntaxKind::NewExpression
                    | SyntaxKind::TaggedTemplateExpression
                    | SyntaxKind::MetaProperty
            ));
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
        self.context
            .arena_mut()?
            .metadata_mut(identifier)
            .set_generated_binding_id(binding.id());
        if let Some(base) = binding.numbered_base() {
            self.context
                .arena_mut()?
                .metadata_mut(identifier)
                .set_generated_binding_base(base);
        }
        if let Some(base) = binding.preferred_base() {
            self.context
                .arena_mut()?
                .metadata_mut(identifier)
                .set_generated_binding_preferred_base(base);
        }
        if binding.reserve_in_nested_scopes() {
            self.context
                .arena_mut()?
                .metadata_mut(identifier)
                .reserve_generated_binding_in_nested_scopes();
        }
        Ok(identifier)
    }

    fn create_property_access(
        &mut self,
        expression: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .arena_mut()?
            .metadata_mut(name)
            .add_flags(EmitFlags::NO_SUBSTITUTION);
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

    fn create_call(
        &mut self,
        expression: TransformNode,
        arguments: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let mut flags = self.child_flags(&[expression])?;
        if let Some(arguments) = arguments {
            flags |= self
                .context
                .arena()
                .array_transform_flags(self.array(arguments));
        }
        self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments,
            }),
            flags,
        )
    }

    fn create_function_call_call(
        &mut self,
        expression: TransformNode,
        receiver: TransformNode,
        arguments: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let call_name = self.create_identifier("call")?;
        let call_access = self.create_property_access(expression, call_name)?;
        let mut nodes = Vec::new();
        nodes.push(receiver);
        if let Some(arguments) = arguments {
            let arguments = self.context.arena().node_array(self.array(arguments))?;
            nodes.extend(arguments.nodes.iter().map(|argument| self.node(*argument)));
        }
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, nodes)?;
        self.create_call(call_access, Some(arguments.array()))
    }

    fn create_delete(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[expression])?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::DeleteExpression(tsc_syntax::nodes::DeleteExpressionData {
                expression: Some(expression.node()),
            }),
            flags,
        )
    }

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let right = if self.assignment_rhs_requires_parentheses(right)? {
            self.create_parenthesized(right)?
        } else {
            right
        };
        self.create_binary(left, SyntaxKind::EqualsToken, right)
    }

    /// Apply the observable part of tsc's assignment-RHS parenthesizer at the
    /// target-transform construction boundary. In particular, the TypeScript
    /// pass retains an `ExpressionWithTypeArguments` node after erasing its
    /// type arguments so later passes can preserve the grammar boundary as
    /// `(expression)` instead of silently treating it as a plain identifier.
    fn assignment_rhs_requires_parentheses(
        &self,
        mut expression: TransformNode,
    ) -> Result<bool, TransformError> {
        loop {
            let record = self.context.arena().node(expression)?;
            match &record.data {
                NodeData::PartiallyEmittedExpression(data) => {
                    expression = data
                        .expression
                        .map(|expression| self.node(expression))
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PartiallyEmittedExpression,
                            field: "assignment operand",
                        })?;
                }
                NodeData::ParenthesizedExpression(_) => return Ok(false),
                NodeData::ExpressionWithTypeArguments(_) | NodeData::CommaListExpression(_) => {
                    return Ok(true);
                }
                NodeData::BinaryExpression(data) => {
                    return Ok(data
                        .operator_token
                        .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                        .is_some_and(|operator| {
                            self.context
                                .arena()
                                .node(operator)
                                .is_ok_and(|operator| operator.kind == SyntaxKind::CommaToken)
                        }));
                }
                _ => return Ok(false),
            }
        }
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

    fn create_not_null_condition(
        &mut self,
        left: TransformNode,
        right: TransformNode,
        invert: bool,
    ) -> Result<TransformNode, TransformError> {
        let null = self.create_null()?;
        let left = self.create_binary(
            left,
            if invert {
                SyntaxKind::EqualsEqualsEqualsToken
            } else {
                SyntaxKind::ExclamationEqualsEqualsToken
            },
            null,
        )?;
        let undefined = self.create_void_zero()?;
        let right = self.create_binary(
            right,
            if invert {
                SyntaxKind::EqualsEqualsEqualsToken
            } else {
                SyntaxKind::ExclamationEqualsEqualsToken
            },
            undefined,
        )?;
        self.create_binary(
            left,
            if invert {
                SyntaxKind::BarBarToken
            } else {
                SyntaxKind::AmpersandAmpersandToken
            },
            right,
        )
    }

    fn create_null(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::NullKeyword,
            TransformFlags::NONE,
        )
    }

    fn create_true(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::TrueKeyword,
            TransformFlags::NONE,
        )
    }

    fn create_this(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
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

impl NodeDataChildVisitor for TargetVisitor<'_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("target-pass child belongs to the current transform source")
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

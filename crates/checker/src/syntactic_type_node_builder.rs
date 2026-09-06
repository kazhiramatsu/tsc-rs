#![allow(dead_code)]

use tsc_binder::node_util;
use tsc_emitter::{
    CommentRange, EmitFlags, EmitNodeBuilderFlags, EmitResolverError, EmitResolverMethod,
    SourceRange, TransformArena, TransformError, TransformFlags, TransformNode, TransformNodeArray,
    TransformSourceId,
};
use tsc_syntax::nodes::{
    ArrayTypeData, ConstructorTypeData, FunctionTypeData, IdentifierData, IndexSignatureData,
    LiteralTypeData, MethodSignatureData, NumericLiteralData, ParameterData,
    PrefixUnaryExpressionData, PropertySignatureData, StringLiteralData, TupleTypeData,
    TypeLiteralData, UnionTypeData,
};
use tsc_syntax::{
    is_identifier_text_for_target, try_visit_each_child, NodeArrayId, NodeData,
    NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

use crate::evaluate::EvalValue;
use crate::node_builder::{
    no_inference_fallback_is_set, restore_no_inference_fallback, save_no_inference_fallback,
    NodeBuilderContext, SyntacticAccessorDeclarations, SyntacticBuilderResolver,
    SyntacticRecoveryBoundary, SyntacticSymbol,
};

const IN_OBJECT_TYPE_LITERAL: u32 = 4_194_304;
const USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE: u32 = 268_435_456;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SyntacticResult {
    r#type: Option<TransformNode>,
    report_fallback: bool,
}

impl SyntacticResult {
    const fn new(r#type: Option<TransformNode>, report_fallback: bool) -> Self {
        Self {
            r#type,
            report_fallback,
        }
    }

    const fn syntactic(r#type: Option<TransformNode>) -> Self {
        Self::new(r#type, true)
    }

    const fn failed() -> Self {
        Self::new(None, true)
    }

    const fn already_reported() -> Self {
        Self::new(None, false)
    }

    const fn not_implemented() -> Self {
        Self::new(None, false)
    }
}

/// Captured, immutable option state for the dormant syntactic type-node
/// builder.
///
/// tsc-port: createSyntacticTypeNodeBuilder @6.0.3
/// tsc-hash: c98a5407512036e20afdd848d82d832e09b2f728a3ed25f4423136356387e2c9
/// tsc-span: _tsc.js:133276-134447
pub(crate) struct SyntacticTypeNodeBuilder {
    strict_null_checks: bool,
    script_target: ScriptTarget,
}

impl SyntacticTypeNodeBuilder {
    /// tsrs-native: Rust constructor for the ported machinery.
    pub(crate) fn new(options: &CompilerOptions) -> Self {
        Self {
            strict_null_checks: options.strict_option_value(options.strict_null_checks),
            script_target: options.emit_script_target(),
        }
    }

    /// tsrs-native: syntactic front-door wrapper (session dispatch).
    pub(crate) fn try_reuse_existing_type_node(
        &self,
        resolver: &mut dyn SyntacticBuilderResolver,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        existing: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let mut session = SyntacticBuildSession::new(
            self,
            resolver,
            arena,
            target,
            context,
            EmitResolverMethod::CreateTypeOfDeclaration,
        );
        if !session
            .resolver
            .can_reuse_type_node(session.arena, session.context, existing)?
        {
            return Ok(None);
        }
        session.try_reuse_existing_type_node(existing)
    }

    /// tsrs-native: syntactic front-door wrapper (probe-observable boundary, h2-7a-m-3 §6.2).
    pub(crate) fn serialize_type_of_declaration(
        &self,
        resolver: &mut dyn SyntacticBuilderResolver,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        crate::node_builder::replay_sink::enter_syntactic_frame();
        let result = SyntacticBuildSession::new(
            self,
            resolver,
            arena,
            target,
            context,
            EmitResolverMethod::CreateTypeOfDeclaration,
        )
        .serialize_type_of_declaration(node, symbol);
        let produced = match &result {
            Ok(Some(node)) => crate::node_builder::transform_node_class(arena, *node),
            Ok(None) | Err(_) => crate::node_builder::replay_sink::ProducedClass::Absent,
        };
        crate::node_builder::replay_sink::exit_syntactic_frame(
            "syntactic.serializeTypeOfDeclaration",
            produced,
        );
        result
    }

    /// tsrs-native: syntactic front-door wrapper (probe-observable boundary, h2-7a-m-3 §6.2).
    pub(crate) fn serialize_return_type_for_signature(
        &self,
        resolver: &mut dyn SyntacticBuilderResolver,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        crate::node_builder::replay_sink::enter_syntactic_frame();
        let result = SyntacticBuildSession::new(
            self,
            resolver,
            arena,
            target,
            context,
            EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
        )
        .serialize_return_type_for_signature(node, symbol);
        let produced = match &result {
            Ok(Some(node)) => crate::node_builder::transform_node_class(arena, *node),
            Ok(None) | Err(_) => crate::node_builder::replay_sink::ProducedClass::Absent,
        };
        crate::node_builder::replay_sink::exit_syntactic_frame(
            "syntactic.serializeReturnTypeForSignature",
            produced,
        );
        result
    }

    /// tsrs-native: syntactic front-door wrapper (session dispatch).
    pub(crate) fn serialize_type_of_expression(
        &self,
        resolver: &mut dyn SyntacticBuilderResolver,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        expression: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        SyntacticBuildSession::new(
            self,
            resolver,
            arena,
            target,
            context,
            EmitResolverMethod::CreateTypeOfExpression,
        )
        .serialize_type_of_expression(expression, false, false)
        .map(Some)
    }

    /// tsrs-native: syntactic front-door wrapper (session dispatch).
    pub(crate) fn serialize_type_of_accessor(
        &self,
        resolver: &mut dyn SyntacticBuilderResolver,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        accessor: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        SyntacticBuildSession::new(
            self,
            resolver,
            arena,
            target,
            context,
            EmitResolverMethod::CreateTypeOfDeclaration,
        )
        .serialize_type_of_accessor(accessor, symbol)
        .map(Some)
    }
}

struct SyntacticBuildSession<'a, 'tracker> {
    builder: &'a SyntacticTypeNodeBuilder,
    resolver: &'a mut dyn SyntacticBuilderResolver,
    arena: &'a mut TransformArena,
    target: TransformSourceId,
    context: &'a mut NodeBuilderContext<'tracker>,
    method: EmitResolverMethod,
    recovery_boundaries: Vec<SyntacticRecoveryBoundary>,
    visit_sources: Vec<TransformSourceId>,
}

impl<'a, 'tracker> SyntacticBuildSession<'a, 'tracker> {
    fn new(
        builder: &'a SyntacticTypeNodeBuilder,
        resolver: &'a mut dyn SyntacticBuilderResolver,
        arena: &'a mut TransformArena,
        target: TransformSourceId,
        context: &'a mut NodeBuilderContext<'tracker>,
        method: EmitResolverMethod,
    ) -> Self {
        Self {
            builder,
            resolver,
            arena,
            target,
            context,
            method,
            recovery_boundaries: Vec::new(),
            visit_sources: Vec::new(),
        }
    }

    fn factory_error(&self, error: TransformError) -> EmitResolverError {
        EmitResolverError::Factory {
            method: self.method,
            error: Box::new(error),
        }
    }

    fn node(&self, node: TransformNode) -> Result<&tsc_syntax::Node, EmitResolverError> {
        self.arena
            .node(node)
            .map_err(|error| self.factory_error(error))
    }

    fn kind(&self, node: TransformNode) -> Result<SyntaxKind, EmitResolverError> {
        Ok(self.node(node)?.kind)
    }

    fn child(&self, source: TransformSourceId, node: Option<NodeId>) -> Option<TransformNode> {
        node.and_then(|node| self.arena.node_ref(source, node))
    }

    fn array(
        &self,
        source: TransformSourceId,
        array: Option<NodeArrayId>,
    ) -> Option<TransformNodeArray> {
        array.and_then(|array| self.arena.node_array_ref(source, array))
    }

    fn nodes(
        &self,
        source: TransformSourceId,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, EmitResolverError> {
        let Some(array) = self.array(source, array) else {
            return Ok(Vec::new());
        };
        Ok(self
            .arena
            .node_array(array)
            .map_err(|error| self.factory_error(error))?
            .nodes
            .iter()
            .filter_map(|&node| self.arena.node_ref(source, node))
            .collect())
    }

    fn create_node(
        &mut self,
        source: TransformSourceId,
        data: NodeData,
        flags: TransformFlags,
    ) -> Result<TransformNode, EmitResolverError> {
        crate::node_builder::create_factory_node(self.arena, source, data, flags).map_err(|error| {
            match error {
                EmitResolverError::Factory { error, .. } => EmitResolverError::Factory {
                    method: self.method,
                    error,
                },
                other => other,
            }
        })
    }

    fn create_type_node(
        &mut self,
        source: TransformSourceId,
        data: NodeData,
    ) -> Result<TransformNode, EmitResolverError> {
        self.create_node(source, data, TransformFlags::CONTAINS_TYPE_SCRIPT)
    }

    fn create_token(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
        flags: TransformFlags,
    ) -> Result<TransformNode, EmitResolverError> {
        let mut factory = self.arena.factory();
        let result = match kind {
            SyntaxKind::NullKeyword => factory.create_null(source),
            SyntaxKind::TrueKeyword => factory.create_true(source),
            SyntaxKind::FalseKeyword => factory.create_false(source),
            SyntaxKind::ThisType => factory.create_this_type_node(source),
            SyntaxKind::NotEmittedTypeElement => factory.create_not_emitted_type_element(source),
            _ => factory.create_token(source, kind, flags),
        };
        result.map_err(|error| EmitResolverError::Factory {
            method: self.method,
            error: Box::new(error),
        })
    }

    fn create_keyword_type(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
    ) -> Result<TransformNode, EmitResolverError> {
        self.arena
            .factory()
            .create_keyword_type_node(source, kind)
            .map_err(|error| EmitResolverError::Factory {
                method: self.method,
                error: Box::new(error),
            })
    }

    fn create_node_array(
        &mut self,
        source: TransformSourceId,
        nodes: Vec<TransformNode>,
    ) -> Result<TransformNodeArray, EmitResolverError> {
        self.arena
            .factory()
            .create_node_array(source, nodes)
            .map_err(|error| EmitResolverError::Factory {
                method: self.method,
                error: Box::new(error),
            })
    }

    fn node_in_source(
        &mut self,
        source: TransformSourceId,
        node: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        if node.source() == source {
            return Ok(node);
        }
        self.arena
            .factory()
            .clone_node_to_source(node, source)
            .map_err(|error| self.factory_error(error))
    }

    fn create_identifier(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
    ) -> Result<TransformNode, EmitResolverError> {
        let text = text.into();
        self.create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(&text),
                text,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_string_literal(
        &mut self,
        source: TransformSourceId,
        text: impl Into<String>,
    ) -> Result<TransformNode, EmitResolverError> {
        self.create_node(
            source,
            NodeData::StringLiteral(StringLiteralData {
                text: text.into(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_numeric_literal(
        &mut self,
        source: TransformSourceId,
        value: f64,
    ) -> Result<TransformNode, EmitResolverError> {
        let magnitude = if value < 0.0 { -value } else { value };
        let text = if magnitude.fract() == 0.0 {
            format!("{magnitude:.0}")
        } else {
            magnitude.to_string()
        };
        let literal = self.create_node(
            source,
            NodeData::NumericLiteral(NumericLiteralData { text }),
            TransformFlags::NONE,
        )?;
        if value < 0.0 {
            self.create_node(
                source,
                NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                    operator: SyntaxKind::MinusToken,
                    operand: Some(literal.node()),
                }),
                TransformFlags::NONE,
            )
        } else {
            Ok(literal)
        }
    }

    fn create_literal_type(
        &mut self,
        source: TransformSourceId,
        literal: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        self.create_type_node(
            source,
            NodeData::LiteralType(LiteralTypeData {
                literal: Some(literal.node()),
            }),
        )
    }

    fn create_union_type(
        &mut self,
        source: TransformSourceId,
        types: Vec<TransformNode>,
    ) -> Result<TransformNode, EmitResolverError> {
        let types = self.create_node_array(source, types)?;
        self.create_type_node(
            source,
            NodeData::UnionType(UnionTypeData {
                types: Some(types.array()),
            }),
        )
    }

    fn create_array_type(
        &mut self,
        source: TransformSourceId,
        element_type: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        self.create_type_node(
            source,
            NodeData::ArrayType(ArrayTypeData {
                element_type: Some(element_type.node()),
            }),
        )
    }

    fn create_type_literal(
        &mut self,
        source: TransformSourceId,
        members: Vec<TransformNode>,
    ) -> Result<TransformNode, EmitResolverError> {
        let members = self.create_node_array(source, members)?;
        self.create_type_node(
            source,
            NodeData::TypeLiteral(TypeLiteralData {
                members: Some(members.array()),
            }),
        )
    }

    fn create_tuple_type(
        &mut self,
        source: TransformSourceId,
        elements: Vec<TransformNode>,
    ) -> Result<TransformNode, EmitResolverError> {
        let elements = self.create_node_array(source, elements)?;
        self.create_type_node(
            source,
            NodeData::TupleType(TupleTypeData {
                elements: Some(elements.array()),
            }),
        )
    }

    fn update_node(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, EmitResolverError> {
        crate::node_builder::update_factory_node(self.arena, original, data).map_err(|error| {
            match error {
                EmitResolverError::Factory { error, .. } => EmitResolverError::Factory {
                    method: self.method,
                    error,
                },
                other => other,
            }
        })
    }

    fn clone_node(&mut self, node: TransformNode) -> Result<TransformNode, EmitResolverError> {
        self.arena
            .factory()
            .clone_node(node)
            .map_err(|error| EmitResolverError::Factory {
                method: self.method,
                error: Box::new(error),
            })
    }

    /// tsc-port: reuseNode @6.0.3
    /// tsc-hash: a6bf75c4ca013ebaaf9e1df387d10227a43ae7e134bb797b61dedcdb2382dd09
    /// tsc-span: _tsc.js:133290-133292
    fn reuse_node(
        &mut self,
        node: Option<TransformNode>,
        range: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let Some(node) = node else {
            return Ok(None);
        };
        let synthesized =
            NodeFlags::from_bits(self.node(node)?.flags).intersects(NodeFlags::SYNTHESIZED);
        let candidate = if synthesized {
            node
        } else {
            self.clone_node(node)?
        };
        self.resolver
            .mark_node_reuse(self.arena, self.context, candidate, range.unwrap_or(node))
            .map(Some)
    }

    /// tsc-port: tryReuseExistingTypeNode @6.0.3
    /// tsc-hash: 73134f6715280a2e5ee712256fea004260db59f5797e30ce14d2d37f21a6d5a4
    /// tsc-span: _tsc.js:133293-133688
    fn try_reuse_existing_type_node(
        &mut self,
        existing: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        // h2-7b-m-2 fence amendment #4e: TypeScript reuses an existing
        // annotation from ANY file of its single node pool (the reused nodes
        // are cloned, and setTextRange copies no positions from another file).
        // The Rust arena keys child handles by source, so an annotation
        // living in another source is first cloned into the emitted target
        // (`clone_node_to_source`: synthesized, position-free, original-chain
        // kept) and the walk rebuilds the clone; same source = identity.
        // `approximateLength += existing.end - existing.pos` reads the parse
        // positions: a clone into the target is position-free, so follow the
        // original chain back to the positioned node first.
        let reused_span = {
            let positioned = self.arena.get_original_node(existing);
            let record = self.node(positioned)?;
            if record.pos == u32::MAX || record.end == u32::MAX {
                0
            } else {
                record.end.saturating_sub(record.pos)
            }
        };
        let existing = self.node_in_source(self.target, existing)?;
        let boundary = self
            .resolver
            .create_recovery_boundary(self.arena, self.context)?;
        self.recovery_boundaries.push(boundary);
        let transformed = self.visit_existing_node_tree_symbols(existing);
        let Some(boundary) = self.recovery_boundaries.pop() else {
            return Err(self.required_child_error(SyntaxKind::Unknown, "recoveryBoundary"));
        };
        let finalized = boundary.finalize(self.context, self.resolver)?;
        let transformed = transformed?;
        if !finalized {
            return Ok(None);
        }
        self.context.approximate_length =
            self.context.approximate_length.saturating_add(reused_span);
        Ok(transformed)
    }

    fn recovery_had_error(&self) -> bool {
        self.recovery_boundaries
            .last()
            .is_some_and(|boundary| boundary.had_error(self.context))
    }

    fn mark_recovery_error(&mut self) {
        if let Some(boundary) = self.recovery_boundaries.last_mut() {
            boundary.mark_error(self.context);
        }
    }

    /// tsc-port: visitExistingNodeTreeSymbols @6.0.3
    /// tsc-hash: edf2889cab0f535c29bb52a64b5baf6542a656d6e40571036991e81a3ad5c944
    /// tsc-span: _tsc.js:133301-133315
    fn visit_existing_node_tree_symbols(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if self.recovery_had_error() {
            return Ok(Some(node));
        }
        let Some(boundary) = self.recovery_boundaries.last() else {
            return Err(self.required_child_error(SyntaxKind::Unknown, "recoveryBoundary"));
        };
        let recover = boundary.start_recovery_scope(self.context);
        let cleanup = if self.is_new_scope_node(node)? {
            Some(
                self.resolver
                    .enter_new_scope(self.arena, self.target, self.context, node)?,
            )
        } else {
            None
        };
        self.visit_sources.push(node.source());
        let result = self.visit_existing_node_tree_symbols_worker(node);
        self.visit_sources.pop();
        if let Some(cleanup) = cleanup {
            cleanup.restore(self.context);
        }
        let result = result?;
        if self.recovery_had_error() {
            if self.is_type_node(node)? && self.kind(node)? != SyntaxKind::TypePredicate {
                let Some(boundary) = self.recovery_boundaries.last_mut() else {
                    return Err(self.required_child_error(SyntaxKind::Unknown, "recoveryBoundary"));
                };
                boundary.recover(self.context, recover);
                return self.resolver.serialize_existing_type_node(
                    self.arena,
                    self.target,
                    self.context,
                    node,
                    false,
                );
            }
            return Ok(Some(node));
        }
        match result {
            Some(result) => self
                .resolver
                .mark_node_reuse(self.arena, self.context, result, node)
                .map(Some),
            None => Ok(None),
        }
    }

    /// tsc-port: tryVisitSimpleTypeNode @6.0.3
    /// tsc-hash: a9055f86215bdbd7003f32be0b2dc25e3cf71b6913cbc4b97bb858033d233f1d
    /// tsc-span: _tsc.js:133316-133332
    fn try_visit_simple_type_node(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let inner = self.skip_type_parentheses(node)?;
        match self.kind(inner)? {
            SyntaxKind::TypeReference => self.try_visit_type_reference(inner),
            SyntaxKind::TypeQuery => self.try_visit_type_query(inner),
            SyntaxKind::IndexedAccessType => self.try_visit_indexed_access(inner),
            SyntaxKind::TypeOperator => {
                let NodeData::TypeOperator(data) = self.node(inner)?.data.clone() else {
                    return Ok(None);
                };
                if data.operator == SyntaxKind::KeyOfKeyword {
                    self.try_visit_key_of(inner)
                } else {
                    self.visit_existing_node_tree_symbols(node)
                }
            }
            _ => self.visit_existing_node_tree_symbols(node),
        }
    }

    /// tsc-port: tryVisitIndexedAccess @6.0.3
    /// tsc-hash: 4f284aaa5552320115ad8e5e2c2f6b1f482593d4015236e45dcdf533a3c779aa
    /// tsc-span: _tsc.js:133333-133339
    fn try_visit_indexed_access(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::IndexedAccessType(mut data) = self.node(node)?.data.clone() else {
            return Ok(None);
        };
        let Some(object) = self.child(node.source(), data.object_type) else {
            return Ok(None);
        };
        let Some(object) = self.try_visit_simple_type_node(object)? else {
            return Ok(None);
        };
        data.object_type = Some(self.node_in_source(node.source(), object)?.node());
        data.index_type = match self.child(node.source(), data.index_type) {
            Some(index) => self
                .visit_existing_node_tree_symbols(index)?
                .map(|index| self.node_in_source(node.source(), index))
                .transpose()?
                .map(TransformNode::node),
            None => None,
        };
        self.update_node(node, NodeData::IndexedAccessType(data))
            .map(Some)
    }

    /// tsc-port: tryVisitKeyOf @6.0.3
    /// tsc-hash: 1af02b6f4e1a58d7e4d91d75bd9c731db04ff875c42d90ae37aab672ece1db2f
    /// tsc-span: _tsc.js:133340-133347
    fn try_visit_key_of(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::TypeOperator(mut data) = self.node(node)?.data.clone() else {
            return Ok(None);
        };
        debug_assert_eq!(data.operator, SyntaxKind::KeyOfKeyword);
        let Some(inner) = self.child(node.source(), data.r#type) else {
            return Ok(None);
        };
        let Some(inner) = self.try_visit_simple_type_node(inner)? else {
            return Ok(None);
        };
        data.r#type = Some(self.node_in_source(node.source(), inner)?.node());
        self.update_node(node, NodeData::TypeOperator(data))
            .map(Some)
    }

    /// tsc-port: tryVisitTypeQuery @6.0.3
    /// tsc-hash: 0ea38917ef4438f9065f4c7f904e3df7be0a26dc60e934e6eed5519105b32ff3
    /// tsc-span: _tsc.js:133348-133366
    fn try_visit_type_query(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::TypeQuery(mut data) = self.node(node)?.data.clone() else {
            return Ok(None);
        };
        let Some(expr_name) = self.child(node.source(), data.expr_name) else {
            return Ok(None);
        };
        let tracked = self.resolver.track_existing_entity_name(
            self.arena,
            self.target,
            self.context,
            expr_name,
        )?;
        if !tracked.introduces_error {
            data.expr_name = Some(tracked.node.node());
            data.type_arguments =
                self.visit_optional_node_array(node.source(), data.type_arguments)?;
            return self.update_node(node, NodeData::TypeQuery(data)).map(Some);
        }
        let serialized = self.resolver.serialize_type_name(
            self.arena,
            self.target,
            self.context,
            expr_name,
            true,
            None,
        )?;
        match serialized {
            Some(serialized) => self
                .resolver
                .mark_node_reuse(self.arena, self.context, serialized, expr_name)
                .map(Some),
            None => Ok(None),
        }
    }

    /// tsc-port: tryVisitTypeReference @6.0.3
    /// tsc-hash: f20025033699cce2ad6ed94d59dde522079781878049462bbdd695b8f78694a8
    /// tsc-span: _tsc.js:133367-133391
    fn try_visit_type_reference(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if !self
            .resolver
            .can_reuse_type_node(self.arena, self.context, node)?
        {
            return Ok(None);
        }
        let NodeData::TypeReference(mut data) = self.node(node)?.data.clone() else {
            return Ok(None);
        };
        let Some(type_name) = self.child(node.source(), data.type_name) else {
            return Ok(None);
        };
        let tracked = self.resolver.track_existing_entity_name(
            self.arena,
            self.target,
            self.context,
            type_name,
        )?;
        data.type_arguments = self.visit_optional_node_array(node.source(), data.type_arguments)?;
        let type_arguments = self.array(node.source(), data.type_arguments);
        if !tracked.introduces_error {
            data.type_name = Some(tracked.node.node());
            let updated = self.update_node(node, NodeData::TypeReference(data))?;
            return self
                .resolver
                .mark_node_reuse(self.arena, self.context, updated, node)
                .map(Some);
        }
        let serialized = self.resolver.serialize_type_name(
            self.arena,
            self.target,
            self.context,
            type_name,
            false,
            type_arguments,
        )?;
        match serialized {
            Some(serialized) => self
                .resolver
                .mark_node_reuse(self.arena, self.context, serialized, type_name)
                .map(Some),
            None => Ok(None),
        }
    }

    /// tsc-port: visitExistingNodeTreeSymbolsWorker @6.0.3
    /// tsc-hash: aa5987f04f0db2a443801757c93a230d12009e42b0e26ac99c872d325dd70e19
    /// tsc-span: _tsc.js:133392-133687
    fn visit_existing_node_tree_symbols_worker(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let source = node.source();
        let data = self.node(node)?.data.clone();
        match data {
            NodeData::JSDocTypeExpression(data) => {
                return match self.child(source, data.r#type) {
                    Some(r#type) => self.visit_existing_node_tree_symbols(r#type),
                    None => Ok(None),
                };
            }
            NodeData::JSDocAllType(_) | NodeData::JSDocNamepathType(_) => {
                return self
                    .create_keyword_type(source, SyntaxKind::AnyKeyword)
                    .map(Some);
            }
            NodeData::JSDocUnknownType(_) => {
                return self
                    .create_keyword_type(source, SyntaxKind::UnknownKeyword)
                    .map(Some);
            }
            NodeData::JSDocNullableType(data) => {
                let Some(inner) = self.child(source, data.r#type) else {
                    return Ok(None);
                };
                let Some(inner) = self.visit_existing_node_tree_symbols(inner)? else {
                    return Ok(None);
                };
                let null =
                    self.create_token(source, SyntaxKind::NullKeyword, TransformFlags::NONE)?;
                let null = self.create_literal_type(source, null)?;
                return self.create_union_type(source, vec![inner, null]).map(Some);
            }
            NodeData::JSDocOptionalType(data) => {
                let Some(inner) = self.child(source, data.r#type) else {
                    return Ok(None);
                };
                let Some(inner) = self.visit_existing_node_tree_symbols(inner)? else {
                    return Ok(None);
                };
                let undefined = self.create_keyword_type(source, SyntaxKind::UndefinedKeyword)?;
                return self
                    .create_union_type(source, vec![inner, undefined])
                    .map(Some);
            }
            NodeData::JSDocNonNullableType(data) => {
                return match self.child(source, data.r#type) {
                    Some(r#type) => self.visit_existing_node_tree_symbols(r#type),
                    None => Ok(None),
                };
            }
            NodeData::JSDocVariadicType(data) => {
                let Some(inner) = self.child(source, data.r#type) else {
                    return Ok(None);
                };
                let Some(inner) = self.visit_existing_node_tree_symbols(inner)? else {
                    return Ok(None);
                };
                return self.create_array_type(source, inner).map(Some);
            }
            NodeData::JSDocTypeLiteral(data) => {
                let tags = self.nodes(source, data.js_doc_property_tags)?;
                let mut members = Vec::with_capacity(tags.len());
                for tag in tags {
                    // tsc-port: visitExistingNodeTreeSymbols (JSDocTypeLiteral) @6.0.3
                    // tsc-hash: 1cf444d81393bf59cab6555d09fa2234a853b231a9def4bf59ad00f2c8f2c323
                    // tsc-span: _tsc.js:133415-133426
                    let (tag_name, tag_type_expression, is_bracketed) =
                        match self.node(tag)?.data.clone() {
                            NodeData::JSDocPropertyTag(data) => {
                                (data.name, data.type_expression, data.is_bracketed)
                            }
                            NodeData::JSDocParameterTag(data) => {
                                (data.name, data.type_expression, data.is_bracketed)
                            }
                            _ => continue,
                        };
                    let Some(name) = self.child(source, tag_name) else {
                        continue;
                    };
                    let name = self.rightmost_name(name)?;
                    let Some(name) = self.visit_existing_node_tree_symbols(name)? else {
                        continue;
                    };
                    let override_type = self.resolver.get_js_doc_property_override(
                        self.arena,
                        self.target,
                        self.context,
                        node,
                        tag,
                    )?;
                    let type_expression = match self.child(source, tag_type_expression) {
                        Some(expression) => self.jsdoc_type_expression_type(expression)?,
                        None => None,
                    };
                    let optional = is_bracketed
                        || type_expression.is_some_and(|r#type| {
                            self.kind(r#type).ok() == Some(SyntaxKind::JSDocOptionalType)
                        });
                    let question = if optional {
                        Some(
                            self.create_token(
                                source,
                                SyntaxKind::QuestionToken,
                                TransformFlags::NONE,
                            )?
                            .node(),
                        )
                    } else {
                        None
                    };
                    let property_type = if let Some(override_type) = override_type {
                        override_type
                    } else if let Some(r#type) = type_expression {
                        match self.visit_existing_node_tree_symbols(r#type)? {
                            Some(r#type) => r#type,
                            None => self.create_keyword_type(source, SyntaxKind::AnyKeyword)?,
                        }
                    } else {
                        self.create_keyword_type(source, SyntaxKind::AnyKeyword)?
                    };
                    let property = self.create_type_node(
                        source,
                        NodeData::PropertySignature(PropertySignatureData {
                            name: Some(name.node()),
                            question_token: question,
                            modifiers: None,
                            r#type: Some(property_type.node()),
                            initializer: None,
                        }),
                    )?;
                    members.push(property);
                }
                return self.create_type_literal(source, members).map(Some);
            }
            _ => {}
        }

        if let NodeData::TypeReference(reference) = &data {
            if let Some(name) = self.child(source, reference.type_name) {
                if self.kind(name)? == SyntaxKind::Identifier
                    && self.identifier_text(name)?.is_empty()
                {
                    let any = self.create_keyword_type(source, SyntaxKind::AnyKeyword)?;
                    self.arena
                        .set_original_node(any, Some(node))
                        .map_err(|error| self.factory_error(error))?;
                    return Ok(Some(any));
                }
            }
        }

        if self.is_jsdoc_index_signature(node)? {
            let arguments = match &data {
                NodeData::TypeReference(reference) => reference.type_arguments,
                NodeData::ExpressionWithTypeArguments(reference) => reference.type_arguments,
                _ => None,
            };
            let arguments = self.nodes(source, arguments)?;
            if arguments.len() == 2 {
                let key = self
                    .visit_existing_node_tree_symbols(arguments[0])?
                    .unwrap_or(arguments[0]);
                let value = self
                    .visit_existing_node_tree_symbols(arguments[1])?
                    .unwrap_or(arguments[1]);
                let name = self.create_identifier(source, "x")?;
                let parameter = self.create_node(
                    source,
                    NodeData::Parameter(ParameterData {
                        name: Some(name.node()),
                        modifiers: None,
                        dot_dot_dot_token: None,
                        question_token: None,
                        r#type: Some(key.node()),
                        initializer: None,
                    }),
                    TransformFlags::CONTAINS_TYPE_SCRIPT,
                )?;
                let parameters = self.create_node_array(source, vec![parameter])?;
                let index = self.create_type_node(
                    source,
                    NodeData::IndexSignature(IndexSignatureData {
                        type_parameters: None,
                        parameters: Some(parameters.array()),
                        r#type: Some(value.node()),
                        modifiers: None,
                    }),
                )?;
                return self.create_type_literal(source, vec![index]).map(Some);
            }
        }

        if matches!(data, NodeData::JSDocFunctionType(_)) {
            return self.visit_jsdoc_function_type(node).map(Some);
        }

        if self.kind(node)? == SyntaxKind::ThisType {
            if self
                .resolver
                .can_reuse_type_node(self.arena, self.context, node)?
            {
                return Ok(Some(node));
            }
            self.mark_recovery_error();
            return Ok(Some(node));
        }

        if let NodeData::TypeParameter(mut parameter) = data.clone() {
            let Some(name) = self.child(source, parameter.name) else {
                return Ok(Some(node));
            };
            let tracked = self.resolver.track_existing_entity_name(
                self.arena,
                self.target,
                self.context,
                name,
            )?;
            parameter.name = Some(tracked.node.node());
            parameter.modifiers = self.visit_optional_node_array(source, parameter.modifiers)?;
            parameter.constraint = self.visit_optional_child(source, parameter.constraint)?;
            parameter.r#default = self.visit_optional_child(source, parameter.r#default)?;
            return self
                .update_node(node, NodeData::TypeParameter(parameter))
                .map(Some);
        }

        if matches!(data, NodeData::IndexedAccessType(_)) {
            if let Some(result) = self.try_visit_indexed_access(node)? {
                return Ok(Some(result));
            }
            self.mark_recovery_error();
            return Ok(Some(node));
        }

        if matches!(data, NodeData::TypeReference(_)) {
            if let Some(result) = self.try_visit_type_reference(node)? {
                return Ok(Some(result));
            }
            self.mark_recovery_error();
            return Ok(Some(node));
        }

        if let NodeData::ImportType(mut import_type) = data.clone() {
            let argument = self.child(source, import_type.argument);
            let literal = match argument {
                Some(argument) => self.literal_type_literal(argument)?,
                None => None,
            };
            if let (Some(argument), Some(literal)) = (argument, literal) {
                if self.kind(literal)? == SyntaxKind::StringLiteral {
                    if self.import_type_has_assert_attributes(source, &import_type)? {
                        self.mark_recovery_error();
                        return Ok(Some(node));
                    }
                    if !self
                        .resolver
                        .can_reuse_type_node(self.arena, self.context, node)?
                    {
                        return self.resolver.serialize_existing_type_node(
                            self.arena,
                            self.target,
                            self.context,
                            node,
                            false,
                        );
                    }
                    let specifier = self.rewrite_module_specifier_2(node, literal)?;
                    import_type.argument = if specifier == literal {
                        self.reuse_node(Some(argument), Some(argument))?
                            .map(TransformNode::node)
                    } else {
                        Some(self.create_literal_type(source, specifier)?.node())
                    };
                    import_type.attributes =
                        self.visit_optional_child(source, import_type.attributes)?;
                    import_type.qualifier =
                        self.visit_optional_child(source, import_type.qualifier)?;
                    import_type.type_arguments =
                        self.visit_optional_node_array(source, import_type.type_arguments)?;
                    return self
                        .update_node(node, NodeData::ImportType(import_type))
                        .map(Some);
                }
            }
        }

        if let Some(name) = self.name_of(node)? {
            if self.kind(name)? == SyntaxKind::ComputedPropertyName
                && !self.resolver.has_late_bindable_name(self.arena, node)?
            {
                if !self.has_dynamic_name(node)? {
                    return self.visit_each_child_2(node);
                }
                if self
                    .resolver
                    .should_remove_declaration(self.arena, self.context, node)?
                {
                    return Ok(None);
                }
            }
        }

        if self.needs_missing_type_any(node)? {
            let mut visited = self.visit_each_child_2(node)?.unwrap_or(node);
            if visited == node {
                let clone = self.clone_node(node)?;
                visited = self
                    .resolver
                    .mark_node_reuse(self.arena, self.context, clone, node)?;
            }
            let any = self.create_keyword_type(source, SyntaxKind::AnyKeyword)?;
            let data = self.with_declaration_type(visited, Some(any.node()), true)?;
            return self.update_node(visited, data).map(Some);
        }

        if matches!(data, NodeData::TypeQuery(_)) {
            if let Some(result) = self.try_visit_type_query(node)? {
                return Ok(Some(result));
            }
            self.mark_recovery_error();
            return Ok(Some(node));
        }

        if let NodeData::ComputedPropertyName(mut computed) = data.clone() {
            if let Some(expression) = self.child(source, computed.expression) {
                if self.is_entity_name_expression(expression)? {
                    let tracked = self.resolver.track_existing_entity_name(
                        self.arena,
                        self.target,
                        self.context,
                        expression,
                    )?;
                    if !tracked.introduces_error {
                        computed.expression = Some(tracked.node.node());
                        return self
                            .update_node(node, NodeData::ComputedPropertyName(computed))
                            .map(Some);
                    }
                    let computed_type = self.resolver.serialize_type_of_expression(
                        self.arena,
                        self.target,
                        self.context,
                        expression,
                    )?;
                    let mut literal = match computed_type {
                        Some(r#type) => self.literal_type_literal(r#type)?,
                        None => None,
                    };
                    if literal.is_none() {
                        let evaluated = self
                            .resolver
                            .evaluate_entity_name_expression(self.arena, expression)?;
                        literal = match evaluated.value {
                            Some(EvalValue::Str(value)) => {
                                Some(self.create_string_literal(source, value)?)
                            }
                            Some(EvalValue::Num(value)) => {
                                Some(self.create_numeric_literal(source, value)?)
                            }
                            None => None,
                        };
                        if literal.is_none() {
                            if match computed_type {
                                Some(r#type) => self.kind(r#type)? == SyntaxKind::ImportType,
                                None => false,
                            } {
                                self.resolver.track_computed_name(
                                    self.arena,
                                    self.context,
                                    expression,
                                )?;
                            }
                            return Ok(Some(node));
                        }
                    }
                    let Some(literal) = literal else {
                        return Ok(Some(node));
                    };
                    if self.kind(literal)? == SyntaxKind::StringLiteral {
                        let text = self.literal_text(literal)?.to_owned();
                        if is_identifier_text_for_target(&text, self.builder.script_target) {
                            return self.create_identifier(source, text).map(Some);
                        }
                    }
                    if self.kind(literal)? == SyntaxKind::NumericLiteral
                        && !self.literal_text(literal)?.starts_with('-')
                    {
                        return Ok(Some(literal));
                    }
                    computed.expression = Some(literal.node());
                    return self
                        .update_node(node, NodeData::ComputedPropertyName(computed))
                        .map(Some);
                }
            }
        }

        if let NodeData::TypePredicate(mut predicate) = data.clone() {
            if let Some(parameter_name) = self.child(source, predicate.parameter_name) {
                if self.kind(parameter_name)? == SyntaxKind::Identifier {
                    let tracked = self.resolver.track_existing_entity_name(
                        self.arena,
                        self.target,
                        self.context,
                        parameter_name,
                    )?;
                    if tracked.introduces_error {
                        self.mark_recovery_error();
                    }
                    predicate.parameter_name = Some(tracked.node.node());
                } else {
                    predicate.parameter_name = Some(self.clone_node(parameter_name)?.node());
                }
            }
            predicate.asserts_modifier = match self.child(source, predicate.asserts_modifier) {
                Some(asserts) => Some(self.clone_node(asserts)?.node()),
                None => None,
            };
            predicate.r#type = self.visit_optional_child(source, predicate.r#type)?;
            return self
                .update_node(node, NodeData::TypePredicate(predicate))
                .map(Some);
        }

        if matches!(
            data,
            NodeData::TupleType(_) | NodeData::TypeLiteral(_) | NodeData::MappedType(_)
        ) {
            let visited = self.visit_each_child_2(node)?.unwrap_or(node);
            let reusable = if visited == node {
                self.clone_node(node)?
            } else {
                visited
            };
            let clone = self
                .resolver
                .mark_node_reuse(self.arena, self.context, reusable, node)?;
            let keep_multiline = self
                .context
                .flags
                .contains(EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)
                && matches!(data, NodeData::TypeLiteral(_));
            if !keep_multiline {
                self.arena
                    .metadata_mut(clone)
                    .add_flags(EmitFlags::SINGLE_LINE);
            }
            return Ok(Some(clone));
        }

        if matches!(data, NodeData::StringLiteral(_))
            && self.context.flags.0 & USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE != 0
            && !self
                .arena
                .metadata(node)
                .and_then(tsc_emitter::EmitMetadata::string_literal_single_quote)
                .unwrap_or(false)
        {
            let clone = self.clone_node(node)?;
            self.arena
                .metadata_mut(clone)
                .set_string_literal_single_quote(true);
            return Ok(Some(clone));
        }

        if let NodeData::ConditionalType(mut conditional) = data.clone() {
            conditional.check_type = self.visit_optional_child(source, conditional.check_type)?;
            let cleanup =
                self.resolver
                    .enter_new_scope(self.arena, self.target, self.context, node)?;
            let scoped = (|| {
                conditional.extends_type =
                    self.visit_optional_child(source, conditional.extends_type)?;
                conditional.true_type = self.visit_optional_child(source, conditional.true_type)?;
                Ok::<(), EmitResolverError>(())
            })();
            cleanup.restore(self.context);
            scoped?;
            conditional.false_type = self.visit_optional_child(source, conditional.false_type)?;
            return self
                .update_node(node, NodeData::ConditionalType(conditional))
                .map(Some);
        }

        if let NodeData::TypeOperator(operator) = &data {
            if operator.operator == SyntaxKind::UniqueKeyword
                && self
                    .child(source, operator.r#type)
                    .is_some_and(|inner| self.kind(inner).ok() == Some(SyntaxKind::SymbolKeyword))
            {
                if !self
                    .resolver
                    .can_reuse_type_node(self.arena, self.context, node)?
                {
                    self.mark_recovery_error();
                    return Ok(Some(node));
                }
            } else if operator.operator == SyntaxKind::KeyOfKeyword {
                if let Some(result) = self.try_visit_key_of(node)? {
                    return Ok(Some(result));
                }
                self.mark_recovery_error();
                return Ok(Some(node));
            }
        }
        self.visit_each_child_2(node)
    }

    /// tsc-port: visitEachChild2 @6.0.3
    /// tsc-hash: c54d9f7a056cbed8b087378aa5e53dfbc19ecea978d92feb835d273153b806c9
    /// tsc-span: _tsc.js:133655-133664
    fn visit_each_child_2(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let mut data = self.node(node)?.data.clone();
        self.visit_sources.push(node.source());
        let result = try_visit_each_child(&mut data, self);
        self.visit_sources.pop();
        result?;
        self.update_node(node, data).map(Some)
    }

    /// tsc-port: visitNodesWithoutCopyingPositions @6.0.3
    /// tsc-hash: 6d51e27430eb6333a1eabb23b9d7bb9c65bf4fe34935c8f4b5cade667ecb2747
    /// tsc-span: _tsc.js:133665-133676
    fn visit_nodes_without_copying_positions(
        &mut self,
        source: TransformSourceId,
        original: TransformNodeArray,
        visited: TransformNodeArray,
    ) -> Result<TransformNodeArray, EmitResolverError> {
        let record = self
            .arena
            .node_array(visited)
            .map_err(|error| self.factory_error(error))?;
        if record.pos == u32::MAX && record.end == u32::MAX {
            return Ok(visited);
        }
        let result = if visited == original {
            let nodes = self
                .arena
                .node_array(original)
                .map_err(|error| self.factory_error(error))?
                .nodes
                .iter()
                .filter_map(|&node| self.arena.node_ref(source, node))
                .collect();
            self.create_node_array(source, nodes)?
        } else {
            visited
        };
        self.arena
            .factory()
            .set_node_array_text_range(result, u32::MAX, u32::MAX)
            .map_err(|error| EmitResolverError::Factory {
                method: self.method,
                error: Box::new(error),
            })?;
        Ok(result)
    }

    /// tsc-port: getEffectiveDotDotDotForParameter @6.0.3
    /// tsc-hash: 1e89a25207fb24356a79eef565ef5510065875274a54d444ffdafe1f942935f3
    /// tsc-span: _tsc.js:133677-133679
    fn get_effective_dot_dot_dot_for_parameter(
        &mut self,
        parameter: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::Parameter(data) = self.node(parameter)?.data.clone() else {
            return Ok(None);
        };
        if let Some(token) = self.child(parameter.source(), data.dot_dot_dot_token) {
            return Ok(Some(token));
        }
        if self
            .child(parameter.source(), data.r#type)
            .is_some_and(|r#type| self.kind(r#type).ok() == Some(SyntaxKind::JSDocVariadicType))
        {
            return self
                .create_token(
                    parameter.source(),
                    SyntaxKind::DotDotDotToken,
                    TransformFlags::NONE,
                )
                .map(Some);
        }
        Ok(None)
    }

    /// tsc-port: getNameForJSDocFunctionParameter @6.0.3
    /// tsc-hash: c1caebed3311a56bc008d46ee0ce09d4b1ab13ecf564922d0ff42193d985205a
    /// tsc-span: _tsc.js:133680-133682
    fn get_name_for_jsdoc_function_parameter(
        &mut self,
        parameter: TransformNode,
        index: usize,
    ) -> Result<String, EmitResolverError> {
        let NodeData::Parameter(data) = self.node(parameter)?.data.clone() else {
            return Ok(format!("arg{index}"));
        };
        if let Some(name) = self.child(parameter.source(), data.name) {
            if self.kind(name)? == SyntaxKind::Identifier && self.identifier_text(name)? == "this" {
                return Ok("this".to_owned());
            }
        }
        if self
            .get_effective_dot_dot_dot_for_parameter(parameter)?
            .is_some()
        {
            Ok("args".to_owned())
        } else {
            Ok(format!("arg{index}"))
        }
    }

    /// tsc-port: rewriteModuleSpecifier2 @6.0.3
    /// tsc-hash: 09a1ec41681f1905a35c53e3928202e526641593a8ec7138ff46424733fff787
    /// tsc-span: _tsc.js:133683-133686
    fn rewrite_module_specifier_2(
        &mut self,
        parent: TransformNode,
        literal: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        let Some(name) = self.resolver.get_module_specifier_override(
            self.arena,
            self.context,
            parent,
            literal,
        )?
        else {
            return Ok(literal);
        };
        let rewritten = self.create_string_literal(literal.source(), name)?;
        self.arena
            .set_original_node(rewritten, Some(literal))
            .map_err(|error| self.factory_error(error))?;
        Ok(rewritten)
    }

    /// tsc-port: serializeExistingTypeNode @6.0.3
    /// tsc-hash: 50784c39b4f9048f4548fa4273d0a8602e5da744c9eca1b6776d89a8399d6c28
    /// tsc-span: _tsc.js:133689-133705
    fn serialize_existing_type_node(
        &mut self,
        type_node: Option<TransformNode>,
        add_undefined: bool,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let Some(type_node) = type_node else {
            return Ok(None);
        };
        if (!add_undefined || self.can_add_undefined(type_node)?)
            && self
                .resolver
                .can_reuse_type_node(self.arena, self.context, type_node)?
        {
            if let Some(result) = self.try_reuse_existing_type_node(type_node)? {
                return self
                    .add_undefined_if_needed(result, add_undefined, None)
                    .map(Some);
            }
        }
        Ok(None)
    }

    /// tsc-port: serializeTypeAnnotationOfDeclaration @6.0.3
    /// tsc-hash: e23bbbb5fd3312ca518de15faf39bf09573dbe3a1e071fdc6640ff91ff3525e9
    /// tsc-span: _tsc.js:133706-133729
    fn serialize_type_annotation_of_declaration(
        &mut self,
        declared_type: Option<TransformNode>,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
        requires_adding_undefined: Option<bool>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let Some(declared_type) = declared_type else {
            return Ok(None);
        };
        let use_fallback = requires_adding_undefined.is_some();
        if !self.resolver.can_reuse_type_node_annotation(
            self.arena,
            self.context,
            node,
            declared_type,
            symbol,
            requires_adding_undefined,
        )? && (requires_adding_undefined != Some(true)
            || !self.resolver.can_reuse_type_node_annotation(
                self.arena,
                self.context,
                node,
                declared_type,
                symbol,
                Some(false),
            )?)
        {
            return Ok(None);
        }
        let add_undefined = requires_adding_undefined.unwrap_or(false);
        let result = if !add_undefined || self.can_add_undefined(declared_type)? {
            self.serialize_existing_type_node(Some(declared_type), add_undefined)?
        } else {
            None
        };
        if result.is_some() || !use_fallback {
            return Ok(result);
        }
        self.report_inference_fallback(node)?;
        let result = self.resolver.serialize_existing_type_node(
            self.arena,
            self.target,
            self.context,
            declared_type,
            add_undefined,
        )?;
        match result {
            Some(result) => Ok(Some(result)),
            None => self
                .create_keyword_type(node.source(), SyntaxKind::AnyKeyword)
                .map(Some),
        }
    }

    /// tsc-port: serializeExistingTypeNodeWithFallback @6.0.3
    /// tsc-hash: 0376999d33ff00abb168ab7a6195e54bc1dd3a9c02758c9304870d9cc11126fe
    /// tsc-span: _tsc.js:133730-133738
    fn serialize_existing_type_node_with_fallback(
        &mut self,
        type_node: Option<TransformNode>,
        add_undefined: bool,
        target_node: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let Some(type_node) = type_node else {
            return Ok(None);
        };
        if let Some(result) = self.serialize_existing_type_node(Some(type_node), add_undefined)? {
            return Ok(Some(result));
        }
        self.report_inference_fallback(target_node.unwrap_or(type_node))?;
        let result = self.resolver.serialize_existing_type_node(
            self.arena,
            self.target,
            self.context,
            type_node,
            add_undefined,
        )?;
        match result {
            Some(result) => Ok(Some(result)),
            None => self
                .create_keyword_type(type_node.source(), SyntaxKind::AnyKeyword)
                .map(Some),
        }
    }

    /// tsc-port: serializeTypeOfAccessor @6.0.3
    /// tsc-hash: 79c6c1089daccf2f075aad5141d26b071206a01a97cc6ddd1b6acb695ae321ea
    /// tsc-span: _tsc.js:133739-133741
    fn serialize_type_of_accessor(
        &mut self,
        accessor: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<TransformNode, EmitResolverError> {
        if let Some(result) = self.type_from_accessor(accessor, symbol)? {
            return Ok(result);
        }
        let accessors = self
            .resolver
            .get_all_accessor_declarations(self.arena, accessor)?;
        self.infer_accessor_type(accessor, accessors, symbol, true)
    }

    /// tsc-port: serializeTypeOfExpression @6.0.3
    /// tsc-hash: 463aa6379bbc0afeead57879d4c541c489f2859066fd2463e735f9ff5a95d61b
    /// tsc-span: _tsc.js:133742-133752
    fn serialize_type_of_expression(
        &mut self,
        expression: TransformNode,
        add_undefined: bool,
        preserve_literals: bool,
    ) -> Result<TransformNode, EmitResolverError> {
        let result =
            self.type_from_expression(expression, false, add_undefined, preserve_literals)?;
        match result.r#type {
            Some(result) => Ok(result),
            None => self.infer_expression_type(expression, result.report_fallback),
        }
    }

    /// tsc-port: serializeTypeOfDeclaration @6.0.3
    /// tsc-hash: 136df25c4224ca49b1f3b79f75b8938d83ccfa9f5a8f47bdb04124b7f2ca3383
    /// tsc-span: _tsc.js:133753-133785
    fn serialize_type_of_declaration(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let result = match self.kind(node)? {
            SyntaxKind::Parameter | SyntaxKind::JSDocParameterTag => {
                self.type_from_parameter(node, symbol)?
            }
            SyntaxKind::VariableDeclaration => self.type_from_variable(node, symbol)?,
            SyntaxKind::PropertySignature
            | SyntaxKind::JSDocPropertyTag
            | SyntaxKind::PropertyDeclaration => self.type_from_property(node, symbol)?,
            SyntaxKind::BindingElement => self.infer_type_of_declaration(node, symbol, true)?,
            SyntaxKind::ExportAssignment => {
                let NodeData::ExportAssignment(data) = self.node(node)?.data.clone() else {
                    return Ok(None);
                };
                let Some(expression) = self.child(node.source(), data.expression) else {
                    return Ok(None);
                };
                Some(self.serialize_type_of_expression(expression, false, true)?)
            }
            SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::BinaryExpression => self.type_from_expando_property(node, symbol)?,
            SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment => {
                self.type_from_property_assignment(node, symbol)?
            }
            _ => return Ok(None),
        };
        Ok(result)
    }

    /// tsc-port: typeFromPropertyAssignment @6.0.3
    /// tsc-hash: 5e6d2c936395ed6d627a9d73735c6a0e7b9f2b5cef2ec289ccdb9c99d8f4f9ea
    /// tsc-span: _tsc.js:133786-133806
    fn type_from_property_assignment(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let type_annotation = self.effective_type_annotation_node(node)?;
        let mut result = None;
        if let Some(type_annotation) = type_annotation {
            if self.resolver.can_reuse_type_node_annotation(
                self.arena,
                self.context,
                node,
                type_annotation,
                symbol,
                None,
            )? {
                result = self.serialize_existing_type_node(Some(type_annotation), false)?;
            }
        }
        if result.is_none() && self.kind(node)? == SyntaxKind::PropertyAssignment {
            let NodeData::PropertyAssignment(data) = self.node(node)?.data.clone() else {
                return self.infer_type_of_declaration(node, symbol, false);
            };
            if let Some(initializer) = self.child(node.source(), data.initializer) {
                let assertion = if self.is_jsdoc_type_assertion(initializer)? {
                    self.jsdoc_type_assertion_type(initializer)?
                } else {
                    match &self.node(initializer)?.data {
                        NodeData::AsExpression(data) => {
                            self.child(initializer.source(), data.r#type)
                        }
                        NodeData::TypeAssertionExpression(data) => {
                            self.child(initializer.source(), data.r#type)
                        }
                        _ => None,
                    }
                };
                if let Some(assertion) = assertion {
                    if !self.is_const_type_reference(assertion)?
                        && self.resolver.can_reuse_type_node_annotation(
                            self.arena,
                            self.context,
                            node,
                            assertion,
                            symbol,
                            None,
                        )?
                    {
                        result = self.serialize_existing_type_node(Some(assertion), false)?;
                    }
                }
            }
        }
        match result {
            Some(result) => Ok(Some(result)),
            None => self.infer_type_of_declaration(node, symbol, false),
        }
    }

    /// tsc-port: serializeReturnTypeForSignature @6.0.3
    /// tsc-hash: 392dd1cdbbe89fcce6955d0c3d92b5f8f79cb32596078ca3d747b39114d144f8
    /// tsc-span: _tsc.js:133807-133829
    fn serialize_return_type_for_signature(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let result = match self.kind(node)? {
            SyntaxKind::GetAccessor => self.serialize_type_of_accessor(node, symbol)?,
            SyntaxKind::MethodDeclaration
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ConstructSignature
            | SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::Constructor
            | SyntaxKind::SetAccessor
            | SyntaxKind::IndexSignature
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::JSDocFunctionType
            | SyntaxKind::JSDocSignature => {
                self.create_return_from_signature(node, symbol, true)?
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    /// tsc-port: getTypeAnnotationFromAccessor @6.0.3
    /// tsc-hash: b68d84b08e95c6749632a76792ba24de7ff7c28dcd157701ac642af0a4d1ddfe
    /// tsc-span: _tsc.js:133830-133834
    fn get_type_annotation_from_accessor(
        &self,
        accessor: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let Some(accessor) = accessor else {
            return Ok(None);
        };
        if self.kind(accessor)? == SyntaxKind::GetAccessor {
            if self.is_in_js_file(accessor)? {
                if let Some(r#type) = self.get_jsdoc_type(accessor)? {
                    return Ok(Some(r#type));
                }
            }
            self.effective_return_type_node(accessor)
        } else {
            let Some(parameter) = self.set_accessor_value_parameter(accessor)? else {
                return Ok(None);
            };
            self.effective_type_annotation_node(parameter)
        }
    }

    /// tsc-port: getTypeAnnotationFromAllAccessorDeclarations @6.0.3
    /// tsc-hash: 0280090f068b3fde58ce44fd23bf020a940a2c811f977d31dfe1f3fd39a4ca33
    /// tsc-span: _tsc.js:133835-133844
    fn get_type_annotation_from_all_accessor_declarations(
        &self,
        node: TransformNode,
        accessors: SyntacticAccessorDeclarations,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let mut accessor_type = self.get_type_annotation_from_accessor(Some(node))?;
        if accessor_type.is_none() && node != accessors.first_accessor {
            accessor_type =
                self.get_type_annotation_from_accessor(Some(accessors.first_accessor))?;
        }
        if accessor_type.is_none()
            && accessors
                .second_accessor
                .is_some_and(|second| node != second)
        {
            accessor_type = self.get_type_annotation_from_accessor(accessors.second_accessor)?;
        }
        Ok(accessor_type)
    }

    /// tsc-port: typeFromAccessor @6.0.3
    /// tsc-hash: 004eac44bbda96fc6084e550648b6bb9bc9fe6d52401fe495d3816b50eb026e1
    /// tsc-span: _tsc.js:133845-133855
    fn type_from_accessor(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let accessors = self
            .resolver
            .get_all_accessor_declarations(self.arena, node)?;
        let accessor_type =
            self.get_type_annotation_from_all_accessor_declarations(node, accessors)?;
        if let Some(accessor_type) = accessor_type {
            if self.kind(accessor_type)? != SyntaxKind::TypePredicate {
                return self.with_new_scope(node, |session| {
                    if let Some(result) = session.serialize_type_annotation_of_declaration(
                        Some(accessor_type),
                        node,
                        symbol,
                        None,
                    )? {
                        Ok(Some(result))
                    } else {
                        session.infer_type_of_declaration(node, symbol, true)
                    }
                });
            }
        }
        if let Some(getter) = accessors.get_accessor {
            return self.with_new_scope(getter, |session| {
                session
                    .create_return_from_signature(getter, symbol, true)
                    .map(Some)
            });
        }
        Ok(None)
    }

    /// tsc-port: typeFromVariable @6.0.3
    /// tsc-hash: 85f3798f047596dba19ca4c86bed1b1a67331da75e99c8105a4dcad1f35d0b12
    /// tsc-span: _tsc.js:133856-133876
    fn type_from_variable(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let declared_type = self.effective_type_annotation_node(node)?;
        let mut result = SyntacticResult::failed();
        if declared_type.is_some() {
            result = SyntacticResult::syntactic(self.serialize_type_annotation_of_declaration(
                declared_type,
                node,
                symbol,
                None,
            )?);
        } else if let Some(initializer) = self.initializer_of(node)? {
            let sole_variable = symbol.is_some_and(|symbol| {
                symbol.declaration_count == 1 || symbol.variable_declaration_count == 1
            });
            if sole_variable
                && !SyntacticBuilderResolver::is_expando_function_declaration(
                    &mut *self.resolver,
                    self.arena,
                    node,
                )?
                && !self.is_contextually_typed(node)?
            {
                result = self.type_from_expression(
                    initializer,
                    false,
                    false,
                    self.is_var_const_like(node)?,
                )?;
            }
        }
        match result.r#type {
            Some(result) => Ok(Some(result)),
            None => self.infer_type_of_declaration(node, symbol, result.report_fallback),
        }
    }

    /// tsc-port: typeFromParameter @6.0.3
    /// tsc-hash: 4c4e99d60e87cd04b1fad244bea00d2bd2044fca6edcb7cb4fd6064528483e58
    /// tsc-span: _tsc.js:133877-133902
    fn type_from_parameter(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if let Some(parent) = self.parent(node) {
            if self.kind(parent)? == SyntaxKind::SetAccessor {
                return self.serialize_type_of_accessor(parent, None).map(Some);
            }
        }
        let declared_type = self.effective_type_annotation_node(node)?;
        let add_undefined = SyntacticBuilderResolver::requires_adding_implicit_undefined(
            &mut *self.resolver,
            self.arena,
            node,
            symbol,
            self.context.enclosing_declaration,
        )?;
        let mut result = SyntacticResult::failed();
        if declared_type.is_some() {
            result = SyntacticResult::syntactic(self.serialize_type_annotation_of_declaration(
                declared_type,
                node,
                symbol,
                Some(add_undefined),
            )?);
        } else if self.kind(node)? == SyntaxKind::Parameter {
            if let Some(initializer) = self.initializer_of(node)? {
                let name_is_identifier = self
                    .name_of(node)?
                    .is_some_and(|name| self.kind(name).ok() == Some(SyntaxKind::Identifier));
                if name_is_identifier && !self.is_contextually_typed(node)? {
                    result = self.type_from_expression(initializer, false, add_undefined, false)?;
                }
            }
        }
        match result.r#type {
            Some(result) => Ok(Some(result)),
            None => self.infer_type_of_declaration(node, symbol, result.report_fallback),
        }
    }

    /// tsc-port: typeFromExpandoProperty @6.0.3
    /// tsc-hash: 8f044a256589e1b17aaadc603635ff4cdd552882249dd628f32d3f101e4444c0
    /// tsc-span: _tsc.js:133903-133920
    fn type_from_expando_property(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let declared_type = self.effective_type_annotation_node(node)?;
        let result = if declared_type.is_some() {
            self.serialize_type_annotation_of_declaration(declared_type, node, symbol, None)?
        } else {
            None
        };
        let old_suppress = self.context.suppress_report_inference_fallback;
        self.context.suppress_report_inference_fallback = true;
        let inferred = match result {
            Some(result) => Ok(Some(result)),
            None => self.infer_type_of_declaration(node, symbol, false),
        };
        self.context.suppress_report_inference_fallback = old_suppress;
        inferred
    }

    /// tsc-port: typeFromProperty @6.0.3
    /// tsc-hash: 4eac46c53ae9b80d9b3d0b1970fbb8472f94d05c38d2ca36e4af4c51725a1c82
    /// tsc-span: _tsc.js:133921-133942
    fn type_from_property(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let declared_type = self.effective_type_annotation_node(node)?;
        let add_undefined = SyntacticBuilderResolver::requires_adding_implicit_undefined(
            &mut *self.resolver,
            self.arena,
            node,
            symbol,
            self.context.enclosing_declaration,
        )?;
        let mut result = SyntacticResult::failed();
        if declared_type.is_some() {
            result = SyntacticResult::syntactic(self.serialize_type_annotation_of_declaration(
                declared_type,
                node,
                symbol,
                Some(add_undefined),
            )?);
        } else if self.kind(node)? == SyntaxKind::PropertyDeclaration {
            if let Some(initializer) = self.initializer_of(node)? {
                if !self.is_contextually_typed(node)? {
                    result = self.type_from_expression(
                        initializer,
                        false,
                        add_undefined,
                        self.is_declaration_readonly(node)?,
                    )?;
                }
            }
        }
        match result.r#type {
            Some(result) => Ok(Some(result)),
            None => self.infer_type_of_declaration(node, symbol, result.report_fallback),
        }
    }

    /// tsc-port: inferTypeOfDeclaration @6.0.3
    /// tsc-hash: 98ff825bbbe3bb9650a28f6cf95332f20957f56abc458fc1e2209d1df617e732
    /// tsc-span: _tsc.js:133943-133951
    fn infer_type_of_declaration(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
        report_fallback: bool,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        crate::node_builder::replay_sink::mark_syntactic_fallback(
            "syntactic.serializeTypeOfDeclaration",
            report_fallback,
        );
        if report_fallback {
            self.report_inference_fallback(node)?;
        }
        if no_inference_fallback_is_set(self.context) {
            return self
                .create_keyword_type(node.source(), SyntaxKind::AnyKeyword)
                .map(Some);
        }
        match self.resolver.serialize_type_of_declaration(
            self.arena,
            self.target,
            self.context,
            node,
            symbol,
        )? {
            Some(result) => Ok(Some(result)),
            None => Ok(None),
        }
    }

    /// tsc-port: inferExpressionType @6.0.3
    /// tsc-hash: 6d550e6503825ca4532050625aa0ce349748320b514da23c4e64aea51efa6ce9
    /// tsc-span: _tsc.js:133952-133961
    fn infer_expression_type(
        &mut self,
        node: TransformNode,
        report_fallback: bool,
    ) -> Result<TransformNode, EmitResolverError> {
        if report_fallback {
            self.report_inference_fallback(node)?;
        }
        if no_inference_fallback_is_set(self.context) {
            return self.create_keyword_type(node.source(), SyntaxKind::AnyKeyword);
        }
        match self.resolver.serialize_type_of_expression(
            self.arena,
            self.target,
            self.context,
            node,
        )? {
            Some(result) => Ok(result),
            None => self.create_keyword_type(node.source(), SyntaxKind::AnyKeyword),
        }
    }

    /// tsc-port: inferReturnTypeOfSignatureSignature @6.0.3
    /// tsc-hash: 0dc7d6a529bd4c9f52dbbd183a425c0b83ce8e4662657e1ff99f9d8c7d78f5e9
    /// tsc-span: _tsc.js:133962-133970
    fn infer_return_type_of_signature_signature(
        &mut self,
        node: TransformNode,
        symbol: Option<SyntacticSymbol>,
        report_fallback: bool,
    ) -> Result<TransformNode, EmitResolverError> {
        crate::node_builder::replay_sink::mark_syntactic_fallback(
            "syntactic.serializeReturnTypeForSignature",
            report_fallback,
        );
        if report_fallback {
            self.report_inference_fallback(node)?;
        }
        if no_inference_fallback_is_set(self.context) {
            return self.create_keyword_type(node.source(), SyntaxKind::AnyKeyword);
        }
        match self.resolver.serialize_return_type_for_signature(
            self.arena,
            self.target,
            self.context,
            node,
            symbol,
        )? {
            Some(result) => Ok(result),
            None => self.create_keyword_type(node.source(), SyntaxKind::AnyKeyword),
        }
    }

    /// tsc-port: inferAccessorType @6.0.3
    /// tsc-hash: d7be66eb7c08de0dc200d55e96667c21bd9a74bb47cd28721fc501be73ea1d56
    /// tsc-span: _tsc.js:133971-133981
    fn infer_accessor_type(
        &mut self,
        node: TransformNode,
        accessors: SyntacticAccessorDeclarations,
        symbol: Option<SyntacticSymbol>,
        report_fallback: bool,
    ) -> Result<TransformNode, EmitResolverError> {
        if self.kind(node)? == SyntaxKind::GetAccessor {
            return self.create_return_from_signature(node, symbol, report_fallback);
        }
        if report_fallback {
            self.report_inference_fallback(node)?;
        }
        if let Some(getter) = accessors.get_accessor {
            return self.create_return_from_signature(getter, symbol, report_fallback);
        }
        match self.resolver.serialize_type_of_declaration(
            self.arena,
            self.target,
            self.context,
            node,
            symbol,
        )? {
            Some(result) => Ok(result),
            None => self.create_keyword_type(node.source(), SyntaxKind::AnyKeyword),
        }
    }

    /// tsc-port: withNewScope @6.0.3
    /// tsc-hash: f34baf4436a3b8a4e85c5dad79311132bf0f394fef44ba85e5549bf1f72f3fb1
    /// tsc-span: _tsc.js:133982-133987
    fn with_new_scope<T>(
        &mut self,
        node: TransformNode,
        operation: impl FnOnce(&mut Self) -> Result<T, EmitResolverError>,
    ) -> Result<T, EmitResolverError> {
        let cleanup = self
            .resolver
            .enter_new_scope(self.arena, self.target, self.context, node)?;
        let result = operation(self);
        cleanup.restore(self.context);
        result
    }

    /// tsc-port: typeFromTypeAssertion @6.0.3
    /// tsc-hash: 35cfd4c12775e6d02a0ffe014b8eddb48abf9a7974d39eb816ec11e89a553af2
    /// tsc-span: _tsc.js:133988-133999
    fn type_from_type_assertion(
        &mut self,
        expression: TransformNode,
        r#type: TransformNode,
        requires_adding_undefined: bool,
    ) -> Result<SyntacticResult, EmitResolverError> {
        if self.is_const_type_reference(r#type)? {
            return self.type_from_expression(expression, true, requires_adding_undefined, false);
        }
        Ok(SyntacticResult::syntactic(
            self.serialize_existing_type_node_with_fallback(
                Some(r#type),
                requires_adding_undefined,
                None,
            )?,
        ))
    }

    /// tsc-port: typeFromExpression @6.0.3
    /// tsc-hash: 618dccd876b1fdcf9053f5bfabc74f50521767b59a4b2b3bb3e2f5b441562ca7
    /// tsc-span: _tsc.js:134000-134082
    fn type_from_expression(
        &mut self,
        node: TransformNode,
        is_const_context: bool,
        requires_adding_undefined: bool,
        preserve_literals: bool,
    ) -> Result<SyntacticResult, EmitResolverError> {
        // #4e: an initializer living in another source is cloned into the
        // emitted target before the syntactic inference walks it.
        let node = self.node_in_source(self.target, node)?;
        self.type_from_expression_in_source(
            node,
            is_const_context,
            requires_adding_undefined,
            preserve_literals,
        )
    }

    /// tsrs-native: `typeFromExpression`'s body on the target-source node (#4e).
    fn type_from_expression_in_source(
        &mut self,
        node: TransformNode,
        is_const_context: bool,
        requires_adding_undefined: bool,
        preserve_literals: bool,
    ) -> Result<SyntacticResult, EmitResolverError> {
        let source = node.source();
        match self.node(node)?.data.clone() {
            NodeData::ParenthesizedExpression(data) => {
                let Some(expression) = self.child(source, data.expression) else {
                    return Ok(SyntacticResult::failed());
                };
                if self.is_jsdoc_type_assertion(node)? {
                    if let Some(r#type) = self.jsdoc_type_assertion_type(node)? {
                        return self.type_from_type_assertion(
                            expression,
                            r#type,
                            requires_adding_undefined,
                        );
                    }
                }
                return self.type_from_expression(
                    expression,
                    is_const_context,
                    requires_adding_undefined,
                    false,
                );
            }
            NodeData::Identifier(_) => {
                if self
                    .resolver
                    .is_undefined_identifier_expression(self.arena, node)?
                {
                    return self
                        .create_undefined_type_node(source)
                        .map(|r#type| SyntacticResult::syntactic(Some(r#type)));
                }
            }
            NodeData::Token if self.kind(node)? == SyntaxKind::NullKeyword => {
                let r#type = if self.builder.strict_null_checks {
                    let null =
                        self.create_token(source, SyntaxKind::NullKeyword, TransformFlags::NONE)?;
                    let null = self.create_literal_type(source, null)?;
                    self.add_undefined_if_needed(null, requires_adding_undefined, Some(node))?
                } else {
                    self.create_keyword_type(source, SyntaxKind::AnyKeyword)?
                };
                return Ok(SyntacticResult::syntactic(Some(r#type)));
            }
            NodeData::ArrowFunction(_) | NodeData::FunctionExpression(_) => {
                return self.with_new_scope(node, |session| {
                    session.type_from_function_like_expression(node)
                });
            }
            NodeData::AsExpression(data) => {
                let Some(expression) = self.child(source, data.expression) else {
                    return Ok(SyntacticResult::failed());
                };
                let Some(r#type) = self.child(source, data.r#type) else {
                    return Ok(SyntacticResult::failed());
                };
                return self.type_from_type_assertion(
                    expression,
                    r#type,
                    requires_adding_undefined,
                );
            }
            NodeData::TypeAssertionExpression(data) => {
                let Some(expression) = self.child(source, data.expression) else {
                    return Ok(SyntacticResult::failed());
                };
                let Some(r#type) = self.child(source, data.r#type) else {
                    return Ok(SyntacticResult::failed());
                };
                return self.type_from_type_assertion(
                    expression,
                    r#type,
                    requires_adding_undefined,
                );
            }
            NodeData::PrefixUnaryExpression(data) => {
                if self.is_primitive_literal_value(node, true)? {
                    let Some(operand) = self.child(source, data.operand) else {
                        return Ok(SyntacticResult::failed());
                    };
                    let primitive = if data.operator == SyntaxKind::PlusToken {
                        operand
                    } else {
                        node
                    };
                    let base_type = if self.kind(operand)? == SyntaxKind::BigIntLiteral {
                        SyntaxKind::BigIntKeyword
                    } else {
                        SyntaxKind::NumberKeyword
                    };
                    return self.type_from_primitive_literal(
                        primitive,
                        base_type,
                        is_const_context || preserve_literals,
                        requires_adding_undefined,
                    );
                }
            }
            NodeData::ArrayLiteralExpression(_) => {
                return self.type_from_array_literal(
                    node,
                    is_const_context,
                    requires_adding_undefined,
                );
            }
            NodeData::ObjectLiteralExpression(_) => {
                return self.type_from_object_literal(
                    node,
                    is_const_context,
                    requires_adding_undefined,
                );
            }
            NodeData::ClassExpression(_) => {
                let inferred = self.infer_expression_type(node, true)?;
                return Ok(SyntacticResult::syntactic(Some(inferred)));
            }
            NodeData::TemplateExpression(_) => {
                if !is_const_context && !preserve_literals {
                    return self
                        .create_keyword_type(source, SyntaxKind::StringKeyword)
                        .map(|r#type| SyntacticResult::syntactic(Some(r#type)));
                }
            }
            _ => {}
        }

        let mut primitive_node = node;
        let base_type = match self.kind(node)? {
            SyntaxKind::NumericLiteral => Some(SyntaxKind::NumberKeyword),
            SyntaxKind::NoSubstitutionTemplateLiteral => {
                let text = self.literal_text(node)?.to_owned();
                primitive_node = self.create_string_literal(source, text)?;
                Some(SyntaxKind::StringKeyword)
            }
            SyntaxKind::StringLiteral => Some(SyntaxKind::StringKeyword),
            SyntaxKind::BigIntLiteral => Some(SyntaxKind::BigIntKeyword),
            SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword => Some(SyntaxKind::BooleanKeyword),
            _ => None,
        };
        if let Some(base_type) = base_type {
            return self.type_from_primitive_literal(
                primitive_node,
                base_type,
                is_const_context || preserve_literals,
                requires_adding_undefined,
            );
        }
        Ok(SyntacticResult::failed())
    }

    /// tsc-port: typeFromFunctionLikeExpression @6.0.3
    /// tsc-hash: dac44bf3aeff2abba5cf1cd413b0b89f809afbfd664442505b8c5b9209b41cf3
    /// tsc-span: _tsc.js:134083-134099
    fn type_from_function_like_expression(
        &mut self,
        node: TransformNode,
    ) -> Result<SyntacticResult, EmitResolverError> {
        let node = self.node_in_source(self.target, node)?;
        self.type_from_function_like_expression_in_source(node)
    }

    /// tsrs-native: `typeFromFunctionLikeExpression`'s body on the target-source node (#4e).
    fn type_from_function_like_expression_in_source(
        &mut self,
        node: TransformNode,
    ) -> Result<SyntacticResult, EmitResolverError> {
        let return_type = self.create_return_from_signature(node, None, true)?;
        let (type_parameters, parameters) = self.function_type_parameters_and_parameters(node)?;
        let type_parameters = self.reuse_type_parameters(node.source(), type_parameters)?;
        let mut ensured_parameters = Vec::new();
        for parameter in self.nodes(node.source(), parameters)? {
            ensured_parameters.push(self.ensure_parameter(parameter)?);
        }
        let parameters = self.create_node_array(node.source(), ensured_parameters)?;
        let r#type = self.create_type_node(
            node.source(),
            NodeData::FunctionType(FunctionTypeData {
                type_parameters,
                parameters: Some(parameters.array()),
                r#type: Some(return_type.node()),
                modifiers: None,
            }),
        )?;
        Ok(SyntacticResult::syntactic(Some(r#type)))
    }

    /// tsc-port: canGetTypeFromArrayLiteral @6.0.3
    /// tsc-hash: 48df0e7a6cf9f1baa438f3aac54aa3549e94c9497cba9c1cba8dda4e7591a5cd
    /// tsc-span: _tsc.js:134100-134112
    fn can_get_type_from_array_literal(
        &mut self,
        array_literal: TransformNode,
        is_const_context: bool,
    ) -> Result<bool, EmitResolverError> {
        if !is_const_context {
            self.report_inference_fallback(array_literal)?;
            return Ok(false);
        }
        let NodeData::ArrayLiteralExpression(data) = self.node(array_literal)?.data.clone() else {
            return Ok(false);
        };
        for element in self.nodes(array_literal.source(), data.elements)? {
            if self.kind(element)? == SyntaxKind::SpreadElement {
                self.report_inference_fallback(element)?;
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: typeFromArrayLiteral @6.0.3
    /// tsc-hash: 2431975c6161697a74205141f9641c62a5f334c6e62dcb017aaa1cb2b88ed9c4
    /// tsc-span: _tsc.js:134113-134145
    fn type_from_array_literal(
        &mut self,
        array_literal: TransformNode,
        is_const_context: bool,
        requires_adding_undefined: bool,
    ) -> Result<SyntacticResult, EmitResolverError> {
        if !self.can_get_type_from_array_literal(array_literal, is_const_context)? {
            if requires_adding_undefined
                || self.expression_is_declaration_initializer(array_literal)?
            {
                return Ok(SyntacticResult::already_reported());
            }
            return self
                .infer_expression_type(array_literal, false)
                .map(|r#type| SyntacticResult::syntactic(Some(r#type)));
        }
        let old_no_inference_fallback = save_no_inference_fallback(self.context);
        let result = (|| {
            let NodeData::ArrayLiteralExpression(data) = self.node(array_literal)?.data.clone()
            else {
                return Ok(SyntacticResult::not_implemented());
            };
            let mut element_types = Vec::new();
            for element in self.nodes(array_literal.source(), data.elements)? {
                if self.kind(element)? == SyntaxKind::OmittedExpression {
                    element_types.push(self.create_undefined_type_node(array_literal.source())?);
                } else {
                    let expression_type =
                        self.type_from_expression(element, is_const_context, false, false)?;
                    let element_type = match expression_type.r#type {
                        Some(r#type) => r#type,
                        None => {
                            self.infer_expression_type(element, expression_type.report_fallback)?
                        }
                    };
                    element_types.push(element_type);
                }
            }
            let tuple = self.create_tuple_type(array_literal.source(), element_types)?;
            self.arena
                .metadata_mut(tuple)
                .add_flags(EmitFlags::SINGLE_LINE);
            Ok(SyntacticResult::not_implemented())
        })();
        restore_no_inference_fallback(self.context, old_no_inference_fallback);
        result
    }

    /// tsc-port: canGetTypeFromObjectLiteral @6.0.3
    /// tsc-hash: 59976f6fafe490e01cd5681beb491a819e9e17bb01f5ccd5ca061025966f08f9
    /// tsc-span: _tsc.js:134146-134174
    fn can_get_type_from_object_literal(
        &mut self,
        object_literal: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let NodeData::ObjectLiteralExpression(data) = self.node(object_literal)?.data.clone()
        else {
            return Ok(false);
        };
        let mut result = true;
        for property in self.nodes(object_literal.source(), data.properties)? {
            if NodeFlags::from_bits(self.node(property)?.flags)
                .intersects(NodeFlags::THIS_NODE_HAS_ERROR)
            {
                return Ok(false);
            }
            if matches!(
                self.kind(property)?,
                SyntaxKind::ShorthandPropertyAssignment | SyntaxKind::SpreadAssignment
            ) {
                self.report_inference_fallback(property)?;
                result = false;
                continue;
            }
            let Some(name) = self.name_of(property)? else {
                return Ok(false);
            };
            if NodeFlags::from_bits(self.node(name)?.flags)
                .intersects(NodeFlags::THIS_NODE_HAS_ERROR)
            {
                return Ok(false);
            }
            if self.kind(name)? == SyntaxKind::PrivateIdentifier {
                result = false;
            } else if let NodeData::ComputedPropertyName(data) = self.node(name)?.data.clone() {
                let Some(expression) = self.child(name.source(), data.expression) else {
                    return Ok(false);
                };
                if !self.is_primitive_literal_value(expression, false)?
                    && !self
                        .resolver
                        .is_definitely_reference_to_global_symbol_object(self.arena, expression)?
                {
                    self.report_inference_fallback(name)?;
                    result = false;
                }
            }
        }
        Ok(result)
    }

    /// tsc-port: typeFromObjectLiteral @6.0.3
    /// tsc-hash: c39996ec238e6596ad43ad4a0408abe5051696954de9a57e0729deefcfdc8e71
    /// tsc-span: _tsc.js:134175-134221
    fn type_from_object_literal(
        &mut self,
        object_literal: TransformNode,
        is_const_context: bool,
        requires_adding_undefined: bool,
    ) -> Result<SyntacticResult, EmitResolverError> {
        if !self.can_get_type_from_object_literal(object_literal)? {
            if requires_adding_undefined
                || self.expression_is_declaration_initializer(object_literal)?
            {
                return Ok(SyntacticResult::already_reported());
            }
            return self
                .infer_expression_type(object_literal, false)
                .map(|r#type| SyntacticResult::syntactic(Some(r#type)));
        }
        let old_no_inference_fallback = save_no_inference_fallback(self.context);
        let old_flags = self.context.flags;
        self.context.flags.0 |= IN_OBJECT_TYPE_LITERAL;
        let result = (|| {
            let NodeData::ObjectLiteralExpression(data) = self.node(object_literal)?.data.clone()
            else {
                return Ok(SyntacticResult::not_implemented());
            };
            let mut properties = Vec::new();
            for property in self.nodes(object_literal.source(), data.properties)? {
                let Some(name) = self.name_of(property)? else {
                    continue;
                };
                let synthesized = match self.kind(property)? {
                    SyntaxKind::MethodDeclaration => self.with_new_scope(property, |session| {
                        session.type_from_object_literal_method(property, name, is_const_context)
                    })?,
                    SyntaxKind::PropertyAssignment => {
                        Some(self.type_from_object_literal_property_assignment(
                            property,
                            name,
                            is_const_context,
                        )?)
                    }
                    SyntaxKind::SetAccessor | SyntaxKind::GetAccessor => {
                        self.type_from_object_literal_accessor(property, name)?
                    }
                    _ => None,
                };
                if let Some(synthesized) = synthesized {
                    self.set_comment_range_from(synthesized, property)?;
                    properties.push(synthesized);
                }
            }
            self.context.flags = old_flags;
            let type_node = self.create_type_literal(object_literal.source(), properties)?;
            if !self
                .context
                .flags
                .contains(EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS)
            {
                self.arena
                    .metadata_mut(type_node)
                    .add_flags(EmitFlags::SINGLE_LINE);
            }
            Ok(SyntacticResult::not_implemented())
        })();
        self.context.flags = old_flags;
        restore_no_inference_fallback(self.context, old_no_inference_fallback);
        result
    }

    /// tsc-port: typeFromObjectLiteralPropertyAssignment @6.0.3
    /// tsc-hash: e9fa156a8f2135b5749df691a5e6a6579ebbd20af8ecfe550e1fc8e3075e8c05
    /// tsc-span: _tsc.js:134222-134239
    fn type_from_object_literal_property_assignment(
        &mut self,
        property: TransformNode,
        name: TransformNode,
        is_const_context: bool,
    ) -> Result<TransformNode, EmitResolverError> {
        let NodeData::PropertyAssignment(data) = self.node(property)?.data.clone() else {
            return Err(self.required_child_error(SyntaxKind::PropertyAssignment, "initializer"));
        };
        let Some(initializer) = self.child(property.source(), data.initializer) else {
            return Err(self.required_child_error(SyntaxKind::PropertyAssignment, "initializer"));
        };
        let modifiers = if is_const_context {
            Some(
                self.create_modifier_array(property.source(), SyntaxKind::ReadonlyKeyword)?
                    .array(),
            )
        } else {
            Some(
                self.create_node_array(property.source(), Vec::new())?
                    .array(),
            )
        };
        let expression = self.type_from_expression(initializer, is_const_context, false, false)?;
        let property_type = match expression.r#type {
            Some(r#type) => Some(r#type),
            None => self.infer_type_of_declaration(property, None, expression.report_fallback)?,
        };
        let name = self.reuse_node_required(name, None)?;
        self.create_type_node(
            property.source(),
            NodeData::PropertySignature(PropertySignatureData {
                name: Some(name.node()),
                question_token: None,
                modifiers,
                r#type: property_type.map(TransformNode::node),
                initializer: None,
            }),
        )
    }

    /// tsc-port: ensureParameter @6.0.3
    /// tsc-hash: 41e5cf3583f7ad54e44d1124ba4f1d41fc9ffa44bbcacc064e7503d6541213d3
    /// tsc-span: _tsc.js:134240-134258
    fn ensure_parameter(
        &mut self,
        parameter: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        let NodeData::Parameter(mut data) = self.node(parameter)?.data.clone() else {
            return Err(self.required_child_error(SyntaxKind::Parameter, "name"));
        };
        data.modifiers = None;
        data.dot_dot_dot_token = match self.child(parameter.source(), data.dot_dot_dot_token) {
            Some(token) => Some(self.reuse_node_required(token, None)?.node()),
            None => None,
        };
        data.name = Some(
            self.resolver
                .serialize_name_of_parameter(self.arena, self.target, self.context, parameter)?
                .node(),
        );
        data.question_token = if SyntacticBuilderResolver::is_optional_parameter(
            &mut *self.resolver,
            self.arena,
            parameter,
        )? {
            Some(
                self.create_token(
                    parameter.source(),
                    SyntaxKind::QuestionToken,
                    TransformFlags::NONE,
                )?
                .node(),
            )
        } else {
            None
        };
        data.r#type = self
            .type_from_parameter(parameter, None)?
            .map(TransformNode::node);
        data.initializer = None;
        self.update_node(parameter, NodeData::Parameter(data))
    }

    /// tsc-port: reuseTypeParameters @6.0.3
    /// tsc-hash: a453e9f7f80e75f8dc438316906a6bdb485fc3c5f1b5729ece67ae261ffd17e4
    /// tsc-span: _tsc.js:134259-134271
    fn reuse_type_parameters(
        &mut self,
        source: TransformSourceId,
        type_parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, EmitResolverError> {
        let Some(type_parameters) = self.array(source, type_parameters) else {
            return Ok(None);
        };
        let mut updated = Vec::new();
        let type_parameter_ids = self
            .arena
            .node_array(type_parameters)
            .map_err(|error| self.factory_error(error))?
            .nodes
            .clone();
        let type_parameter_nodes: Vec<_> = type_parameter_ids
            .into_iter()
            .filter_map(|node| self.arena.node_ref(source, node))
            .collect();
        for type_parameter in type_parameter_nodes {
            let NodeData::TypeParameter(mut data) = self.node(type_parameter)?.data.clone() else {
                continue;
            };
            let Some(name) = self.child(source, data.name) else {
                continue;
            };
            data.name = Some(
                self.resolver
                    .track_existing_entity_name(self.arena, self.target, self.context, name)?
                    .node
                    .node(),
            );
            if let Some(modifiers) = self.array(source, data.modifiers) {
                let modifier_nodes = self
                    .arena
                    .node_array(modifiers)
                    .map_err(|error| self.factory_error(error))?
                    .nodes
                    .clone();
                let mut reused = Vec::new();
                for modifier in modifier_nodes {
                    if let Some(modifier) = self.arena.node_ref(source, modifier) {
                        reused.push(self.reuse_node_required(modifier, None)?);
                    }
                }
                data.modifiers = Some(self.create_node_array(source, reused)?.array());
            }
            data.constraint = self
                .serialize_existing_type_node_with_fallback(
                    self.child(source, data.constraint),
                    false,
                    None,
                )?
                .map(TransformNode::node);
            data.r#default = self
                .serialize_existing_type_node_with_fallback(
                    self.child(source, data.r#default),
                    false,
                    None,
                )?
                .map(TransformNode::node);
            updated.push(self.update_node(type_parameter, NodeData::TypeParameter(data))?);
        }
        self.create_node_array(source, updated)
            .map(|array| Some(array.array()))
    }

    /// tsc-port: typeFromObjectLiteralMethod @6.0.3
    /// tsc-hash: 6aa59e07a37670d5fddd84147e248d2975b2dc7d849b765b9d047357a3dbd1cb
    /// tsc-span: _tsc.js:134272-134305
    fn type_from_object_literal_method(
        &mut self,
        method: TransformNode,
        mut name: TransformNode,
        is_const_context: bool,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::MethodDeclaration(data) = self.node(method)?.data.clone() else {
            return Ok(None);
        };
        let return_type = self.create_return_from_signature(method, None, true)?;
        let type_parameters = self.reuse_type_parameters(method.source(), data.type_parameters)?;
        let mut parameters = Vec::new();
        for parameter in self.nodes(method.source(), data.parameters)? {
            parameters.push(self.ensure_parameter(parameter)?);
        }
        let parameters = self.create_node_array(method.source(), parameters)?;
        if is_const_context {
            let function_type = self.create_type_node(
                method.source(),
                NodeData::FunctionType(FunctionTypeData {
                    type_parameters,
                    parameters: Some(parameters.array()),
                    r#type: Some(return_type.node()),
                    modifiers: None,
                }),
            )?;
            let name = self.reuse_node_required(name, None)?;
            let question = match self.child(method.source(), data.question_token) {
                Some(question) => Some(self.reuse_node_required(question, None)?.node()),
                None => None,
            };
            let modifiers =
                self.create_modifier_array(method.source(), SyntaxKind::ReadonlyKeyword)?;
            return self
                .create_type_node(
                    method.source(),
                    NodeData::PropertySignature(PropertySignatureData {
                        name: Some(name.node()),
                        question_token: question,
                        modifiers: Some(modifiers.array()),
                        r#type: Some(function_type.node()),
                        initializer: None,
                    }),
                )
                .map(Some);
        }
        if self.kind(name)? == SyntaxKind::Identifier && self.identifier_text(name)? == "new" {
            name = self.create_string_literal(method.source(), "new")?;
        }
        let name = self.reuse_node_required(name, None)?;
        let question = match self.child(method.source(), data.question_token) {
            Some(question) => Some(self.reuse_node_required(question, None)?.node()),
            None => None,
        };
        let modifiers = self.create_node_array(method.source(), Vec::new())?;
        self.create_type_node(
            method.source(),
            NodeData::MethodSignature(MethodSignatureData {
                name: Some(name.node()),
                type_parameters,
                parameters: Some(parameters.array()),
                r#type: Some(return_type.node()),
                question_token: question,
                modifiers: Some(modifiers.array()),
            }),
        )
        .map(Some)
    }

    /// tsc-port: typeFromObjectLiteralAccessor @6.0.3
    /// tsc-hash: 9c54493a026acb3e925759c0405fd116f58e514a20271802a11d05d86ab36e52
    /// tsc-span: _tsc.js:134306-134352
    fn type_from_object_literal_accessor(
        &mut self,
        accessor: TransformNode,
        name: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let accessors = self
            .resolver
            .get_all_accessor_declarations(self.arena, accessor)?;
        let get_type = self.get_type_annotation_from_accessor(accessors.get_accessor)?;
        let set_type = self.get_type_annotation_from_accessor(accessors.set_accessor)?;
        if let (Some(get_type), Some(_set_type)) = (get_type, set_type) {
            return self.with_new_scope(accessor, |session| {
                let (_, parameter_array) =
                    session.function_type_parameters_and_parameters(accessor)?;
                let mut parameters = Vec::new();
                for parameter in session.nodes(accessor.source(), parameter_array)? {
                    parameters.push(session.ensure_parameter(parameter)?);
                }
                let parameters = session.create_node_array(accessor.source(), parameters)?;
                let name = session.reuse_node_required(name, None)?;
                let modifiers = session.create_node_array(accessor.source(), Vec::new())?;
                match session.node(accessor)?.data.clone() {
                    NodeData::GetAccessor(mut data) => {
                        data.modifiers = Some(modifiers.array());
                        data.name = Some(name.node());
                        data.parameters = Some(parameters.array());
                        data.r#type = session
                            .serialize_existing_type_node_with_fallback(
                                Some(get_type),
                                false,
                                None,
                            )?
                            .map(TransformNode::node);
                        data.body = None;
                        session
                            .update_node(accessor, NodeData::GetAccessor(data))
                            .map(Some)
                    }
                    NodeData::SetAccessor(mut data) => {
                        data.modifiers = Some(modifiers.array());
                        data.name = Some(name.node());
                        data.parameters = Some(parameters.array());
                        data.body = None;
                        session
                            .update_node(accessor, NodeData::SetAccessor(data))
                            .map(Some)
                    }
                    _ => Ok(None),
                }
            });
        }
        if accessors.first_accessor != accessor {
            return Ok(None);
        }
        let found_type = if let (Some(getter), Some(get_type)) = (accessors.get_accessor, get_type)
        {
            self.with_new_scope(getter, |session| {
                session.serialize_existing_type_node_with_fallback(Some(get_type), false, None)
            })?
        } else if let (Some(setter), Some(set_type)) = (accessors.set_accessor, set_type) {
            self.with_new_scope(setter, |session| {
                session.serialize_existing_type_node_with_fallback(Some(set_type), false, None)
            })?
        } else {
            None
        };
        let property_type = match found_type {
            Some(found_type) => found_type,
            None => self.infer_accessor_type(accessor, accessors, None, true)?,
        };
        let modifiers = if accessors.set_accessor.is_none() {
            self.create_modifier_array(accessor.source(), SyntaxKind::ReadonlyKeyword)?
        } else {
            self.create_node_array(accessor.source(), Vec::new())?
        };
        let name = self.reuse_node_required(name, None)?;
        self.create_type_node(
            accessor.source(),
            NodeData::PropertySignature(PropertySignatureData {
                name: Some(name.node()),
                question_token: None,
                modifiers: Some(modifiers.array()),
                r#type: Some(property_type.node()),
                initializer: None,
            }),
        )
        .map(Some)
    }

    /// tsc-port: createUndefinedTypeNode @6.0.3
    /// tsc-hash: 25193b696fb519392895aa1c08129b93100203a9da7dfef6d1d59baa2cafbfe1
    /// tsc-span: _tsc.js:134353-134359
    fn create_undefined_type_node(
        &mut self,
        source: TransformSourceId,
    ) -> Result<TransformNode, EmitResolverError> {
        self.create_keyword_type(
            source,
            if self.builder.strict_null_checks {
                SyntaxKind::UndefinedKeyword
            } else {
                SyntaxKind::AnyKeyword
            },
        )
    }

    /// tsc-port: typeFromPrimitiveLiteral @6.0.3
    /// tsc-hash: 6b87b0e019de151dd63d0421a21e0e7d538db118b4b47dd2396fbc51d7bb5fdf
    /// tsc-span: _tsc.js:134360-134371
    fn type_from_primitive_literal(
        &mut self,
        node: TransformNode,
        base_type: SyntaxKind,
        preserve_literals: bool,
        requires_adding_undefined: bool,
    ) -> Result<SyntacticResult, EmitResolverError> {
        let result = if preserve_literals {
            let reused = self.reuse_node_required(node, None)?;
            self.create_literal_type(node.source(), reused)?
        } else {
            self.create_keyword_type(node.source(), base_type)?
        };
        self.add_undefined_if_needed(result, requires_adding_undefined, Some(node))
            .map(|r#type| SyntacticResult::syntactic(Some(r#type)))
    }

    /// tsc-port: addUndefinedIfNeeded @6.0.3
    /// tsc-hash: 842dbb50f8689404ba5f49123e54619d2203aefe766e2d7792af8923c11995e5
    /// tsc-span: _tsc.js:134372-134383
    fn add_undefined_if_needed(
        &mut self,
        node: TransformNode,
        add_undefined: bool,
        owner: Option<TransformNode>,
    ) -> Result<TransformNode, EmitResolverError> {
        let optional_declaration = match owner {
            Some(owner) => self
                .parent_of_walked_parenthesized_expression(owner)?
                .is_some_and(|parent| self.is_optional_declaration(parent).unwrap_or(false)),
            None => false,
        };
        if !self.builder.strict_null_checks || !(add_undefined || optional_declaration) {
            return Ok(node);
        }
        if !self.can_add_undefined(node)? {
            self.report_inference_fallback(node)?;
        }
        let undefined = self.create_keyword_type(node.source(), SyntaxKind::UndefinedKeyword)?;
        if let NodeData::UnionType(data) = self.node(node)?.data.clone() {
            let mut types = self.nodes(node.source(), data.types)?;
            types.push(undefined);
            self.create_union_type(node.source(), types)
        } else {
            self.create_union_type(node.source(), vec![node, undefined])
        }
    }

    /// tsc-port: canAddUndefined @6.0.3
    /// tsc-hash: 970fc8bf27dbde3b8a2906b15560256d19aaafa5a1f612140f730b3fac189a38
    /// tsc-span: _tsc.js:134384-134396
    fn can_add_undefined(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        if !self.builder.strict_null_checks {
            return Ok(true);
        }
        let kind = self.kind(node)?;
        if (kind >= SyntaxKind::FirstKeyword && kind <= SyntaxKind::LastKeyword)
            || matches!(
                kind,
                SyntaxKind::LiteralType
                    | SyntaxKind::FunctionType
                    | SyntaxKind::ConstructorType
                    | SyntaxKind::ArrayType
                    | SyntaxKind::TupleType
                    | SyntaxKind::TypeLiteral
                    | SyntaxKind::TemplateLiteralType
                    | SyntaxKind::ThisType
            )
        {
            return Ok(true);
        }
        if let NodeData::ParenthesizedType(data) = &self.node(node)?.data {
            return match self.child(node.source(), data.r#type) {
                Some(inner) => self.can_add_undefined(inner),
                None => Ok(false),
            };
        }
        let types = match &self.node(node)?.data {
            NodeData::UnionType(data) => data.types,
            NodeData::IntersectionType(data) => data.types,
            _ => return Ok(false),
        };
        for r#type in self.nodes(node.source(), types)? {
            if !self.can_add_undefined(r#type)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: createReturnFromSignature @6.0.3
    /// tsc-hash: 1e237223cdce33e60b17473553ec75a4a4a36d23e0147d6c3c33037a16063359
    /// tsc-span: _tsc.js:134397-134406
    fn create_return_from_signature(
        &mut self,
        function: TransformNode,
        symbol: Option<SyntacticSymbol>,
        report_fallback: bool,
    ) -> Result<TransformNode, EmitResolverError> {
        let return_type_node = if self.is_jsdoc_construct_signature(function)? {
            let (_, parameters) = self.function_type_parameters_and_parameters(function)?;
            self.nodes(function.source(), parameters)?
                .first()
                .copied()
                .map(|parameter| self.effective_type_annotation_node(parameter))
                .transpose()?
                .flatten()
        } else {
            self.effective_return_type_node(function)?
        };
        let mut return_type = SyntacticResult::failed();
        if return_type_node.is_some() {
            return_type =
                SyntacticResult::syntactic(self.serialize_type_annotation_of_declaration(
                    return_type_node,
                    function,
                    symbol,
                    None,
                )?);
        } else if self.is_value_signature_declaration(function)? {
            return_type = self.type_from_single_return_expression(function)?;
        }
        match return_type.r#type {
            Some(return_type) => Ok(return_type),
            None => self.infer_return_type_of_signature_signature(
                function,
                symbol,
                report_fallback && return_type.report_fallback && return_type_node.is_none(),
            ),
        }
    }

    /// tsc-port: typeFromSingleReturnExpression @6.0.3
    /// tsc-hash: 300d9bc8493ff2d3f460dc696531a2ebf20651741256b7a044ab0d41e8c55138
    /// tsc-span: _tsc.js:134407-134441
    fn type_from_single_return_expression(
        &mut self,
        declaration: TransformNode,
    ) -> Result<SyntacticResult, EmitResolverError> {
        let Some(body) = self.function_body(declaration)? else {
            return Ok(SyntacticResult::failed());
        };
        if self.node(body)?.pos == u32::MAX && self.node(body)?.end == u32::MAX {
            return Ok(SyntacticResult::failed());
        }
        if self.function_flags(declaration)? & 3 != 0 {
            return Ok(SyntacticResult::failed());
        }
        let candidate = if self.kind(body)? == SyntaxKind::Block {
            self.single_top_level_return_expression(body)?
        } else {
            Some(body)
        };
        let Some(candidate) = candidate else {
            return Ok(SyntacticResult::failed());
        };
        if self.is_contextually_typed(candidate)? {
            let assertion = if self.is_jsdoc_type_assertion(candidate)? {
                self.jsdoc_type_assertion_type(candidate)?
            } else {
                match &self.node(candidate)?.data {
                    NodeData::AsExpression(data) => self.child(candidate.source(), data.r#type),
                    NodeData::TypeAssertionExpression(data) => {
                        self.child(candidate.source(), data.r#type)
                    }
                    _ => None,
                }
            };
            if let Some(assertion) = assertion {
                if !self.is_const_type_reference(assertion)? {
                    return Ok(SyntacticResult::syntactic(
                        self.serialize_existing_type_node(Some(assertion), false)?,
                    ));
                }
            }
        } else {
            return self.type_from_expression(candidate, false, false, false);
        }
        Ok(SyntacticResult::failed())
    }

    /// tsc-port: isContextuallyTyped @6.0.3
    /// tsc-hash: 045888c7689997501d48c7fecb06b6beefc553a072825502ad1019cc0ba70468
    /// tsc-span: _tsc.js:134442-134446
    fn is_contextually_typed(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let mut current = self.parent(node);
        while let Some(ancestor) = current {
            let kind = self.kind(ancestor)?;
            if kind == SyntaxKind::CallExpression
                || (!Self::is_function_like_kind(kind)
                    && self.effective_type_annotation_node(ancestor)?.is_some())
                || matches!(kind, SyntaxKind::JsxElement | SyntaxKind::JsxExpression)
            {
                return Ok(true);
            }
            current = self.parent(ancestor);
        }
        Ok(false)
    }

    fn report_inference_fallback(&mut self, node: TransformNode) -> Result<(), EmitResolverError> {
        let suppress = self.context.suppress_report_inference_fallback;
        self.context.tracker.report_inference_fallback(
            &mut self.context.reported_diagnostic,
            suppress,
            self.resolver,
            node.node(),
        )
    }

    fn reuse_node_required(
        &mut self,
        node: TransformNode,
        range: Option<TransformNode>,
    ) -> Result<TransformNode, EmitResolverError> {
        self.reuse_node(Some(node), range)?.ok_or_else(|| {
            self.required_child_error(self.kind(node).unwrap_or(SyntaxKind::Unknown), "reuseNode")
        })
    }

    fn create_modifier_array(
        &mut self,
        source: TransformSourceId,
        kind: SyntaxKind,
    ) -> Result<TransformNodeArray, EmitResolverError> {
        let modifier = self
            .arena
            .factory()
            .create_modifier(source, kind)
            .map_err(|error| EmitResolverError::Factory {
                method: self.method,
                error: Box::new(error),
            })?;
        self.create_node_array(source, vec![modifier])
    }

    fn set_comment_range_from(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<(), EmitResolverError> {
        let record = self.node(original)?;
        let (pos, end) = (record.pos, record.end);
        let source = self
            .arena
            .source(original.source())
            .map_err(|error| self.factory_error(error))?
            .syntax();
        let range = SourceRange::from_raw(pos, end, source.positions()).map_err(|error| {
            self.factory_error(TransformError::InvalidSourceRange {
                node: original,
                error,
            })
        })?;
        self.arena
            .metadata_mut(node)
            .set_comment_range(CommentRange::new(original.source(), range));
        Ok(())
    }

    fn is_in_js_file(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        Ok(NodeFlags::from_bits(self.node(node)?.flags).intersects(NodeFlags::JAVA_SCRIPT_FILE))
    }

    fn is_const_type_reference(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let NodeData::TypeReference(data) = &self.node(node)?.data else {
            return Ok(false);
        };
        if data.type_arguments.is_some() {
            return Ok(false);
        }
        let Some(name) = self.child(node.source(), data.type_name) else {
            return Ok(false);
        };
        Ok(self.kind(name)? == SyntaxKind::Identifier && self.identifier_text(name)? == "const")
    }

    fn direct_jsdoc_tags(
        &self,
        host: TransformNode,
    ) -> Result<Vec<TransformNode>, EmitResolverError> {
        let Some(docs) = self.node(host)?.js_doc else {
            return Ok(Vec::new());
        };
        let mut tags = Vec::new();
        for doc in self.nodes(host.source(), Some(docs))? {
            let NodeData::JSDoc(data) = &self.node(doc)?.data else {
                continue;
            };
            tags.extend(self.nodes(host.source(), data.tags)?);
        }
        Ok(tags)
    }

    fn jsdoc_type_from_tag(
        &self,
        tag: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let expression = match &self.node(tag)?.data {
            NodeData::JSDocParameterTag(data) => data.type_expression,
            NodeData::JSDocPropertyTag(data) => data.type_expression,
            NodeData::JSDocReturnTag(data) => data.type_expression,
            NodeData::JSDocTypeTag(data) => data.type_expression,
            _ => None,
        };
        let Some(expression) = self.child(tag.source(), expression) else {
            return Ok(None);
        };
        self.jsdoc_type_expression_type(expression)
    }

    fn get_jsdoc_type(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        for tag in self.direct_jsdoc_tags(node)? {
            if self.kind(tag)? == SyntaxKind::JSDocTypeTag {
                if let Some(r#type) = self.jsdoc_type_from_tag(tag)? {
                    return Ok(Some(r#type));
                }
            }
        }
        if self.kind(node)? == SyntaxKind::Parameter {
            let name_text = self
                .name_of(node)?
                .filter(|name| self.kind(*name).ok() == Some(SyntaxKind::Identifier))
                .and_then(|name| self.identifier_text(name).ok())
                .map(str::to_owned);
            if let Some(parent) = self.parent(node) {
                for tag in self.direct_jsdoc_tags(parent)? {
                    let NodeData::JSDocParameterTag(data) = self.node(tag)?.data.clone() else {
                        continue;
                    };
                    let tag_name = self
                        .child(tag.source(), data.name)
                        .map(|name| self.rightmost_name(name))
                        .transpose()?
                        .filter(|name| self.kind(*name).ok() == Some(SyntaxKind::Identifier))
                        .and_then(|name| self.identifier_text(name).ok())
                        .map(str::to_owned);
                    if name_text.is_some() && name_text == tag_name {
                        if let Some(r#type) = self.jsdoc_type_from_tag(tag)? {
                            return Ok(Some(r#type));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    fn get_jsdoc_return_type(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let tags = self.direct_jsdoc_tags(node)?;
        for &tag in &tags {
            if self.kind(tag)? == SyntaxKind::JSDocReturnTag {
                if let Some(r#type) = self.jsdoc_type_from_tag(tag)? {
                    return Ok(Some(r#type));
                }
            }
        }
        for tag in tags {
            if self.kind(tag)? != SyntaxKind::JSDocTypeTag {
                continue;
            }
            let Some(r#type) = self.jsdoc_type_from_tag(tag)? else {
                continue;
            };
            return Ok(match &self.node(r#type)?.data {
                NodeData::JSDocFunctionType(data) => self.child(r#type.source(), data.r#type),
                NodeData::FunctionType(data) => self.child(r#type.source(), data.r#type),
                NodeData::TypeLiteral(data) => self
                    .nodes(r#type.source(), data.members)?
                    .into_iter()
                    .find_map(|member| match &self.node(member).ok()?.data {
                        NodeData::CallSignature(data) => self.child(member.source(), data.r#type),
                        _ => None,
                    }),
                _ => None,
            });
        }
        Ok(None)
    }

    fn effective_type_annotation_node(
        &self,
        declaration: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if self.kind(declaration)? == SyntaxKind::FunctionDeclaration
            && !self.is_in_js_file(declaration)?
        {
            return Ok(None);
        }
        if self.kind(declaration)? == SyntaxKind::TypeAliasDeclaration {
            return Ok(None);
        }
        let direct = match &self.node(declaration)?.data {
            NodeData::VariableDeclaration(data) => self.child(declaration.source(), data.r#type),
            NodeData::Parameter(data) => self.child(declaration.source(), data.r#type),
            NodeData::PropertyDeclaration(data) => self.child(declaration.source(), data.r#type),
            NodeData::PropertySignature(data) => self.child(declaration.source(), data.r#type),
            NodeData::MethodSignature(data) => self.child(declaration.source(), data.r#type),
            NodeData::MethodDeclaration(data) => self.child(declaration.source(), data.r#type),
            NodeData::GetAccessor(data) => self.child(declaration.source(), data.r#type),
            NodeData::FunctionDeclaration(data) => self.child(declaration.source(), data.r#type),
            NodeData::FunctionExpression(data) => self.child(declaration.source(), data.r#type),
            NodeData::ArrowFunction(data) => self.child(declaration.source(), data.r#type),
            NodeData::CallSignature(data) => self.child(declaration.source(), data.r#type),
            NodeData::ConstructSignature(data) => self.child(declaration.source(), data.r#type),
            NodeData::IndexSignature(data) => self.child(declaration.source(), data.r#type),
            NodeData::FunctionType(data) => self.child(declaration.source(), data.r#type),
            NodeData::JSDocFunctionType(data) => self.child(declaration.source(), data.r#type),
            NodeData::ConstructorType(data) => self.child(declaration.source(), data.r#type),
            NodeData::JSDocPropertyTag(data) => self
                .child(declaration.source(), data.type_expression)
                .map(|expression| self.jsdoc_type_expression_type(expression))
                .transpose()?
                .flatten(),
            NodeData::JSDocParameterTag(data) => self
                .child(declaration.source(), data.type_expression)
                .map(|expression| self.jsdoc_type_expression_type(expression))
                .transpose()?
                .flatten(),
            _ => None,
        };
        if direct.is_some() || !self.is_in_js_file(declaration)? {
            Ok(direct)
        } else {
            self.get_jsdoc_type(declaration)
        }
    }

    fn effective_return_type_node(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        if let NodeData::JSDocSignature(data) = &self.node(node)?.data {
            let Some(tag) = self.child(node.source(), data.r#type) else {
                return Ok(None);
            };
            return self.jsdoc_type_from_tag(tag);
        }
        let direct = match &self.node(node)?.data {
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            NodeData::FunctionType(data) => data.r#type,
            NodeData::JSDocFunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            NodeData::IndexSignature(data) => data.r#type,
            _ => None,
        };
        let direct = self.child(node.source(), direct);
        if direct.is_some() || !self.is_in_js_file(node)? {
            Ok(direct)
        } else {
            self.get_jsdoc_return_type(node)
        }
    }

    fn set_accessor_value_parameter(
        &self,
        accessor: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let (_, parameters) = self.function_type_parameters_and_parameters(accessor)?;
        let parameters = self.nodes(accessor.source(), parameters)?;
        if parameters.is_empty() {
            return Ok(None);
        }
        let has_this = parameters.len() == 2
            && self.name_of(parameters[0])?.is_some_and(|name| {
                self.kind(name).ok() == Some(SyntaxKind::Identifier)
                    && self.identifier_text(name).ok() == Some("this")
            });
        Ok(parameters.get(usize::from(has_this)).copied())
    }

    fn is_jsdoc_type_assertion(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        if self.kind(node)? != SyntaxKind::ParenthesizedExpression || !self.is_in_js_file(node)? {
            return Ok(false);
        }
        let source = self
            .arena
            .source(node.source())
            .map_err(|error| self.factory_error(error))?;
        Ok(source.contains_parsed_node(node.node())
            && node_util::is_jsdoc_type_assertion(source.syntax(), node.node()))
    }

    fn jsdoc_type_assertion_type(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let source = self
            .arena
            .source(node.source())
            .map_err(|error| self.factory_error(error))?;
        if !source.contains_parsed_node(node.node()) {
            return Ok(None);
        }
        let type_node = node_util::get_jsdoc_type_tag(source.syntax(), node.node())
            .and_then(|tag| node_util::jsdoc_type_expression(source.syntax(), tag))
            .and_then(
                |expression| match &source.syntax().arena.node(expression).data {
                    NodeData::JSDocTypeExpression(data) => data.r#type,
                    _ => None,
                },
            );
        Ok(type_node.and_then(|node_id| self.arena.node_ref(node.source(), node_id)))
    }

    fn is_var_const_like(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let mut flags = NodeFlags::from_bits(self.node(node)?.flags);
        if let Some(parent) = self.parent(node) {
            if self.kind(parent)? == SyntaxKind::VariableDeclarationList {
                flags |= NodeFlags::from_bits(self.node(parent)?.flags);
            }
        }
        let block_scope = flags.bits() & NodeFlags::BLOCK_SCOPED.bits();
        Ok(matches!(block_scope, 2 | 4 | 6))
    }

    fn is_declaration_readonly(
        &self,
        declaration: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let modifiers = match &self.node(declaration)?.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::PropertySignature(data) => data.modifiers,
            NodeData::Parameter(data) => data.modifiers,
            _ => None,
        };
        for modifier in self.nodes(declaration.source(), modifiers)? {
            if self.kind(modifier)? == SyntaxKind::ReadonlyKeyword {
                return Ok(self.kind(declaration)? != SyntaxKind::Parameter);
            }
        }
        Ok(false)
    }

    fn is_primitive_literal_value(
        &self,
        node: TransformNode,
        include_big_int: bool,
    ) -> Result<bool, EmitResolverError> {
        Ok(match self.kind(node)? {
            SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NumericLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral => true,
            SyntaxKind::BigIntLiteral => include_big_int,
            SyntaxKind::PrefixUnaryExpression => {
                let NodeData::PrefixUnaryExpression(data) = &self.node(node)?.data else {
                    return Ok(false);
                };
                let Some(operand) = self.child(node.source(), data.operand) else {
                    return Ok(false);
                };
                data.operator == SyntaxKind::MinusToken
                    && (self.kind(operand)? == SyntaxKind::NumericLiteral
                        || include_big_int && self.kind(operand)? == SyntaxKind::BigIntLiteral)
                    || data.operator == SyntaxKind::PlusToken
                        && self.kind(operand)? == SyntaxKind::NumericLiteral
            }
            _ => false,
        })
    }

    fn function_type_parameters_and_parameters(
        &self,
        function: TransformNode,
    ) -> Result<(Option<NodeArrayId>, Option<NodeArrayId>), EmitResolverError> {
        Ok(match &self.node(function)?.data {
            NodeData::ArrowFunction(data) => (data.type_parameters, data.parameters),
            NodeData::CallSignature(data) => (data.type_parameters, data.parameters),
            NodeData::ConstructSignature(data) => (data.type_parameters, data.parameters),
            NodeData::Constructor(data) => (data.type_parameters, data.parameters),
            NodeData::ConstructorType(data) => (data.type_parameters, data.parameters),
            NodeData::FunctionDeclaration(data) => (data.type_parameters, data.parameters),
            NodeData::FunctionExpression(data) => (data.type_parameters, data.parameters),
            NodeData::FunctionType(data) => (data.type_parameters, data.parameters),
            NodeData::GetAccessor(data) => (data.type_parameters, data.parameters),
            NodeData::IndexSignature(data) => (data.type_parameters, data.parameters),
            NodeData::JSDocFunctionType(data) => (data.type_parameters, data.parameters),
            NodeData::JSDocSignature(data) => (data.type_parameters, data.parameters),
            NodeData::MethodDeclaration(data) => (data.type_parameters, data.parameters),
            NodeData::MethodSignature(data) => (data.type_parameters, data.parameters),
            NodeData::SetAccessor(data) => (data.type_parameters, data.parameters),
            _ => (None, None),
        })
    }

    fn function_body(
        &self,
        function: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let body = match &self.node(function)?.data {
            NodeData::ArrowFunction(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            _ => None,
        };
        Ok(self.child(function.source(), body))
    }

    fn function_flags(&self, function: TransformNode) -> Result<u32, EmitResolverError> {
        let (asterisk, modifiers) = match &self.node(function)?.data {
            NodeData::FunctionDeclaration(data) => (data.asterisk_token, data.modifiers),
            NodeData::FunctionExpression(data) => (data.asterisk_token, data.modifiers),
            NodeData::MethodDeclaration(data) => (data.asterisk_token, data.modifiers),
            NodeData::ArrowFunction(data) => (None, data.modifiers),
            _ => (None, None),
        };
        let mut flags = u32::from(asterisk.is_some());
        for modifier in self.nodes(function.source(), modifiers)? {
            if self.kind(modifier)? == SyntaxKind::AsyncKeyword {
                flags |= 2;
                break;
            }
        }
        if self.function_body(function)?.is_none() {
            flags |= 4;
        }
        Ok(flags)
    }

    fn is_jsdoc_construct_signature(
        &self,
        function: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        if self.kind(function)? != SyntaxKind::JSDocFunctionType {
            return Ok(false);
        }
        let (_, parameters) = self.function_type_parameters_and_parameters(function)?;
        let Some(parameter) = self.nodes(function.source(), parameters)?.first().copied() else {
            return Ok(false);
        };
        Ok(self.name_of(parameter)?.is_some_and(|name| {
            self.kind(name).ok() == Some(SyntaxKind::Identifier)
                && self.identifier_text(name).ok() == Some("new")
        }))
    }

    fn is_value_signature_declaration(
        &self,
        node: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(matches!(
            self.kind(node)?,
            SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::Constructor
        ))
    }

    fn expression_is_declaration_initializer(
        &self,
        expression: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        let mut current = expression;
        while let Some(parent) = self.parent(current) {
            if self.kind(parent)? != SyntaxKind::ParenthesizedExpression {
                break;
            }
            current = parent;
        }
        let Some(parent) = self.parent(current) else {
            return Ok(false);
        };
        Ok(matches!(
            self.kind(parent)?,
            SyntaxKind::VariableDeclaration
                | SyntaxKind::Parameter
                | SyntaxKind::BindingElement
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::ExportAssignment
        ))
    }

    fn parent_of_walked_parenthesized_expression(
        &self,
        expression: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let mut current = expression;
        while let Some(parent) = self.parent(current) {
            if self.kind(parent)? != SyntaxKind::ParenthesizedExpression {
                break;
            }
            current = parent;
        }
        Ok(self.parent(current))
    }

    fn is_optional_declaration(
        &self,
        declaration: TransformNode,
    ) -> Result<bool, EmitResolverError> {
        Ok(match &self.node(declaration)?.data {
            NodeData::PropertyDeclaration(data) => data.question_token.is_some(),
            NodeData::PropertySignature(data) => data.question_token.is_some(),
            NodeData::Parameter(data) => {
                data.question_token.is_some()
                    || self
                        .child(declaration.source(), data.r#type)
                        .is_some_and(|r#type| {
                            self.kind(r#type).ok() == Some(SyntaxKind::JSDocOptionalType)
                        })
            }
            NodeData::JSDocPropertyTag(data) => data.is_bracketed,
            NodeData::JSDocParameterTag(data) => data.is_bracketed,
            _ => false,
        })
    }

    fn single_top_level_return_expression(
        &self,
        body: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::Block(data) = &self.node(body)?.data else {
            return Ok(None);
        };
        let mut candidate = None;
        let mut invalid = false;
        for statement in self.nodes(body.source(), data.statements)? {
            if let NodeData::ReturnStatement(data) = &self.node(statement)?.data {
                if candidate.is_some() {
                    invalid = true;
                    break;
                }
                candidate = self.child(statement.source(), data.expression);
            } else if self.contains_return_statement(statement)? {
                invalid = true;
                break;
            }
        }
        Ok((!invalid).then_some(candidate).flatten())
    }

    fn contains_return_statement(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let source = self
            .arena
            .source(node.source())
            .map_err(|error| self.factory_error(error))?;
        if !source.contains_parsed_node(node.node()) {
            return Ok(false);
        }
        fn scan(source: &tsc_syntax::SourceFile, node: NodeId, root: NodeId) -> bool {
            let kind = source.arena.node(node).kind;
            if kind == SyntaxKind::ReturnStatement {
                return true;
            }
            if node != root && node_util::is_function_like_kind(kind) {
                return false;
            }
            let mut children = Vec::new();
            tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            children.into_iter().any(|child| scan(source, child, root))
        }
        Ok(scan(source.syntax(), node.node(), node.node()))
    }

    fn required_child_error(&self, parent: SyntaxKind, field: &'static str) -> EmitResolverError {
        self.factory_error(TransformError::RequiredChildRemoved { parent, field })
    }

    fn parent(&self, node: TransformNode) -> Option<TransformNode> {
        self.node(node)
            .ok()
            .and_then(|record| record.parent)
            .and_then(|parent| self.arena.node_ref(node.source(), parent))
    }

    fn identifier_text(&self, node: TransformNode) -> Result<&str, EmitResolverError> {
        match &self.node(node)?.data {
            NodeData::Identifier(data) => Ok(&data.escaped_text),
            _ => Err(self.required_child_error(SyntaxKind::Identifier, "escapedText")),
        }
    }

    fn literal_text(&self, node: TransformNode) -> Result<&str, EmitResolverError> {
        match &self.node(node)?.data {
            NodeData::StringLiteral(data) => Ok(&data.text),
            NodeData::NumericLiteral(data) => Ok(&data.text),
            NodeData::BigIntLiteral(data) => Ok(&data.text),
            NodeData::NoSubstitutionTemplateLiteral(data) => Ok(&data.text),
            _ => Err(self.required_child_error(self.kind(node)?, "text")),
        }
    }

    fn rightmost_name(&self, mut node: TransformNode) -> Result<TransformNode, EmitResolverError> {
        loop {
            let right = match &self.node(node)?.data {
                NodeData::JSDocMemberName(data) => data.right,
                NodeData::QualifiedName(data) => data.right,
                _ => None,
            };
            let Some(right) = right.and_then(|right| self.arena.node_ref(node.source(), right))
            else {
                return Ok(node);
            };
            node = right;
        }
    }

    fn skip_type_parentheses(
        &self,
        mut node: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        loop {
            let NodeData::ParenthesizedType(data) = &self.node(node)?.data else {
                return Ok(node);
            };
            let Some(inner) = self.child(node.source(), data.r#type) else {
                return Ok(node);
            };
            node = inner;
        }
    }

    fn skip_expression_parentheses(
        &self,
        mut node: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        loop {
            let NodeData::ParenthesizedExpression(data) = &self.node(node)?.data else {
                return Ok(node);
            };
            let Some(inner) = self.child(node.source(), data.expression) else {
                return Ok(node);
            };
            node = inner;
        }
    }

    fn is_type_node(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let kind = self.kind(node)?;
        Ok(
            (kind >= SyntaxKind::TypePredicate && kind <= SyntaxKind::ImportType)
                || matches!(
                    kind,
                    SyntaxKind::AnyKeyword
                        | SyntaxKind::UnknownKeyword
                        | SyntaxKind::NumberKeyword
                        | SyntaxKind::BigIntKeyword
                        | SyntaxKind::ObjectKeyword
                        | SyntaxKind::BooleanKeyword
                        | SyntaxKind::StringKeyword
                        | SyntaxKind::SymbolKeyword
                        | SyntaxKind::VoidKeyword
                        | SyntaxKind::UndefinedKeyword
                        | SyntaxKind::NeverKeyword
                        | SyntaxKind::IntrinsicKeyword
                        | SyntaxKind::ExpressionWithTypeArguments
                        | SyntaxKind::JSDocAllType
                        | SyntaxKind::JSDocUnknownType
                        | SyntaxKind::JSDocNullableType
                        | SyntaxKind::JSDocNonNullableType
                        | SyntaxKind::JSDocOptionalType
                        | SyntaxKind::JSDocFunctionType
                        | SyntaxKind::JSDocVariadicType
                ),
        )
    }

    fn is_function_like_kind(kind: SyntaxKind) -> bool {
        node_util::is_function_like_kind(kind)
    }

    fn is_new_scope_node(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let kind = self.kind(node)?;
        Ok(Self::is_function_like_kind(kind)
            || matches!(kind, SyntaxKind::JSDocSignature | SyntaxKind::MappedType))
    }

    fn jsdoc_type_expression_type(
        &self,
        expression: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::JSDocTypeExpression(data) = &self.node(expression)?.data else {
            return Ok(None);
        };
        Ok(self.child(expression.source(), data.r#type))
    }

    fn literal_type_literal(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let NodeData::LiteralType(data) = &self.node(node)?.data else {
            return Ok(None);
        };
        Ok(self.child(node.source(), data.literal))
    }

    fn import_type_has_assert_attributes(
        &self,
        source: TransformSourceId,
        import_type: &tsc_syntax::nodes::ImportTypeData,
    ) -> Result<bool, EmitResolverError> {
        let Some(attributes) = self.child(source, import_type.attributes) else {
            return Ok(false);
        };
        Ok(matches!(
            &self.node(attributes)?.data,
            NodeData::ImportAttributes(data) if data.token == SyntaxKind::AssertKeyword
        ))
    }

    fn name_of(&self, node: TransformNode) -> Result<Option<TransformNode>, EmitResolverError> {
        let name = match &self.node(node)?.data {
            NodeData::BindingElement(data) => data.name,
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::ClassExpression(data) => data.name,
            NodeData::EnumDeclaration(data) => data.name,
            NodeData::EnumMember(data) => data.name,
            NodeData::FunctionDeclaration(data) => data.name,
            NodeData::FunctionExpression(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::ImportEqualsDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::MethodSignature(data) => data.name,
            NodeData::ModuleDeclaration(data) => data.name,
            NodeData::Parameter(data) => data.name,
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::PropertySignature(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            NodeData::TypeAliasDeclaration(data) => data.name,
            NodeData::TypeParameter(data) => data.name,
            NodeData::VariableDeclaration(data) => data.name,
            _ => None,
        };
        Ok(self.child(node.source(), name))
    }

    fn is_signed_numeric_literal(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let NodeData::PrefixUnaryExpression(data) = &self.node(node)?.data else {
            return Ok(false);
        };
        if !matches!(
            data.operator,
            SyntaxKind::PlusToken | SyntaxKind::MinusToken
        ) {
            return Ok(false);
        }
        Ok(self
            .child(node.source(), data.operand)
            .is_some_and(|operand| self.kind(operand).ok() == Some(SyntaxKind::NumericLiteral)))
    }

    fn has_dynamic_name(&self, declaration: TransformNode) -> Result<bool, EmitResolverError> {
        let Some(name) = self.name_of(declaration)? else {
            return Ok(false);
        };
        let expression = match &self.node(name)?.data {
            NodeData::ComputedPropertyName(data) => self.child(name.source(), data.expression),
            NodeData::ElementAccessExpression(data) => {
                self.child(name.source(), data.argument_expression)
            }
            _ => None,
        };
        let Some(expression) = expression else {
            return Ok(false);
        };
        let expression = self.skip_expression_parentheses(expression)?;
        Ok(!matches!(
            self.kind(expression)?,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
        ) && !self.is_signed_numeric_literal(expression)?)
    }

    fn declaration_type_field(
        &self,
        node: TransformNode,
    ) -> Result<Option<NodeId>, EmitResolverError> {
        Ok(match &self.node(node)?.data {
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::FunctionType(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::IndexSignature(data) => data.r#type,
            NodeData::JSDocFunctionType(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            NodeData::Parameter(data) => data.r#type,
            NodeData::PropertyDeclaration(data) => data.r#type,
            NodeData::PropertySignature(data) => data.r#type,
            NodeData::SetAccessor(data) => data.r#type,
            NodeData::VariableDeclaration(data) => data.r#type,
            _ => None,
        })
    }

    fn initializer_of(
        &self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let initializer = match &self.node(node)?.data {
            NodeData::BindingElement(data) => data.initializer,
            NodeData::Parameter(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::PropertySignature(data) => data.initializer,
            NodeData::VariableDeclaration(data) => data.initializer,
            _ => None,
        };
        Ok(self.child(node.source(), initializer))
    }

    fn needs_missing_type_any(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let kind = self.kind(node)?;
        let missing_type = self.declaration_type_field(node)?.is_none();
        let missing_initializer = self.initializer_of(node)?.is_none();
        Ok((Self::is_function_like_kind(kind) && missing_type)
            || matches!(
                kind,
                SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature
            ) && missing_type
                && missing_initializer
            || kind == SyntaxKind::Parameter && missing_type && missing_initializer)
    }

    fn with_declaration_type(
        &self,
        node: TransformNode,
        r#type: Option<NodeId>,
        remove_parameter_modifiers: bool,
    ) -> Result<NodeData, EmitResolverError> {
        let mut data = self.node(node)?.data.clone();
        match &mut data {
            NodeData::ArrowFunction(value) => value.r#type = r#type,
            NodeData::CallSignature(value) => value.r#type = r#type,
            NodeData::ConstructSignature(value) => value.r#type = r#type,
            NodeData::Constructor(value) => value.r#type = r#type,
            NodeData::ConstructorType(value) => value.r#type = r#type,
            NodeData::FunctionDeclaration(value) => value.r#type = r#type,
            NodeData::FunctionExpression(value) => value.r#type = r#type,
            NodeData::FunctionType(value) => value.r#type = r#type,
            NodeData::GetAccessor(value) => value.r#type = r#type,
            NodeData::IndexSignature(value) => value.r#type = r#type,
            NodeData::JSDocFunctionType(value) => value.r#type = r#type,
            NodeData::MethodDeclaration(value) => value.r#type = r#type,
            NodeData::MethodSignature(value) => value.r#type = r#type,
            NodeData::Parameter(value) => {
                value.r#type = r#type;
                if remove_parameter_modifiers {
                    value.modifiers = None;
                }
            }
            NodeData::PropertyDeclaration(value) => value.r#type = r#type,
            NodeData::PropertySignature(value) => value.r#type = r#type,
            NodeData::SetAccessor(value) => value.r#type = r#type,
            NodeData::VariableDeclaration(value) => value.r#type = r#type,
            _ => return Err(self.required_child_error(self.kind(node)?, "type")),
        }
        Ok(data)
    }

    fn is_entity_name_expression(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        match &self.node(node)?.data {
            NodeData::Identifier(_) => Ok(true),
            NodeData::PropertyAccessExpression(data) => {
                let Some(name) = self.child(node.source(), data.name) else {
                    return Ok(false);
                };
                let Some(expression) = self.child(node.source(), data.expression) else {
                    return Ok(false);
                };
                Ok(self.kind(name)? == SyntaxKind::Identifier
                    && self.is_entity_name_expression(expression)?)
            }
            _ => Ok(false),
        }
    }

    fn is_jsdoc_index_signature(&self, node: TransformNode) -> Result<bool, EmitResolverError> {
        let NodeData::TypeReference(data) = &self.node(node)?.data else {
            return Ok(false);
        };
        let Some(name) = self.child(node.source(), data.type_name) else {
            return Ok(false);
        };
        if self.kind(name)? != SyntaxKind::Identifier || self.identifier_text(name)? != "Object" {
            return Ok(false);
        }
        let arguments = self.nodes(node.source(), data.type_arguments)?;
        Ok(arguments.len() == 2
            && matches!(
                self.kind(arguments[0])?,
                SyntaxKind::StringKeyword | SyntaxKind::NumberKeyword
            ))
    }

    fn visit_jsdoc_function_type(
        &mut self,
        node: TransformNode,
    ) -> Result<TransformNode, EmitResolverError> {
        let NodeData::JSDocFunctionType(data) = self.node(node)?.data.clone() else {
            return Err(self.required_child_error(SyntaxKind::JSDocFunctionType, "parameters"));
        };
        let source = node.source();
        let type_parameters = self.visit_optional_node_array(source, data.type_parameters)?;
        let parameters = self.nodes(source, data.parameters)?;
        let is_constructor = parameters.first().is_some_and(|parameter| {
            matches!(
                self.node(*parameter).ok().map(|record| &record.data),
                Some(NodeData::Parameter(parameter_data))
                    if self.child(source, parameter_data.name).is_some_and(|name| {
                        self.kind(name).ok() == Some(SyntaxKind::Identifier)
                            && self.identifier_text(name).ok() == Some("new")
                    })
            )
        });
        let mut result_parameters = Vec::with_capacity(parameters.len());
        let mut constructor_type = None;
        for (index, parameter) in parameters.into_iter().enumerate() {
            let NodeData::Parameter(parameter_data) = self.node(parameter)?.data.clone() else {
                continue;
            };
            let is_new = self.child(source, parameter_data.name).is_some_and(|name| {
                self.kind(name).ok() == Some(SyntaxKind::Identifier)
                    && self.identifier_text(name).ok() == Some("new")
            });
            if is_constructor && is_new {
                constructor_type = self.child(source, parameter_data.r#type);
                continue;
            }
            let dot_dot_dot = self
                .get_effective_dot_dot_dot_for_parameter(parameter)?
                .map(TransformNode::node);
            let parameter_name = self.get_name_for_jsdoc_function_parameter(parameter, index)?;
            let name = self.create_identifier(source, parameter_name)?;
            let name = self
                .resolver
                .mark_node_reuse(self.arena, self.context, name, parameter)?;
            let question_token = match self.child(source, parameter_data.question_token) {
                Some(question) => Some(self.clone_node(question)?.node()),
                None => None,
            };
            let r#type = match self.child(source, parameter_data.r#type) {
                Some(r#type) => self
                    .visit_existing_node_tree_symbols(r#type)?
                    .map(TransformNode::node),
                None => None,
            };
            result_parameters.push(self.create_node(
                source,
                NodeData::Parameter(ParameterData {
                    name: Some(name.node()),
                    modifiers: None,
                    dot_dot_dot_token: dot_dot_dot,
                    question_token,
                    r#type,
                    initializer: None,
                }),
                TransformFlags::CONTAINS_TYPE_SCRIPT,
            )?);
        }
        let parameters = self.create_node_array(source, result_parameters)?;
        let return_candidate = constructor_type.or_else(|| self.child(source, data.r#type));
        let return_type = match return_candidate {
            Some(r#type) => match self.visit_existing_node_tree_symbols(r#type)? {
                Some(r#type) => r#type,
                None => self.create_keyword_type(source, SyntaxKind::AnyKeyword)?,
            },
            None => self.create_keyword_type(source, SyntaxKind::AnyKeyword)?,
        };
        if is_constructor {
            self.create_type_node(
                source,
                NodeData::ConstructorType(ConstructorTypeData {
                    type_parameters,
                    parameters: Some(parameters.array()),
                    r#type: Some(return_type.node()),
                    modifiers: None,
                }),
            )
        } else {
            self.create_type_node(
                source,
                NodeData::FunctionType(FunctionTypeData {
                    type_parameters,
                    parameters: Some(parameters.array()),
                    r#type: Some(return_type.node()),
                    modifiers: None,
                }),
            )
        }
    }

    fn visit_optional_child(
        &mut self,
        source: TransformSourceId,
        child: Option<NodeId>,
    ) -> Result<Option<NodeId>, EmitResolverError> {
        match self.child(source, child) {
            Some(child) => Ok(self
                .visit_existing_node_tree_symbols(child)?
                .map(TransformNode::node)),
            None => Ok(None),
        }
    }

    fn visit_optional_node_array(
        &mut self,
        source: TransformSourceId,
        array: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, EmitResolverError> {
        let Some(array) = self.array(source, array) else {
            return Ok(None);
        };
        self.visit_node_array(source, array)
            .map(|array| array.map(TransformNodeArray::array))
    }

    fn visit_node_array(
        &mut self,
        source: TransformSourceId,
        original: TransformNodeArray,
    ) -> Result<Option<TransformNodeArray>, EmitResolverError> {
        let original_nodes = self
            .arena
            .node_array(original)
            .map_err(|error| self.factory_error(error))?
            .nodes
            .clone();
        let mut changed = false;
        let mut nodes = Vec::with_capacity(original_nodes.len());
        for node_id in original_nodes {
            let Some(node) = self.arena.node_ref(source, node_id) else {
                changed = true;
                continue;
            };
            match self.visit_existing_node_tree_symbols(node)? {
                Some(visited) => {
                    changed |= visited != node;
                    nodes.push(visited);
                }
                None => changed = true,
            }
        }
        let mut result = if changed {
            self.create_node_array(source, nodes)?
        } else {
            original
        };
        let enclosing_root = self.context.enclosing_file;
        let source_root = self
            .arena
            .source(source)
            .map_err(|error| self.factory_error(error))?
            .syntax()
            .root;
        if enclosing_root != Some(source_root) {
            result = self.visit_nodes_without_copying_positions(source, original, result)?;
        }
        Ok(Some(result))
    }
}

impl NodeDataChildVisitor for SyntacticBuildSession<'_, '_> {
    type Error = EmitResolverError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        let source = self.visit_sources.last().copied().unwrap_or(self.target);
        self.arena
            .node_ref(source, id)
            .and_then(|node| self.arena.node(node).ok())
            .map_or(SyntaxKind::Unknown, |node| node.kind)
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        let source = self.visit_sources.last().copied().unwrap_or(self.target);
        let Some(node) = self.arena.node_ref(source, id) else {
            return Ok(None);
        };
        Ok(match self.visit_existing_node_tree_symbols(node)? {
            Some(visited) => Some(self.node_in_source(source, visited)?.node()),
            None => None,
        })
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        let source = self.visit_sources.last().copied().unwrap_or(self.target);
        let Some(array) = self.arena.node_array_ref(source, id) else {
            return Ok(None);
        };
        Ok(self
            .visit_node_array(source, array)?
            .map(TransformNodeArray::array))
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        self.factory_error(TransformError::RequiredChildRemoved { parent, field })
    }
}

#[cfg(test)]
#[path = "../tests/unit/syntactic_type_node_builder/tests.rs"]
mod tests;

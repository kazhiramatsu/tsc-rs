use tsc_binder::node_util;
use tsc_emitter::{
    EmitFlags, EmitNodeBuilderFlags, EmitResolverError, EmitResolverMethod, TransformArena,
    TransformError, TransformFlags, TransformNode, TransformNodeArray, TransformSourceId,
};
use tsc_syntax::nodes::{
    ArrayTypeData, ConstructorTypeData, FunctionTypeData, IdentifierData, IndexSignatureData,
    LiteralTypeData, ParameterData, PropertySignatureData, StringLiteralData, TypeLiteralData,
    UnionTypeData,
};
use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::node_builder::{
    NodeBuilderContext, SyntacticBuilderResolver, SyntacticRecoveryBoundary,
};

const USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE: u32 = 268_435_456;

/// The reuse of existing type nodes: strada's syntactic builder visitor
/// (tsgo's nodecopy.go). Its inference from expressions is retired; the
/// pseudochecker (`crate::pseudochecker`) and the node builder's pseudo
/// types replace it.
///
/// tsc-port: createSyntacticTypeNodeBuilder @6.0.3
/// tsc-hash: c98a5407512036e20afdd848d82d832e09b2f728a3ed25f4423136356387e2c9
/// tsc-span: _tsc.js:133276-134447
pub(crate) struct SyntacticTypeNodeBuilder;

impl SyntacticTypeNodeBuilder {
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

    /// tsgo-port: NodeBuilderImpl.tryReuseExistingNodeHelper @7.1
    /// (nodecopy.go:222-232): the existing-node visitor under a recovery
    /// boundary, for any reused node (tsgo has no canReuseTypeNode gate).
    pub(crate) fn try_reuse_existing_node(
        &self,
        resolver: &mut dyn SyntacticBuilderResolver,
        arena: &mut TransformArena,
        target: TransformSourceId,
        context: &mut NodeBuilderContext<'_>,
        existing: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        SyntacticBuildSession::new(
            resolver,
            arena,
            target,
            context,
            EmitResolverMethod::CreateTypeOfDeclaration,
        )
        .try_reuse_existing_type_node(existing)
    }
}

struct SyntacticBuildSession<'a, 'tracker> {
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
        resolver: &'a mut dyn SyntacticBuilderResolver,
        arena: &'a mut TransformArena,
        target: TransformSourceId,
        context: &'a mut NodeBuilderContext<'tracker>,
        method: EmitResolverMethod,
    ) -> Self {
        Self {
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

    /// `factory.createNodeArray(updated, nodes.hasTrailingComma)`: visitNodes
    /// (_tsc.js:91098-91112) and visitNodesWithoutCopyingPositions
    /// (_tsc.js:133665-133676) carry the visited array's trailing comma, which
    /// the printer needs for binding patterns (`{ a, b, }`).
    fn create_visited_node_array(
        &mut self,
        source: TransformSourceId,
        original: TransformNodeArray,
        nodes: Vec<TransformNode>,
    ) -> Result<TransformNodeArray, EmitResolverError> {
        let (has_trailing_comma, pos, end) = {
            let record = self
                .arena
                .node_array(original)
                .map_err(|error| self.factory_error(error))?;
            (record.has_trailing_comma, record.pos, record.end)
        };
        let factory_error = |error| EmitResolverError::Factory {
            method: self.method,
            error: Box::new(error),
        };
        let mut factory = self.arena.factory();
        let array = factory
            .create_node_array_with_trailing_comma(source, nodes, has_trailing_comma)
            .map_err(factory_error)?;
        // tsgo `NodeVisitor.VisitNodes` (ast/visitor.go:99-103): the new
        // list keeps the visited one's range.
        factory
            .set_node_array_text_range(array, pos, end)
            .map_err(factory_error)?;
        Ok(array)
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
                escaped_text: tsc_types::EscapedName::from_identifier_escaped_text(
                    &tsc_syntax::escape_leading_underscores(&text),
                ),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_string_literal(
        &mut self,
        source: TransformSourceId,
        text: impl Into<tsc_types::JsString>,
    ) -> Result<TransformNode, EmitResolverError> {
        self.create_node(
            source,
            NodeData::StringLiteral(StringLiteralData { text: text.into() }),
            TransformFlags::NONE,
        )
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
            .clone_node_keeping_quote(node)
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

    /// tsgo-port: tryVisitSimpleTypeNode @7.1 (nodecopy.go:452-466)
    fn try_visit_simple_type_node(
        &mut self,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        // tsgo skips expression parentheses here (`ast.SkipParentheses`,
        // nodecopy.go:452-466), which leaves a parenthesized type as written.
        let inner = node;
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
                    // tsgo's reparser makes the literal a type literal whose
                    // property names that are not identifiers are string
                    // literals and whose types are the tags' types as written
                    // (reparseJSDocTypeLiteral, reparser.go:244-283).
                    let invalid_name = match &self.node(name)?.data {
                        NodeData::Identifier(identifier)
                            if !tsc_syntax::is_identifier_text(identifier.text()) =>
                        {
                            Some(identifier.text().to_owned())
                        }
                        _ => None,
                    };
                    let name = match invalid_name {
                        Some(text) => self.create_string_literal(source, text)?,
                        None => match self.visit_existing_node_tree_symbols(name)? {
                            Some(name) => name,
                            None => continue,
                        },
                    };
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
                    let property_type = if let Some(r#type) = type_expression {
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
                let literal = self.create_type_literal(source, members)?;
                if data.is_array_type {
                    return self.create_array_type(source, literal).map(Some);
                }
                return Ok(Some(literal));
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
                        );
                    }
                    let specifier = self.rewrite_module_specifier_2(node, literal)?;
                    let reused_literal = if specifier == literal {
                        self.reuse_node_required(literal, Some(literal))?
                    } else {
                        specifier
                    };
                    import_type.argument = if reused_literal == literal {
                        self.reuse_node(Some(argument), Some(argument))?
                            .map(TransformNode::node)
                    } else {
                        Some(self.create_literal_type(source, reused_literal)?.node())
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
                    // tsgo (nodecopy.go:751-759) marks the error instead of
                    // strada's rewriting from the evaluator and the checker.
                    self.mark_recovery_error();
                    return self.visit_each_child_2(node);
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

        // tsgo keeps a reused string literal's characters (an emoji, say)
        // rather than escaping them as ASCII (nodecopy.go:810-821).
        if matches!(
            data,
            NodeData::StringLiteral(_) | NodeData::NoSubstitutionTemplateLiteral(_)
        ) {
            let clone = self.clone_node(node)?;
            if matches!(data, NodeData::StringLiteral(_))
                && self.context.flags.0 & USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE != 0
                && !self
                    .arena
                    .literal_properties(node)
                    .and_then(tsc_emitter::LiteralNodeProperties::string_literal_single_quote)
                    .unwrap_or(false)
            {
                self.arena
                    .literal_properties_mut(clone)
                    .map_err(|error| EmitResolverError::Factory {
                        method: self.method,
                        error: Box::new(error),
                    })?
                    .set_string_literal_single_quote(true);
            }
            self.arena
                .metadata_mut(clone)
                .add_flags(EmitFlags::NO_ASCII_ESCAPING);
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
        // Token children (`?`, `...`, `readonly`, `*`) are visited like any
        // other child: tsgo's NodeVisitor has no token hook here
        // (ast/visitor.go:225-230, checker/nodecopy.go:827-900), so each is
        // cloned and takes its range only from the enclosing file. tsc 6.0
        // kept the original token, which mapped another file's offset.
        let mut data = self.node(node)?.data.clone();
        self.visit_sources.push(node.source());
        let result = try_visit_each_child(&mut data, self);
        self.visit_sources.pop();
        result?;
        self.update_node(node, data).map(Some)
    }

    /// tsgo-port: the VisitNodes hook of the existing-node visitor @7.1
    /// (checker/nodecopy.go:878-888): a list of a node from another file is
    /// copied without its position. `NodeList.HasTrailingComma` compares the
    /// list's end with its last node's (ast/ast.go:139-145), so such a list
    /// has no trailing comma either.
    fn visit_nodes_without_copying_positions(
        &mut self,
        source: TransformSourceId,
        visited: TransformNodeArray,
    ) -> Result<TransformNodeArray, EmitResolverError> {
        let (positioned, has_trailing_comma, nodes) = {
            let record = self
                .arena
                .node_array(visited)
                .map_err(|error| self.factory_error(error))?;
            (
                record.pos != u32::MAX || record.end != u32::MAX,
                record.has_trailing_comma,
                record.nodes.to_vec(),
            )
        };
        if !positioned && !has_trailing_comma {
            return Ok(visited);
        }
        let nodes = nodes
            .into_iter()
            .filter_map(|node| self.arena.node_ref(source, node))
            .collect();
        let result = self
            .arena
            .factory()
            .create_node_array_with_trailing_comma(source, nodes, false)
            .map_err(|error| EmitResolverError::Factory {
                method: self.method,
                error: Box::new(error),
            })?;
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

    fn reuse_node_required(
        &mut self,
        node: TransformNode,
        range: Option<TransformNode>,
    ) -> Result<TransformNode, EmitResolverError> {
        self.reuse_node(Some(node), range)?.ok_or_else(|| {
            self.required_child_error(self.kind(node).unwrap_or(SyntaxKind::Unknown), "reuseNode")
        })
    }

    fn required_child_error(&self, parent: SyntaxKind, field: &'static str) -> EmitResolverError {
        self.factory_error(TransformError::RequiredChildRemoved { parent, field })
    }

    fn identifier_text(&self, node: TransformNode) -> Result<&str, EmitResolverError> {
        match &self.node(node)?.data {
            NodeData::Identifier(data) => Ok(data.escaped_text.identifier_text()),
            _ => Err(self.required_child_error(SyntaxKind::Identifier, "escapedText")),
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
            .to_vec();
        let original_first = original_nodes
            .first()
            .and_then(|&first| self.arena.node_ref(source, first));
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
            self.create_visited_node_array(source, original, nodes)?
        } else {
            original
        };
        // The VisitNodes hook (checker/nodecopy.go:878-900): a list of a node
        // that comes from another file loses its position. A node of another
        // file is visited as its clone in the target source, so the file is
        // the one of the element's most original node.
        let enclosing_root = self.context.enclosing_file;
        let origin_root = match original_first {
            Some(first) => {
                let origin = self.arena.get_original_node(first);
                self.arena
                    .source(origin.source())
                    .map_err(|error| self.factory_error(error))?
                    .syntax()
                    .root
            }
            None => {
                self.arena
                    .source(source)
                    .map_err(|error| self.factory_error(error))?
                    .syntax()
                    .root
            }
        };
        if enclosing_root != Some(origin_root) {
            result = self.visit_nodes_without_copying_positions(source, result)?;
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

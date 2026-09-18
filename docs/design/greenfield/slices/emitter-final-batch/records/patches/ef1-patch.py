#!/usr/bin/env python3
"""EF1: give an access expression's target child its own comments phase (tsc emitExpression)."""
import sys
path = "/Users/hiramatsu/dev/tsc-rs-emitter-final/crates/emitter/src/printer.rs"
src = open(path).read()

def replace_once(old, new):
    global src
    n = src.count(old)
    assert n == 1, f"expected exactly one occurrence, found {n}:\n{old[:200]}"
    src = src.replace(old, new)

# 1. PropertyAccess arm: child phase + anchor from the child's outcome.
replace_once(
"""                let name = transformation
                    .arena()
                    .node_ref(node.source(), name_id)
                    .ok_or(PrinterError::UnknownStatement(name_id.0))?;
                self.emit_node_id_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    expression_id,
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: access_is_optional_chain,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                let token_kind = if data.question_dot_token.is_some() {
""",
"""                let name = transformation
                    .arena()
                    .node_ref(node.source(), name_id)
                    .ok_or(PrinterError::UnknownStatement(name_id.0))?;
                let expression_outcome = self.emit_access_target_with_source_comments(
                    transformation,
                    node.source(),
                    expression_id,
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: access_is_optional_chain,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                let token_kind = if data.question_dot_token.is_some() {
""")
replace_once(
"""                let token_anchor = if let Some(anchor) =
                    deferred_source_comments.visited_trailing_anchor_at(token_cursor)
                {
                    anchor
                } else if break_before_dot {
""",
"""                let token_anchor = if let Some(anchor) = deferred_source_comments
                    .visited_trailing_anchor_at(token_cursor)
                    .or_else(|| Self::access_target_trailing_anchor_at(expression_outcome, token_cursor))
                {
                    anchor
                } else if break_before_dot {
""")

# 2. ElementAccess arm.
replace_once(
"""                self.emit_required_node_with_forwarded_source_comments(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ElementAccessExpression,
                    "expression",
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: access_is_optional_chain,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                let open_cursor = if let Some(question_dot) = data
""",
"""                let expression_id =
                    data.expression
                        .ok_or(PrinterError::MissingTransformedChild {
                            parent: SyntaxKind::ElementAccessExpression,
                            field: "expression",
                        })?;
                let expression_outcome = self.emit_access_target_with_source_comments(
                    transformation,
                    node.source(),
                    expression_id,
                    expression_context.with_grammar(ExpressionGrammarContext::LeftSideOfAccess {
                        optional_chain: access_is_optional_chain,
                    }),
                    deferred_source_comments,
                    writer,
                )?;
                let open_cursor = if let Some(question_dot) = data
""")
replace_once(
"""                let open_anchor = deferred_source_comments
                    .visited_trailing_anchor_at(open_cursor)
                    .unwrap_or_else(|| TokenAnchor::from(open_cursor));
""",
"""                let open_anchor = deferred_source_comments
                    .visited_trailing_anchor_at(open_cursor)
                    .or_else(|| Self::access_target_trailing_anchor_at(expression_outcome, open_cursor))
                    .unwrap_or_else(|| TokenAnchor::from(open_cursor));
""")

# 3. Helpers next to emit_node_id_with_forwarded_source_comments.
replace_once(
"""    fn emit_node_id_with_context(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::Expression
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint(transformation, node, hint, expression_context, writer)
    }
""",
"""    /// The target of a property or element access. tsc's
    /// `emitExpression(node.expression, …)` runs the target's own comments
    /// phase inside the access node's container claim, so a same-line
    /// comment after the target (`(_a = ns).dec /* c */.bind(_a)`) is written
    /// by the target itself. A pending request keeps its forwarding lane; an
    /// inherited NoNested extent suppresses the phase as for every child.
    /// Otherwise the target receives the ordinary nested phase, and its
    /// visited trailing anchor lets the access token skip that boundary
    /// instead of claiming it a second time through a parsed token. A
    /// synthesized access parent (the `.bind` call built around a bound
    /// decorator target) has no parsed token and never claimed that end.
    ///
    /// tsc-port: emitPropertyAccessExpression / emitElementAccessExpression @6.0.3
    /// tsc-span: _tsc.js:118223-118273
    /// tsc-port: pipelineEmitWithComments @6.0.3
    /// tsc-span: _tsc.js:120978-121046
    #[allow(clippy::too_many_arguments)]
    fn emit_access_target_with_source_comments(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        deferred_source_comments: &mut DeferredExpressionSourceCommentsState,
        writer: &mut TextWriter,
    ) -> Result<Option<ExpressionSourceCommentsOutcome>, PrinterError> {
        if deferred_source_comments.is_pending() {
            self.emit_node_id_with_forwarded_source_comments(
                transformation,
                source,
                id,
                expression_context,
                deferred_source_comments,
                writer,
            )?;
            return Ok(None);
        }
        if expression_context.nested_comments_suppressed() {
            self.emit_node_id_with_context(transformation, source, id, expression_context, writer)?;
            return Ok(None);
        }
        let deferred = DeferredExpressionSourceComments::nested(
            expression_context.comments(),
            DeferredSourceCommentExtent::LeadingAndTrailing,
        );
        let outcome = self.emit_node_id_with_context_and_source_comments(
            transformation,
            source,
            id,
            expression_context,
            deferred,
            writer,
        )?;
        debug_assert!(matches!(
            outcome,
            ExpressionSourceCommentsOutcome::Complete { .. }
        ));
        Ok(Some(outcome))
    }

    /// The trailing anchor an access target's own phase visited at exactly
    /// the access token's source boundary, if any.
    fn access_target_trailing_anchor_at(
        outcome: Option<ExpressionSourceCommentsOutcome>,
        cursor: TokenCursor,
    ) -> Option<TokenAnchor> {
        outcome
            .and_then(ExpressionSourceCommentsOutcome::visited_trailing_anchor)
            .filter(|anchor| anchor.cursor() == cursor)
    }

    fn emit_node_id_with_context(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: EmitContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::Expression
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint(transformation, node, hint, expression_context, writer)
    }
""")
open(path, "w").write(src)
print("EF1 patch applied")

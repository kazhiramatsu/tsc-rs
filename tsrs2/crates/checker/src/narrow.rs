//! M5 6.4: the narrowers — narrowType dispatch + sub-narrowers
//! (m5-flow-steps.md stage 6.4; checker-key-functions.md §4.5).
//!
//! tsc SHAPE FACT (70766-71460): narrowType and every sub-narrower
//! are CLOSURES inside getFlowTypeOfReference, reading the query's
//! `reference` binding; the port threads the same [`FlowQuery`] the
//! walk carries. `inlineLevel` alone is checker state (tsc 46453).
//!
//! Stage state (6.4a): the dispatch itself is live — expression kinds
//! outside tsc's switch pass through unchanged (tsc's own answer, no
//! flag), parenthesized/`!`-prefix forms recurse for real. Every
//! sub-narrower is a [FLOW 6.4] identity stub that FLAGS the query
//! (`FlowQuery::traversed_inert_arm`): tsc would narrow here, we
//! cannot yet, so the query exit reverts the answer to the 6.2 value
//! (declared type, auto-converted) and the ladder sites partial-mark
//! the flagged positions. Each of 6.4b-g replaces one stub with the
//! verbatim port and its cases stop flagging (canary/rate per
//! commit); the const-inlining arm keeps flagging until 6.4h.

use tsrs2_binder::node_util;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::TypeId;

use crate::flow::FlowQuery;
use crate::state::{CheckResult2, CheckerState};

impl<'a> CheckerState<'a> {
    /// tsc-port: narrowType @6.0.3
    /// tsc-hash: 27ad08800fadb3f5234051fdb384cffe54850849b9ba6eca2c94f97e7979a821
    /// tsc-span: _tsc.js:71400-71439
    ///
    /// The dispatch (checker-key §4.5): optional-chain roots and
    /// `??`/`??=` left operands divert to optionality narrowing before
    /// the kind switch; parenthesized/nonnull/satisfies wrappers and
    /// `!` recurse; kinds outside the switch narrow nothing (tsc
    /// returns the type unchanged — real semantics, not a stub). The
    /// Identifier arm's const-variable guard-inlining RECURSION is
    /// 6.4h; until it lands, an identifier meeting every inlining
    /// condition flags the query instead (the narrowing tsc performs
    /// through the const is unreproducible, and the pass-through
    /// answer would be over-wide).
    pub(crate) fn narrow_type(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(expr);
        let optionality = node_util::is_expression_of_optional_chain_root(source, expr) || {
            match self.parent_of(expr) {
                Some(parent) if self.kind_of(parent) == SyntaxKind::BinaryExpression => {
                    match self.data_of(parent) {
                        NodeData::BinaryExpression(data) => {
                            data.left == Some(expr)
                                && data.operator_token.is_some_and(|operator| {
                                    matches!(
                                        self.kind_of(operator),
                                        SyntaxKind::QuestionQuestionToken
                                            | SyntaxKind::QuestionQuestionEqualsToken
                                    )
                                })
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        };
        if optionality {
            return self.narrow_type_by_optionality(query, ty, expr, assume_true);
        }
        match self.kind_of(expr) {
            SyntaxKind::Identifier => {
                if !self.is_matching_reference(query.reference, expr)? && self.inline_level < 5 {
                    if let Some(symbol) = self.get_resolved_symbol(expr)? {
                        if self.is_constant_variable(symbol) {
                            let declaration = self.binder.symbol(symbol).value_declaration;
                            if let Some(declaration) = declaration {
                                if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
                                    let (annotated, initializer) = match self.data_of(declaration) {
                                        NodeData::VariableDeclaration(data) => {
                                            (data.r#type.is_some(), data.initializer)
                                        }
                                        _ => (true, None),
                                    };
                                    if !annotated
                                        && initializer.is_some()
                                        && self.is_constant_reference(query.reference)?
                                    {
                                        // [FLOW 6.4h] the inlining
                                        // recursion into the const's
                                        // initializer is unported —
                                        // flag; tsc returns the
                                        // recursion's answer here.
                                        query.traversed_inert_arm = true;
                                    }
                                }
                            }
                        }
                    }
                }
                self.narrow_type_by_truthiness(query, ty, expr, assume_true)
            }
            SyntaxKind::ThisKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression => {
                self.narrow_type_by_truthiness(query, ty, expr, assume_true)
            }
            SyntaxKind::CallExpression => {
                self.narrow_type_by_call_expression(query, ty, expr, assume_true)
            }
            SyntaxKind::ParenthesizedExpression
            | SyntaxKind::NonNullExpression
            | SyntaxKind::SatisfiesExpression => {
                let inner = match self.data_of(expr) {
                    NodeData::ParenthesizedExpression(data) => data.expression,
                    NodeData::NonNullExpression(data) => data.expression,
                    NodeData::SatisfiesExpression(data) => data.expression,
                    _ => None,
                };
                match inner {
                    Some(inner) => self.narrow_type(query, ty, inner, assume_true),
                    None => {
                        // Parser-recovery wrapper with no operand —
                        // tsc always has one; unreproducible, flag.
                        query.traversed_inert_arm = true;
                        Ok(ty)
                    }
                }
            }
            SyntaxKind::BinaryExpression => {
                self.narrow_type_by_binary_expression(query, ty, expr, assume_true)
            }
            SyntaxKind::PrefixUnaryExpression => {
                let (operator, operand) = match self.data_of(expr) {
                    NodeData::PrefixUnaryExpression(data) => (data.operator, data.operand),
                    _ => (SyntaxKind::Unknown, None),
                };
                if operator == SyntaxKind::ExclamationToken {
                    match operand {
                        Some(operand) => {
                            return self.narrow_type(query, ty, operand, !assume_true);
                        }
                        None => {
                            // Parser-recovery `!` with no operand.
                            query.traversed_inert_arm = true;
                        }
                    }
                }
                Ok(ty)
            }
            _ => Ok(ty),
        }
    }

    /// [FLOW 6.4] identity stub: tsc narrowTypeByTruthiness (70855)
    /// narrows by Truthy/Falsy facts (+ the discriminant-property
    /// path); until it lands every dispatch into it flags the query so
    /// the exit reverts to the declared type.
    /// tsc-deferred: M5 (stage 6.4b — narrowTypeByTruthiness)
    fn narrow_type_by_truthiness(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        _expr: NodeId,
        _assume_true: bool,
    ) -> CheckResult2<TypeId> {
        query.traversed_inert_arm = true;
        Ok(ty)
    }

    /// [FLOW 6.4] identity stub: tsc narrowTypeByBinaryExpression
    /// (70895) dispatches ===/!==/==/!=/instanceof/in/assignment
    /// narrowing; flags the query until 6.4c.
    /// tsc-deferred: M5 (stage 6.4c — narrowTypeByBinaryExpression)
    fn narrow_type_by_binary_expression(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        _expr: NodeId,
        _assume_true: bool,
    ) -> CheckResult2<TypeId> {
        query.traversed_inert_arm = true;
        Ok(ty)
    }

    /// [FLOW 6.4] identity stub: tsc narrowTypeByCallExpression
    /// (71351) narrows by type predicates (`x is T`, `asserts x`);
    /// flags the query until 6.4f.
    /// tsc-deferred: M5 (stage 6.4f — narrowTypeByCallExpression)
    fn narrow_type_by_call_expression(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        _expr: NodeId,
        _assume_true: bool,
    ) -> CheckResult2<TypeId> {
        query.traversed_inert_arm = true;
        Ok(ty)
    }

    /// [FLOW 6.4] identity stub: tsc narrowTypeByOptionality (71440)
    /// strips/keeps undefined|null for optional-chain roots and
    /// `??`/`??=` left operands; flags the query until 6.4g.
    /// tsc-deferred: M5 (stage 6.4g — narrowTypeByOptionality)
    fn narrow_type_by_optionality(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        _expr: NodeId,
        _assume_present: bool,
    ) -> CheckResult2<TypeId> {
        query.traversed_inert_arm = true;
        Ok(ty)
    }
}

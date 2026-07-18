//! M5 6.4: the narrowers — narrowType dispatch + sub-narrowers
//! (m5-flow-steps.md stage 6.4; checker-key-functions.md §4.5).
//!
//! tsc SHAPE FACT (70766-71460): narrowType and every sub-narrower
//! are CLOSURES inside getFlowTypeOfReference, reading the query's
//! `reference` binding; the port threads the same [`FlowQuery`] the
//! walk carries. `inlineLevel` alone is checker state (tsc 46453).
//!
//! Stage state (6.4b): the dispatch (6.4a) and narrowTypeByTruthiness
//! with the discriminant-property path (6.4b) are live — expression
//! kinds outside tsc's switch pass through unchanged (tsc's own
//! answer, no flag), parenthesized/`!`-prefix forms recurse for real.
//! The remaining sub-narrowers are [FLOW 6.4] identity stubs that
//! FLAG the query (`FlowQuery::traversed_inert_arm`): tsc would
//! narrow there, we cannot yet, so the query exit reverts the answer
//! to the 6.2 value (declared type, auto-converted) and the ladder
//! sites partial-mark the flagged positions. Each of 6.4c-g replaces
//! one stub with the verbatim port and its cases stop flagging
//! (canary/rate per commit); the const-inlining arm keeps flagging
//! until 6.4h.

use tsrs2_binder::node_util;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{TypeFacts, TypeFlags, TypeId};

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
                if !self.is_matching_query_reference(query, expr)? && self.inline_level < 5 {
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
                                    let constant_reference = if query.synthetic_props.is_some() {
                                        // tsc's isConstantReference over the
                                        // synthetic chain can be true (readonly
                                        // discriminants) — assume so and flag
                                        // rather than miss the inlining.
                                        true
                                    } else {
                                        self.is_constant_reference(query.reference)?
                                    };
                                    if !annotated && initializer.is_some() && constant_reference {
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

    /// tsc-port: narrowTypeByTruthiness @6.0.3
    /// tsc-hash: 8c5e780261fe6e814b7dfc011cc8feb4ed2327ba15376c0884e38e1949351f13
    /// tsc-span: _tsc.js:70855-70867
    ///
    /// A matching reference narrows by the Truthy/Falsy facts; a
    /// non-matching expr still strips undefined|null when the
    /// reference sits inside an optional chain of it (strict, true
    /// branch), then tries the discriminant-property path with the
    /// plain (non-adjusted) facts filter.
    fn narrow_type_by_truthiness(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let facts = if assume_true {
            TypeFacts::TRUTHY
        } else {
            TypeFacts::FALSY
        };
        if self.is_matching_query_reference(query, expr)? {
            return self.get_adjusted_type_with_facts(ty, facts);
        }
        let mut ty = ty;
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        if strict_null_checks
            && assume_true
            && self.optional_chain_contains_query_reference(expr, query)?
        {
            ty = self.get_adjusted_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)?;
        }
        if let Some(access) = self.get_discriminant_property_access(query, expr, ty)? {
            return self.narrow_type_by_discriminant(ty, access, |state, t| {
                state.get_type_with_facts(t, facts)
            });
        }
        Ok(ty)
    }

    /// tsc-port: getCandidateDiscriminantPropertyAccess @6.0.3
    /// tsc-hash: d74ba7cbd8738c2c5568a90acf6e4ba2f31272a34cccdfd981c15c3d985383e9
    /// tsc-span: _tsc.js:70766-70799
    ///
    /// Three reference shapes: (1) a binding-pattern/function
    /// reference (getNarrowedTypeOfSymbol's destructuring query)
    /// candidates its OWN parameter/binding-element declarations, (2)
    /// an access expression over a matching receiver candidates
    /// itself, (3) a const variable whose initializer is an access
    /// over a matching receiver (guard aliasing, `const isFoo =
    /// x.kind === "foo"` style) candidates the initializer — or, for
    /// a binding element destructured FROM the matching reference,
    /// the element itself.
    fn get_candidate_discriminant_property_access(
        &mut self,
        query: &FlowQuery,
        expr: NodeId,
    ) -> CheckResult2<Option<NodeId>> {
        let reference = query.reference;
        let source = self.binder.source_of_node(reference);
        // A synthetic destructuring reference (6.4b) is an access
        // chain — never the binding-pattern/function shape of arm 1.
        if query.synthetic_props.is_none()
            && (node_util::is_binding_pattern(source, reference)
                || self.is_function_expression_or_arrow_function(reference)
                || self.is_object_literal_method(reference))
        {
            if self.kind_of(expr) == SyntaxKind::Identifier {
                let Some(symbol) = self.get_resolved_symbol(expr)? else {
                    return Ok(None);
                };
                let export_symbol = self.get_export_symbol_of_value_symbol_if_exported(symbol);
                if let Some(declaration) = self.binder.symbol(export_symbol).value_declaration {
                    let kind = self.kind_of(declaration);
                    if kind == SyntaxKind::BindingElement || kind == SyntaxKind::Parameter {
                        let (initializer, dot_dot_dot) = match self.data_of(declaration) {
                            NodeData::BindingElement(data) => {
                                (data.initializer, data.dot_dot_dot_token)
                            }
                            NodeData::Parameter(data) => (data.initializer, data.dot_dot_dot_token),
                            _ => (None, None),
                        };
                        if self.parent_of(declaration) == Some(reference)
                            && initializer.is_none()
                            && dot_dot_dot.is_none()
                        {
                            return Ok(Some(declaration));
                        }
                    }
                }
            }
            return Ok(None);
        }
        if matches!(
            self.kind_of(expr),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            let receiver = match self.data_of(expr) {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                _ => None,
            };
            if let Some(receiver) = receiver {
                if self.is_matching_query_reference(query, receiver)? {
                    return Ok(Some(expr));
                }
            }
            return Ok(None);
        }
        if self.kind_of(expr) == SyntaxKind::Identifier {
            let Some(symbol) = self.get_resolved_symbol(expr)? else {
                return Ok(None);
            };
            if !self.is_constant_variable(symbol) {
                return Ok(None);
            }
            let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
                return Ok(None);
            };
            if let Some(initializer) = self.candidate_variable_declaration_initializer(declaration)
            {
                if matches!(
                    self.kind_of(initializer),
                    SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                ) {
                    let receiver = match self.data_of(initializer) {
                        NodeData::PropertyAccessExpression(data) => data.expression,
                        NodeData::ElementAccessExpression(data) => data.expression,
                        _ => None,
                    };
                    if let Some(receiver) = receiver {
                        if self.is_matching_query_reference(query, receiver)? {
                            return Ok(Some(initializer));
                        }
                    }
                }
            }
            if self.kind_of(declaration) == SyntaxKind::BindingElement {
                let element_initializer = match self.data_of(declaration) {
                    NodeData::BindingElement(data) => data.initializer,
                    _ => None,
                };
                if element_initializer.is_none() {
                    let grand = self
                        .parent_of(declaration)
                        .and_then(|pattern| self.parent_of(pattern));
                    if let Some(grand) = grand {
                        if let Some(initializer) =
                            self.candidate_variable_declaration_initializer(grand)
                        {
                            if matches!(
                                self.kind_of(initializer),
                                SyntaxKind::Identifier
                                    | SyntaxKind::PropertyAccessExpression
                                    | SyntaxKind::ElementAccessExpression
                            ) && self.is_matching_query_reference(query, initializer)?
                            {
                                return Ok(Some(declaration));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// The inner getCandidateVariableDeclarationInitializer
    /// (70796-70798): an annotation-free initializered
    /// VariableDeclaration's initializer, parens skipped.
    /// tsrs-native: extracted inner closure of the candidate walk
    /// (same tsc span).
    fn candidate_variable_declaration_initializer(&self, node: NodeId) -> Option<NodeId> {
        if self.kind_of(node) != SyntaxKind::VariableDeclaration {
            return None;
        }
        match self.data_of(node) {
            NodeData::VariableDeclaration(data) if data.r#type.is_none() => data
                .initializer
                .map(|initializer| self.skip_parentheses(initializer)),
            _ => None,
        }
    }

    /// tsc-port: getDiscriminantPropertyAccess @6.0.3
    /// tsc-hash: 92d9a3c195445ff4e7e9a91e43f087290d5e99886821a1caae52b68cbc489913
    /// tsc-span: _tsc.js:70800-70814
    ///
    /// The candidate is admitted when its accessed name is a
    /// discriminant property of the union in play — the declared type
    /// when the computed type is still a subset of it, else the
    /// computed type.
    fn get_discriminant_property_access(
        &mut self,
        query: &FlowQuery,
        expr: NodeId,
        computed_type: TypeId,
    ) -> CheckResult2<Option<NodeId>> {
        let declared_union = self
            .tables
            .flags_of(query.declared_type)
            .intersects(TypeFlags::UNION);
        let computed_union = self
            .tables
            .flags_of(computed_type)
            .intersects(TypeFlags::UNION);
        if !declared_union && !computed_union {
            return Ok(None);
        }
        let Some(access) = self.get_candidate_discriminant_property_access(query, expr)? else {
            return Ok(None);
        };
        let Some(name) = self.get_accessed_property_name(access)? else {
            return Ok(None);
        };
        let ty = if declared_union && self.is_type_subset_of(computed_type, query.declared_type)? {
            query.declared_type
        } else {
            computed_type
        };
        if self.is_discriminant_property(ty, &name)? {
            Ok(Some(access))
        } else {
            Ok(None)
        }
    }

    /// tsc-port: narrowTypeByDiscriminant @6.0.3
    /// tsc-hash: 6d81a7365602d3b3e3b0f8598d509b97f4471753947867f1e167341f3881879f
    /// tsc-span: _tsc.js:70815-70832
    ///
    /// Narrows the discriminant property's type through the supplied
    /// filter, then keeps the union members whose own discriminant
    /// (property or index signature) is comparable to the narrowed
    /// value. Optional-chain/nonnull accesses strip nullable members
    /// before the property read (adding optionality back for chains).
    fn narrow_type_by_discriminant(
        &mut self,
        ty: TypeId,
        access: NodeId,
        mut narrow_prop_type: impl FnMut(&mut Self, TypeId) -> CheckResult2<TypeId>,
    ) -> CheckResult2<TypeId> {
        let Some(prop_name) = self.get_accessed_property_name(access)? else {
            return Ok(ty);
        };
        let source = self.binder.source_of_node(access);
        let optional_chain = node_util::is_optional_chain(source, access);
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let remove_nullable = strict_null_checks
            && (optional_chain || self.is_non_null_access(access))
            && self.maybe_type_of_kind(ty, TypeFlags::NULLABLE);
        let base = if remove_nullable {
            self.get_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)?
        } else {
            ty
        };
        let Some(mut prop_type) = self.get_type_of_property_of_type_full(base, &prop_name)? else {
            return Ok(ty);
        };
        if remove_nullable && optional_chain {
            prop_type = self.get_optional_type(prop_type, /*is_property*/ false)?;
        }
        let narrowed_prop_type = narrow_prop_type(self, prop_type)?;
        let unknown = self.tables.intrinsics.unknown;
        self.filter_type_with(ty, |state, t| {
            let discriminant_type = state
                .get_type_of_property_or_index_signature_of_type(t, &prop_name)?
                .unwrap_or(unknown);
            if state
                .tables
                .flags_of(discriminant_type)
                .intersects(TypeFlags::NEVER)
                || state
                    .tables
                    .flags_of(narrowed_prop_type)
                    .intersects(TypeFlags::NEVER)
            {
                return Ok(false);
            }
            state.are_types_comparable(narrowed_prop_type, discriminant_type)
        })
    }

    /// tsc-port: getTypeOfPropertyOfType @6.0.3
    /// tsc-hash: ddd47344f8b1b3d0de20c2241560a370790f21248f978ea95d10914e91566057
    /// tsc-span: _tsc.js:55803-55806
    ///
    /// The FULL union/intersection-capable form (getPropertyOfType +
    /// getTypeOfSymbol) — engine.rs's same-named accessor is the M3
    /// object-member slice and stays for its M3-era callers.
    pub(crate) fn get_type_of_property_of_type_full(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<TypeId>> {
        match self.get_property_of_type_full(ty, name)? {
            Some(prop) => Ok(Some(self.get_type_of_symbol(prop)?)),
            None => Ok(None),
        }
    }

    /// tsc isNonNullAccess (19318): an access expression whose
    /// receiver is a NonNullExpression.
    /// tsrs-native: NodeData accessor (utilities-side one-liner).
    fn is_non_null_access(&self, node: NodeId) -> bool {
        let receiver = match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        receiver.is_some_and(|receiver| self.kind_of(receiver) == SyntaxKind::NonNullExpression)
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

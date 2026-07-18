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

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{SymbolFlags, TypeData, TypeFacts, TypeFlags, TypeId};

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

    /// tsc-port: narrowTypeByBinaryExpression @6.0.3
    /// tsc-hash: 537de335a28a3d4b8ea226ddcc1d74e8adfa24eec1d418b232ed880adc27ed74
    /// tsc-span: _tsc.js:70895-71020
    ///
    /// The operator dispatch: assignments recurse through the RHS
    /// then truthiness-narrow the LHS; (in)equality tries typeof
    /// forms (6.4d stub), matching-reference equality, optional-chain
    /// containment, discriminant properties, `.constructor`
    /// comparisons, and boolean-literal comparisons; `instanceof`/`in`
    /// take their own narrowers; comma recurses right; aliased
    /// `&&`/`||` (the binder splits real control-flow ones) recurse
    /// both sides with the tsc union combinations.
    fn narrow_type_by_binary_expression(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let (left, operator_token, right) = match self.data_of(expr) {
            NodeData::BinaryExpression(data) => (data.left, data.operator_token, data.right),
            _ => (None, None, None),
        };
        let (Some(expr_left), Some(operator_token), Some(expr_right)) =
            (left, operator_token, right)
        else {
            // Parser-recovery binary with missing pieces — tsc always
            // has all three; unreproducible, flag.
            query.traversed_inert_arm = true;
            return Ok(ty);
        };
        match self.kind_of(operator_token) {
            SyntaxKind::EqualsToken
            | SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken => {
                let narrowed = self.narrow_type(query, ty, expr_right, assume_true)?;
                self.narrow_type_by_truthiness(query, narrowed, expr_left, assume_true)
            }
            operator @ (SyntaxKind::EqualsEqualsToken
            | SyntaxKind::ExclamationEqualsToken
            | SyntaxKind::EqualsEqualsEqualsToken
            | SyntaxKind::ExclamationEqualsEqualsToken) => {
                let left = self.get_reference_candidate(expr_left);
                let right = self.get_reference_candidate(expr_right);
                if self.kind_of(left) == SyntaxKind::TypeOfExpression
                    && self.is_string_literal_like(right)
                {
                    return self.narrow_type_by_typeof(
                        query,
                        ty,
                        left,
                        operator,
                        right,
                        assume_true,
                    );
                }
                if self.kind_of(right) == SyntaxKind::TypeOfExpression
                    && self.is_string_literal_like(left)
                {
                    return self.narrow_type_by_typeof(
                        query,
                        ty,
                        right,
                        operator,
                        left,
                        assume_true,
                    );
                }
                if self.is_matching_query_reference(query, left)? {
                    return self.narrow_type_by_equality(ty, operator, right, assume_true);
                }
                if self.is_matching_query_reference(query, right)? {
                    return self.narrow_type_by_equality(ty, operator, left, assume_true);
                }
                let mut ty = ty;
                if self
                    .options
                    .strict_option_value(self.options.strict_null_checks)
                {
                    if self.optional_chain_contains_query_reference(left, query)? {
                        ty = self.narrow_type_by_optional_chain_containment(
                            ty,
                            operator,
                            right,
                            assume_true,
                        )?;
                    } else if self.optional_chain_contains_query_reference(right, query)? {
                        ty = self.narrow_type_by_optional_chain_containment(
                            ty,
                            operator,
                            left,
                            assume_true,
                        )?;
                    }
                }
                if let Some(left_access) = self.get_discriminant_property_access(query, left, ty)? {
                    return self.narrow_type_by_discriminant_property(
                        ty,
                        left_access,
                        operator,
                        right,
                        assume_true,
                    );
                }
                if let Some(right_access) =
                    self.get_discriminant_property_access(query, right, ty)?
                {
                    return self.narrow_type_by_discriminant_property(
                        ty,
                        right_access,
                        operator,
                        left,
                        assume_true,
                    );
                }
                if self.is_matching_constructor_reference(query, left)? {
                    return self.narrow_type_by_constructor(ty, operator, right, assume_true);
                }
                if self.is_matching_constructor_reference(query, right)? {
                    return self.narrow_type_by_constructor(ty, operator, left, assume_true);
                }
                let left_is_access = matches!(
                    self.kind_of(left),
                    SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                );
                let right_is_access = matches!(
                    self.kind_of(right),
                    SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                );
                if self.is_boolean_literal(right) && !left_is_access {
                    return self.narrow_type_by_boolean_comparison(
                        query,
                        ty,
                        left,
                        right,
                        operator,
                        assume_true,
                    );
                }
                if self.is_boolean_literal(left) && !right_is_access {
                    return self.narrow_type_by_boolean_comparison(
                        query,
                        ty,
                        right,
                        left,
                        operator,
                        assume_true,
                    );
                }
                Ok(ty)
            }
            SyntaxKind::InstanceOfKeyword => {
                self.narrow_type_by_instanceof(query, ty, expr, assume_true)
            }
            SyntaxKind::InKeyword => {
                if self.kind_of(expr_left) == SyntaxKind::PrivateIdentifier {
                    return self.narrow_type_by_private_identifier_in_in_expression(
                        query,
                        ty,
                        expr,
                        assume_true,
                    );
                }
                let target = self.get_reference_candidate(expr_right);
                if self.contains_missing_type(ty)
                    && self.query_reference_is_access(query)
                    && self.query_reference_receiver_matches(query, target)?
                {
                    let left_type = self.get_type_of_expression(expr_left)?;
                    if self.is_type_usable_as_property_name(left_type) {
                        let reference_name = self.query_reference_accessed_property_name(query)?;
                        let left_name = self.get_property_name_from_type(left_type);
                        if reference_name.is_some() && reference_name == left_name {
                            return self.get_type_with_facts(
                                ty,
                                if assume_true {
                                    TypeFacts::NE_UNDEFINED
                                } else {
                                    TypeFacts::EQ_UNDEFINED
                                },
                            );
                        }
                    }
                }
                if self.is_matching_query_reference(query, target)? {
                    let left_type = self.get_type_of_expression(expr_left)?;
                    if self.is_type_usable_as_property_name(left_type) {
                        return self.narrow_type_by_in_keyword(ty, left_type, assume_true);
                    }
                }
                Ok(ty)
            }
            SyntaxKind::CommaToken => self.narrow_type(query, ty, expr_right, assume_true),
            SyntaxKind::AmpersandAmpersandToken => {
                if assume_true {
                    let narrowed = self.narrow_type(query, ty, expr_left, true)?;
                    self.narrow_type(query, narrowed, expr_right, true)
                } else {
                    let left_narrowed = self.narrow_type(query, ty, expr_left, false)?;
                    let right_narrowed = self.narrow_type(query, ty, expr_right, false)?;
                    self.get_union_type_ex(
                        &[left_narrowed, right_narrowed],
                        tsrs2_types::UnionReduction::Literal,
                    )
                }
            }
            SyntaxKind::BarBarToken => {
                if assume_true {
                    let left_narrowed = self.narrow_type(query, ty, expr_left, true)?;
                    let right_narrowed = self.narrow_type(query, ty, expr_right, true)?;
                    self.get_union_type_ex(
                        &[left_narrowed, right_narrowed],
                        tsrs2_types::UnionReduction::Literal,
                    )
                } else {
                    let narrowed = self.narrow_type(query, ty, expr_left, false)?;
                    self.narrow_type(query, narrowed, expr_right, false)
                }
            }
            _ => Ok(ty),
        }
    }

    /// tsc-port: narrowTypeByBooleanComparison @6.0.3
    /// tsc-hash: 166103ea0f194af80e05f3d062c38f4faa07b537cb5149eca1c530463c56f254
    /// tsc-span: _tsc.js:70891-70894
    ///
    /// `x === false` narrows like `!x`, etc. — the polarity is the
    /// XOR of the literal's truth and the operator's negation.
    fn narrow_type_by_boolean_comparison(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        bool_literal: NodeId,
        operator: SyntaxKind,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let literal_is_true = self.kind_of(bool_literal) == SyntaxKind::TrueKeyword;
        let operator_is_equality = operator != SyntaxKind::ExclamationEqualsEqualsToken
            && operator != SyntaxKind::ExclamationEqualsToken;
        let assume_true = (assume_true != literal_is_true) != operator_is_equality;
        self.narrow_type(query, ty, expr, assume_true)
    }

    /// [FLOW 6.4] identity stub: tsc narrowTypeByTypeof (71081)
    /// narrows by `typeof x === "..."` comparisons; flags the query
    /// until 6.4d.
    /// tsc-deferred: M5 (stage 6.4d — narrowTypeByTypeof)
    fn narrow_type_by_typeof(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        _typeof_expr: NodeId,
        _operator: SyntaxKind,
        _literal: NodeId,
        _assume_true: bool,
    ) -> CheckResult2<TypeId> {
        query.traversed_inert_arm = true;
        Ok(ty)
    }

    /// tsc-port: narrowTypeByEquality @6.0.3
    /// tsc-hash: 8b3d5c124c150471de7e71f86b3d4ee35c63ad79759f572c757c270b9a3c5605
    /// tsc-span: _tsc.js:71048-71080
    ///
    /// Matching-reference (in)equality: nullable comparands filter by
    /// the null/undefined facts (strict only); a true branch filters
    /// to comparable members (with the unknown/empty-object `===`
    /// adoption of the comparand) and re-literalizes primitives; a
    /// false branch removes unit-like comparable members for unit
    /// comparands.
    fn narrow_type_by_equality(
        &mut self,
        ty: TypeId,
        operator: SyntaxKind,
        value: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok(ty);
        }
        let assume_true = if operator == SyntaxKind::ExclamationEqualsToken
            || operator == SyntaxKind::ExclamationEqualsEqualsToken
        {
            !assume_true
        } else {
            assume_true
        };
        let value_type = self.get_type_of_expression(value)?;
        let value_flags = self.tables.flags_of(value_type);
        let double_equals = operator == SyntaxKind::EqualsEqualsToken
            || operator == SyntaxKind::ExclamationEqualsToken;
        if value_flags.intersects(TypeFlags::NULLABLE) {
            if !self
                .options
                .strict_option_value(self.options.strict_null_checks)
            {
                return Ok(ty);
            }
            let facts = if double_equals {
                if assume_true {
                    TypeFacts::EQ_UNDEFINED_OR_NULL
                } else {
                    TypeFacts::NE_UNDEFINED_OR_NULL
                }
            } else if value_flags.intersects(TypeFlags::NULL) {
                if assume_true {
                    TypeFacts::EQ_NULL
                } else {
                    TypeFacts::NE_NULL
                }
            } else if assume_true {
                TypeFacts::EQ_UNDEFINED
            } else {
                TypeFacts::NE_UNDEFINED
            };
            return self.get_adjusted_type_with_facts(ty, facts);
        }
        if assume_true {
            let unknown_or_empty = self.tables.flags_of(ty).intersects(TypeFlags::UNKNOWN)
                || self.some_type_result(ty, |state, t| state.is_empty_anonymous_object_type(t))?;
            if !double_equals && unknown_or_empty {
                if value_flags.intersects(TypeFlags::from_bits(
                    TypeFlags::PRIMITIVE.bits() | TypeFlags::NON_PRIMITIVE.bits(),
                )) || self.is_empty_anonymous_object_type(value_type)?
                {
                    return Ok(value_type);
                }
                if value_flags.intersects(TypeFlags::OBJECT) {
                    return Ok(self.tables.intrinsics.non_primitive);
                }
            }
            let filtered = self.filter_type_with(ty, |state, t| {
                if state.are_types_comparable(t, value_type)? {
                    return Ok(true);
                }
                Ok(double_equals && state.is_coercible_under_double_equals(t, value_type))
            })?;
            return self.replace_primitives_with_literals(filtered, value_type);
        }
        if self.is_unit_type(value_type) {
            return self.filter_type_with(ty, |state, t| {
                Ok(!(state.is_unit_like_type(t)? && state.are_types_comparable(t, value_type)?))
            });
        }
        Ok(ty)
    }

    /// tsc-port: narrowTypeByOptionalChainContainment @6.0.3
    /// tsc-hash: 240760f13bf2c98cb8c9ff35216729b5397eb972239e14c8a61eb3a3e371d51c
    /// tsc-span: _tsc.js:71041-71047
    ///
    /// `x?.y === v` where the reference is x: the chain root sheds
    /// undefined|null when the comparison's outcome proves the chain
    /// ran (a nullish comparand on the false-ish face, a
    /// definitely-non-nullish one on the true-ish face).
    fn narrow_type_by_optional_chain_containment(
        &mut self,
        ty: TypeId,
        operator: SyntaxKind,
        value: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let equals_operator = operator == SyntaxKind::EqualsEqualsToken
            || operator == SyntaxKind::EqualsEqualsEqualsToken;
        let nullable_flags = if operator == SyntaxKind::EqualsEqualsToken
            || operator == SyntaxKind::ExclamationEqualsToken
        {
            TypeFlags::NULLABLE
        } else {
            TypeFlags::UNDEFINED
        };
        let value_type = self.get_type_of_expression(value)?;
        let remove_nullable = (equals_operator != assume_true
            && self.tables.every_type(value_type, |tables, t| {
                tables.flags_of(t).intersects(nullable_flags)
            }))
            || (equals_operator == assume_true
                && self.tables.every_type(value_type, |tables, t| {
                    !tables.flags_of(t).intersects(TypeFlags::from_bits(
                        TypeFlags::ANY_OR_UNKNOWN.bits() | nullable_flags.bits(),
                    ))
                }));
        if remove_nullable {
            self.get_adjusted_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: narrowTypeByDiscriminantProperty @6.0.3
    /// tsc-hash: 9ae07c740e9e7b94f514c2a3823c07a6e679d284600f9e5fb41e8602fd294f15
    /// tsc-span: _tsc.js:70833-70844
    ///
    /// Strict (in)equality on a union's KEY property takes the
    /// constituent-map fast path (getKeyPropertyName ≥10-member
    /// machinery, live since M3); everything else funnels through
    /// narrowTypeByDiscriminant with an equality filter.
    fn narrow_type_by_discriminant_property(
        &mut self,
        ty: TypeId,
        access: NodeId,
        operator: SyntaxKind,
        value: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        if matches!(
            operator,
            SyntaxKind::EqualsEqualsEqualsToken | SyntaxKind::ExclamationEqualsEqualsToken
        ) && self.tables.flags_of(ty).intersects(TypeFlags::UNION)
        {
            if let Some(key_property_name) = self.get_key_property_name(ty)? {
                if Some(&key_property_name) == self.get_accessed_property_name(access)?.as_ref() {
                    let value_type = self.get_type_of_expression(value)?;
                    if let Some(candidate) =
                        self.get_constituent_type_for_key_type(ty, value_type)?
                    {
                        let equality_matches = operator
                            == if assume_true {
                                SyntaxKind::EqualsEqualsEqualsToken
                            } else {
                                SyntaxKind::ExclamationEqualsEqualsToken
                            };
                        if equality_matches {
                            return Ok(candidate);
                        }
                        let key_type = self
                            .get_type_of_property_of_type_full(candidate, &key_property_name)?
                            .unwrap_or(self.tables.intrinsics.unknown);
                        if self.is_unit_type(key_type) {
                            return Ok(self.tables.filter_type(ty, |_, t| t != candidate));
                        }
                        return Ok(ty);
                    }
                }
            }
        }
        self.narrow_type_by_discriminant(ty, access, |state, t| {
            state.narrow_type_by_equality(t, operator, value, assume_true)
        })
    }

    /// tsc-port: isTypePresencePossible @6.0.3
    /// tsc-hash: 9b3a45915ce986a841949427850985e8e6974bf65c9f0a8250f37e83fd29aead
    /// tsc-span: _tsc.js:70868-70871
    fn is_type_presence_possible(
        &mut self,
        ty: TypeId,
        prop_name: &str,
        assume_true: bool,
    ) -> CheckResult2<bool> {
        if let Some(prop) = self.get_property_of_type_full(ty, prop_name)? {
            let optional = self
                .binder
                .symbol(prop)
                .flags
                .intersects(tsrs2_types::SymbolFlags::OPTIONAL)
                || self
                    .get_check_flags(prop)
                    .intersects(tsrs2_types::CheckFlags::PARTIAL);
            return Ok(optional || assume_true);
        }
        if self
            .get_applicable_index_info_for_name(ty, prop_name)?
            .is_some()
        {
            return Ok(true);
        }
        Ok(!assume_true)
    }

    /// tsc-port: narrowTypeByInKeyword @6.0.3
    /// tsc-hash: 4717c268e5c0cd51ebcd75b337c673676e7a830a1364dc5f95506f95e6ea2719
    /// tsc-span: _tsc.js:70872-70890
    ///
    /// `"p" in x`: members where the property's presence is possible
    /// survive the filter; a name known to NO member intersects with
    /// `Record<name, unknown>` on the true branch instead (lib-less
    /// programs skip the widening — the alias lookup is silent, like
    /// the facts family's NonNullable miss).
    fn narrow_type_by_in_keyword(
        &mut self,
        ty: TypeId,
        name_type: TypeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let Some(name) = self.get_property_name_from_type(name_type) else {
            return Ok(ty);
        };
        let is_known_property = self.some_type_result(ty, |state, t| {
            state.is_type_presence_possible(t, &name, /*assume_true*/ true)
        })?;
        if is_known_property {
            return self.filter_type_with(ty, |state, t| {
                state.is_type_presence_possible(t, &name, assume_true)
            });
        }
        if assume_true {
            if let Some(record_symbol) = self.get_global_record_symbol() {
                let unknown = self.tables.intrinsics.unknown;
                let record = self.get_type_alias_instantiation(
                    record_symbol,
                    Some(&[name_type, unknown]),
                    None,
                    None,
                )?;
                return self
                    .get_intersection_type(&[ty, record], tsrs2_types::IntersectionFlags::NONE);
            }
        }
        Ok(ty)
    }

    /// tsc-port: getGlobalRecordSymbol @6.0.3
    /// tsc-hash: 7aeb5eb6fcfaffaf11a794c57c77bdf8458500f50959f897f035229637562cce
    /// tsc-span: _tsc.js:61016-61025
    ///
    /// Silent lookup (tsc reports 2318 through this path in lib-less
    /// programs; the narrowing consumer keeps the miss quiet like the
    /// facts family's NonNullable — divergence bounded to no-lib
    /// corpus rows that use `in` widening).
    fn get_global_record_symbol(&mut self) -> Option<SymbolId> {
        if self.deferred_global_record_symbol.is_none() {
            let symbol = self.get_global_symbol("Record", SymbolFlags::TYPE_ALIAS, None);
            self.deferred_global_record_symbol = Some(symbol);
        }
        self.deferred_global_record_symbol.expect("memoized above")
    }

    /// tsc-port: narrowTypeByPrivateIdentifierInInExpression @6.0.3
    /// tsc-hash: 791045c52f97ccc8d44d856c23a3482da01437f31f5a2d335a99020c70d766c9
    /// tsc-span: _tsc.js:71021-71040
    ///
    /// `#field in x` narrows to (or away from) the field's class —
    /// the static form uses the class constructor's type, the
    /// instance form the declared instance type; derived classes
    /// count (checkDerived).
    fn narrow_type_by_private_identifier_in_in_expression(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let (left, right) = match self.data_of(expr) {
            NodeData::BinaryExpression(data) => (data.left, data.right),
            _ => (None, None),
        };
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(ty);
        };
        let target = self.get_reference_candidate(right);
        if !self.is_matching_query_reference(query, target)? {
            return Ok(ty);
        }
        let Some(symbol) = self.get_symbol_for_private_identifier_expression(left)? else {
            return Ok(ty);
        };
        let Some(class_symbol) = self.binder.symbol(symbol).parent else {
            return Ok(ty);
        };
        let is_static = self
            .binder
            .symbol(symbol)
            .value_declaration
            .is_some_and(|declaration| self.has_static_modifier(declaration));
        let target_type = if is_static {
            self.get_type_of_symbol(class_symbol)?
        } else {
            self.get_declared_type_of_symbol_slice(class_symbol)?
        };
        self.get_narrowed_type(ty, target_type, assume_true, /*check_derived*/ true)
    }

    /// tsc-port: isMatchingConstructorReference @6.0.3
    /// tsc-hash: 73c918bf08a645c3c3c7dc2c55a4cf33b9327dc9316682c9efec496b38adc21c
    /// tsc-span: _tsc.js:71228-71230
    fn is_matching_constructor_reference(
        &mut self,
        query: &mut FlowQuery,
        expr: NodeId,
    ) -> CheckResult2<bool> {
        let is_constructor_access = match self.data_of(expr) {
            NodeData::PropertyAccessExpression(data) => {
                let name = data.name;
                self.escaped_text_of(name) == Some("constructor")
            }
            NodeData::ElementAccessExpression(data) => {
                let argument = data.argument_expression;
                argument.is_some_and(|argument| {
                    self.is_string_literal_like(argument)
                        && self.string_literal_text(argument).as_deref() == Some("constructor")
                })
            }
            _ => false,
        };
        if !is_constructor_access {
            return Ok(false);
        }
        let receiver = match self.data_of(expr) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        match receiver {
            Some(receiver) => self.is_matching_query_reference(query, receiver),
            None => Ok(false),
        }
    }

    /// tsc-port: narrowTypeByConstructor @6.0.3
    /// tsc-hash: c9ce900fc29d0d4bb236d3dc25f50a3b1384b62a362f37883c9dc4d0f457adec
    /// tsc-span: _tsc.js:71231-71258
    ///
    /// `x.constructor === C` keeps members constructed by C: classes
    /// compare by symbol identity (no derived-class adoption — that
    /// is instanceof's job), everything else by subtype against C's
    /// prototype type.
    fn narrow_type_by_constructor(
        &mut self,
        ty: TypeId,
        operator: SyntaxKind,
        identifier: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let polarity_matches = if assume_true {
            matches!(
                operator,
                SyntaxKind::EqualsEqualsToken | SyntaxKind::EqualsEqualsEqualsToken
            )
        } else {
            matches!(
                operator,
                SyntaxKind::ExclamationEqualsToken | SyntaxKind::ExclamationEqualsEqualsToken
            )
        };
        if !polarity_matches {
            return Ok(ty);
        }
        let identifier_type = self.get_type_of_expression(identifier)?;
        if !self.is_function_type(identifier_type)? && !self.is_constructor_type(identifier_type)? {
            return Ok(ty);
        }
        let Some(prototype_property) =
            self.get_property_of_type_full(identifier_type, "prototype")?
        else {
            return Ok(ty);
        };
        let prototype_type = self.get_type_of_symbol(prototype_property)?;
        if self
            .tables
            .flags_of(prototype_type)
            .intersects(TypeFlags::ANY)
        {
            return Ok(ty);
        }
        let candidate = prototype_type;
        let global_object = self.global_object_type()?;
        let global_function = self.global_function_type()?;
        if candidate == global_object || candidate == global_function {
            return Ok(ty);
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok(candidate);
        }
        self.filter_type_with(ty, |state, t| state.is_constructed_by(t, candidate))
    }

    /// The inner isConstructedBy of narrowTypeByConstructor (71487-
    /// 71492 closure): class-vs-class is symbol identity; otherwise
    /// subtype.
    /// tsrs-native: extracted inner closure (same tsc span).
    fn is_constructed_by(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        let source_is_class = self.tables.flags_of(source).intersects(TypeFlags::OBJECT)
            && self
                .tables
                .object_flags_of(source)
                .intersects(tsrs2_types::ObjectFlags::CLASS);
        let target_is_class = self.tables.flags_of(target).intersects(TypeFlags::OBJECT)
            && self
                .tables
                .object_flags_of(target)
                .intersects(tsrs2_types::ObjectFlags::CLASS);
        if source_is_class || target_is_class {
            return Ok(self.tables.type_of(source).symbol == self.tables.type_of(target).symbol);
        }
        self.is_type_subtype_of(source, target)
    }

    /// tsc-port: narrowTypeByInstanceof @6.0.3
    /// tsc-hash: 8035e3178ea954c51ea663da404ef8586c661bbac52bef3ea41ce263a29afc9a
    /// tsc-span: _tsc.js:71259-71297
    ///
    /// The `[Symbol.hasInstance]` PREDICATE consult
    /// (getEffectsSignature + getTypePredicateOfSignature) is 6.4f
    /// machinery: until it lands, a right operand whose hasInstance
    /// method syntactically declares a type-predicate return FLAGS
    /// the query (tsc would narrow through the predicate; ordinary
    /// constructors — incl. the lib `Function[Symbol.hasInstance]`
    /// returning boolean — have no predicate in tsc either and take
    /// the prototype path exactly).
    fn narrow_type_by_instanceof(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let (left, right) = match self.data_of(expr) {
            NodeData::BinaryExpression(data) => (data.left, data.right),
            _ => (None, None),
        };
        let (Some(expr_left), Some(expr_right)) = (left, right) else {
            return Ok(ty);
        };
        let left = self.get_reference_candidate(expr_left);
        if !self.is_matching_query_reference(query, left)? {
            if assume_true
                && self
                    .options
                    .strict_option_value(self.options.strict_null_checks)
                && self.optional_chain_contains_query_reference(left, query)?
            {
                return self.get_adjusted_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL);
            }
            return Ok(ty);
        }
        let right_type = self.get_type_of_expression(expr_right)?;
        let global_object = self.global_object_type()?;
        if !self.is_type_derived_from(right_type, global_object)? {
            return Ok(ty);
        }
        if self.has_instance_predicate_signature(right_type)? {
            // [FLOW 6.4f] the predicate-based narrowing is
            // unreproducible until effects signatures land — flag.
            query.traversed_inert_arm = true;
            return Ok(ty);
        }
        let global_function = self.global_function_type()?;
        if !self.is_type_derived_from(right_type, global_function)? {
            return Ok(ty);
        }
        let instance_type = self
            .map_type(
                right_type,
                &mut |state, t| state.get_instance_type(t).map(Some),
                false,
            )?
            .expect("mapper is total");
        let ty_is_any = self.tables.flags_of(ty).intersects(TypeFlags::ANY);
        let instance_not_narrowable = !self
            .tables
            .flags_of(instance_type)
            .intersects(TypeFlags::OBJECT)
            || self.is_empty_anonymous_object_type(instance_type)?;
        if (ty_is_any && (instance_type == global_object || instance_type == global_function))
            || (!assume_true && instance_not_narrowable)
        {
            return Ok(ty);
        }
        self.get_narrowed_type(ty, instance_type, assume_true, /*check_derived*/ true)
    }

    /// The 6.4c stand-in for the instanceof effects-signature
    /// consult: does the right type carry a [Symbol.hasInstance]
    /// method whose declaration syntactically returns a type
    /// predicate? (tsc narrows through the RESOLVED predicate; a
    /// syntactic probe over the call signatures' declared return
    /// types is the conservative superset — over-flagging degrades
    /// to the declared type, never misnarrows.)
    /// tsrs-native: temporary 6.4f gate (retires with
    /// narrowTypeByCallExpression's getEffectsSignature).
    fn has_instance_predicate_signature(&mut self, right_type: TypeId) -> CheckResult2<bool> {
        let Some(has_instance_type) =
            self.get_symbol_has_instance_method_of_object_type(right_type)?
        else {
            return Ok(false);
        };
        let apparent = self.get_apparent_type(has_instance_type)?;
        let signatures =
            self.get_signatures_of_type(apparent, crate::structural::SignatureKind::Call)?;
        for signature in signatures {
            let Some(declaration) = self.signature_of(signature).declaration else {
                continue;
            };
            let return_type_node = self.effective_return_type_node(declaration);
            if return_type_node.is_some_and(|node| self.kind_of(node) == SyntaxKind::TypePredicate)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: getInstanceType @6.0.3
    /// tsc-hash: 4bcb16660db689a30da8ad07250d4b02ac115580e3aff1be34af752dcbae5c41
    /// tsc-span: _tsc.js:71298-71308
    fn get_instance_type(&mut self, constructor_type: TypeId) -> CheckResult2<TypeId> {
        let prototype_property_type =
            self.get_type_of_property_of_type_full(constructor_type, "prototype")?;
        if let Some(prototype_property_type) = prototype_property_type {
            if !self
                .tables
                .flags_of(prototype_property_type)
                .intersects(TypeFlags::ANY)
            {
                return Ok(prototype_property_type);
            }
        }
        let construct_signatures = self.get_signatures_of_type(
            constructor_type,
            crate::structural::SignatureKind::Construct,
        )?;
        if !construct_signatures.is_empty() {
            let mut return_types = Vec::with_capacity(construct_signatures.len());
            for signature in construct_signatures {
                let erased = self.get_erased_signature(signature)?;
                return_types.push(self.get_return_type_of_signature(erased)?);
            }
            return self.get_union_type_ex(&return_types, tsrs2_types::UnionReduction::Literal);
        }
        Ok(self.empty_object_type)
    }

    /// tsc-port: getNarrowedType @6.0.3
    /// tsc-hash: cd81e867aae688b9c71b965e7934618a4278022accf3c58423a44196eaeb0ba0
    /// tsc-span: _tsc.js:71309-71312
    ///
    /// The union-input memo (getCachedType `N` key).
    pub(crate) fn get_narrowed_type(
        &mut self,
        ty: TypeId,
        candidate: TypeId,
        assume_true: bool,
        check_derived: bool,
    ) -> CheckResult2<TypeId> {
        let key = if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            Some(format!(
                "N{},{},{}",
                ty.0,
                candidate.0,
                (assume_true as u8) | ((check_derived as u8) << 1)
            ))
        } else {
            None
        };
        if let Some(key) = &key {
            if let Some(&cached) = self.cached_types.get(key) {
                return Ok(cached);
            }
        }
        let result = self.get_narrowed_type_worker(ty, candidate, assume_true, check_derived)?;
        if let Some(key) = key {
            self.cached_types.insert(key, result);
        }
        Ok(result)
    }

    /// tsc-port: getNarrowedTypeWorker @6.0.3
    /// tsc-hash: 9b77db50e49f6a486dd3087bae26d98a1ea9019ec06589442976e46152c2f75e
    /// tsc-span: _tsc.js:71313-71350
    ///
    /// The shared instanceof/predicate narrowing core: the false
    /// branch removes candidate-related members (derived check or
    /// true-branch-subset removal with unknown decomposition); the
    /// true branch maps candidate constituents through the
    /// key-property fast path, direct (strict-)subtype relations in
    /// both directions, and the instantiable-constraint intersection
    /// fallback, then falls back to the assignability ladder.
    fn get_narrowed_type_worker(
        &mut self,
        ty: TypeId,
        candidate: TypeId,
        assume_true: bool,
        check_derived: bool,
    ) -> CheckResult2<TypeId> {
        if !assume_true {
            if ty == candidate {
                return Ok(self.tables.intrinsics.never);
            }
            if check_derived {
                return self.filter_type_with(ty, |state, t| {
                    Ok(!state.is_type_derived_from(t, candidate)?)
                });
            }
            let ty = if self.tables.flags_of(ty).intersects(TypeFlags::UNKNOWN) {
                self.unknown_union_type
            } else {
                ty
            };
            let true_type =
                self.get_narrowed_type(ty, candidate, /*assume_true*/ true, false)?;
            let filtered =
                self.filter_type_with(ty, |state, t| Ok(!state.is_type_subset_of(t, true_type)?))?;
            return Ok(self.recombine_unknown_type(filtered));
        }
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::ANY_OR_UNKNOWN)
        {
            return Ok(candidate);
        }
        if ty == candidate {
            return Ok(candidate);
        }
        let key_property_name = if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            self.get_key_property_name(ty)?
        } else {
            None
        };
        let narrowed_type = self
            .map_type(
                candidate,
                &mut |state, c| {
                    let discriminant = match &key_property_name {
                        Some(name) => state.get_type_of_property_of_type_full(c, name)?,
                        None => None,
                    };
                    let matching = match discriminant {
                        Some(discriminant) => {
                            state.get_constituent_type_for_key_type(ty, discriminant)?
                        }
                        None => None,
                    };
                    let map_input = matching.unwrap_or(ty);
                    let directly_related = state
                        .map_type(
                            map_input,
                            &mut |state, t| {
                                let related = if check_derived {
                                    if state.is_type_derived_from(t, c)? {
                                        t
                                    } else if state.is_type_derived_from(c, t)? {
                                        c
                                    } else {
                                        state.tables.intrinsics.never
                                    }
                                } else if state.is_type_strict_subtype_of(t, c)? {
                                    t
                                } else if state.is_type_strict_subtype_of(c, t)? {
                                    c
                                } else if state.is_type_subtype_of(t, c)? {
                                    t
                                } else if state.is_type_subtype_of(c, t)? {
                                    c
                                } else {
                                    state.tables.intrinsics.never
                                };
                                Ok(Some(related))
                            },
                            false,
                        )?
                        .expect("mapper is total");
                    if !state
                        .tables
                        .flags_of(directly_related)
                        .intersects(TypeFlags::NEVER)
                    {
                        return Ok(Some(directly_related));
                    }
                    Ok(Some(
                        state
                            .map_type(
                                ty,
                                &mut |state, t| {
                                    if !state.maybe_type_of_kind(t, TypeFlags::INSTANTIABLE) {
                                        return Ok(Some(state.tables.intrinsics.never));
                                    }
                                    let constraint = state
                                        .get_base_constraint_of_type(t)?
                                        .unwrap_or(state.tables.intrinsics.unknown);
                                    let is_related = if check_derived {
                                        state.is_type_derived_from(c, constraint)?
                                    } else {
                                        state.is_type_subtype_of(c, constraint)?
                                    };
                                    if is_related {
                                        state
                                            .get_intersection_type(
                                                &[t, c],
                                                tsrs2_types::IntersectionFlags::NONE,
                                            )
                                            .map(Some)
                                    } else {
                                        Ok(Some(state.tables.intrinsics.never))
                                    }
                                },
                                false,
                            )?
                            .expect("mapper is total"),
                    ))
                },
                false,
            )?
            .expect("mapper is total");
        if !self
            .tables
            .flags_of(narrowed_type)
            .intersects(TypeFlags::NEVER)
        {
            return Ok(narrowed_type);
        }
        if self.is_type_subtype_of(candidate, ty)? {
            return Ok(candidate);
        }
        if self.is_type_assignable_to(ty, candidate)? {
            return Ok(ty);
        }
        if self.is_type_assignable_to(candidate, ty)? {
            return Ok(candidate);
        }
        self.get_intersection_type(&[ty, candidate], tsrs2_types::IntersectionFlags::NONE)
    }

    /// tsc-port: isTypeDerivedFrom @6.0.3
    /// tsc-hash: 42ebab631024cb6d6ec18ad1f419900110e7f6ca78f8cadc014576260770c365
    /// tsc-span: _tsc.js:63922-63924
    pub(crate) fn is_type_derived_from(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        let source_flags = self.tables.flags_of(source);
        if source_flags.intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(source).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            for member in members {
                if !self.is_type_derived_from(member, target)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        if self.tables.flags_of(target).intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(target).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            for member in members {
                if self.is_type_derived_from(source, member)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if source_flags.intersects(TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(source).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            for member in members {
                if self.is_type_derived_from(member, target)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if source_flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE) {
            let constraint = self
                .get_base_constraint_of_type(source)?
                .unwrap_or(self.tables.intrinsics.unknown);
            return self.is_type_derived_from(constraint, target);
        }
        if self.is_empty_anonymous_object_type(target)? {
            return Ok(source_flags.intersects(TypeFlags::from_bits(
                TypeFlags::OBJECT.bits() | TypeFlags::NON_PRIMITIVE.bits(),
            )));
        }
        let global_object = self.global_object_type()?;
        if target == global_object {
            return Ok(source_flags.intersects(TypeFlags::from_bits(
                TypeFlags::OBJECT.bits() | TypeFlags::NON_PRIMITIVE.bits(),
            )) && !self.is_empty_anonymous_object_type(source)?);
        }
        let global_function = self.global_function_type()?;
        if target == global_function {
            return Ok(source_flags.intersects(TypeFlags::OBJECT)
                && self.is_function_object_type(source)?);
        }
        let target_target = self.get_target_type(target);
        if self.has_base_type(source, target_target)? {
            return Ok(true);
        }
        if self.is_array_type(target)? && !self.is_readonly_array_type(target)? {
            let global_readonly_array = self.global_readonly_array_type()?;
            return self.is_type_derived_from(source, global_readonly_array);
        }
        Ok(false)
    }

    /// tsc-port: isFunctionType @6.0.3
    /// tsc-hash: b60f6a37b9534a340f8dc339732079eb9e6ec6fea78009af067b9886d4fc609c
    /// tsc-span: _tsc.js:88268-88270
    fn is_function_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(false);
        }
        Ok(!self
            .get_signatures_of_type(ty, crate::structural::SignatureKind::Call)?
            .is_empty())
    }

    /// tsc-port: isCoercibleUnderDoubleEquals @6.0.3
    /// tsc-hash: d0e7d95e972f0f2a6087ffb5cd7a1401b4cfbabe7ddd08184d68fb4f28fba272
    /// tsc-span: _tsc.js:67892-67894
    fn is_coercible_under_double_equals(&self, source: TypeId, target: TypeId) -> bool {
        self.tables
            .flags_of(source)
            .intersects(TypeFlags::from_bits(
                TypeFlags::NUMBER.bits()
                    | TypeFlags::STRING.bits()
                    | TypeFlags::BOOLEAN_LITERAL.bits(),
            ))
            && self
                .tables
                .flags_of(target)
                .intersects(TypeFlags::from_bits(
                    TypeFlags::NUMBER.bits() | TypeFlags::STRING.bits() | TypeFlags::BOOLEAN.bits(),
                ))
    }

    /// tsc-port: isUnitLikeType @6.0.3
    /// tsc-hash: 8572e35e5cc67db52dbbe0d85e6631b810647c6ea6b8d410e344666c6995ed4a
    /// tsc-span: _tsc.js:67745-67748
    fn is_unit_like_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let t = self.get_base_constraint_or_type(ty)?;
        if self.tables.flags_of(t).intersects(TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(t).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            return Ok(members.iter().any(|&member| self.is_unit_type(member)));
        }
        Ok(self.is_unit_type(t))
    }

    /// tsc-port: extractTypesOfKind @6.0.3
    /// tsc-hash: 2d55adac617ba5027e0c686a7d22bc8f6bc1d791602099402633dbfd80569e42
    /// tsc-span: _tsc.js:70055-70057
    fn extract_types_of_kind(&mut self, ty: TypeId, kind: TypeFlags) -> TypeId {
        self.tables
            .filter_type(ty, |tables, t| tables.flags_of(t).intersects(kind))
    }

    /// tsc-port: replacePrimitivesWithLiterals @6.0.3
    /// tsc-hash: e3c8ad48183e6d9361032f95016372375abf55de29bbd69984d29b191cadeaef
    /// tsc-span: _tsc.js:70058-70063
    ///
    /// `x === "a"` on x: string keeps the literal: primitive members
    /// of the narrowed side are replaced by the comparand's matching
    /// literal kinds (pattern literals collapse to string literals
    /// when the comparand has no string/template side).
    fn replace_primitives_with_literals(
        &mut self,
        type_with_primitives: TypeId,
        type_with_literals: TypeId,
    ) -> CheckResult2<TypeId> {
        let primitives_possible = self.maybe_type_of_kind(
            type_with_primitives,
            TypeFlags::from_bits(
                TypeFlags::STRING.bits()
                    | TypeFlags::TEMPLATE_LITERAL.bits()
                    | TypeFlags::NUMBER.bits()
                    | TypeFlags::BIG_INT.bits(),
            ),
        );
        let literals_possible = self.maybe_type_of_kind(
            type_with_literals,
            TypeFlags::from_bits(
                TypeFlags::STRING_LITERAL.bits()
                    | TypeFlags::TEMPLATE_LITERAL.bits()
                    | TypeFlags::STRING_MAPPING.bits()
                    | TypeFlags::NUMBER_LITERAL.bits()
                    | TypeFlags::BIG_INT_LITERAL.bits(),
            ),
        );
        if !(primitives_possible && literals_possible) {
            return Ok(type_with_primitives);
        }
        Ok(self
            .map_type(
                type_with_primitives,
                &mut |state, t| {
                    let flags = state.tables.flags_of(t);
                    Ok(Some(if flags.intersects(TypeFlags::STRING) {
                        state.extract_types_of_kind(
                            type_with_literals,
                            TypeFlags::from_bits(
                                TypeFlags::STRING.bits()
                                    | TypeFlags::STRING_LITERAL.bits()
                                    | TypeFlags::TEMPLATE_LITERAL.bits()
                                    | TypeFlags::STRING_MAPPING.bits(),
                            ),
                        )
                    } else if state.tables.is_pattern_literal_type(t)
                        && !state.maybe_type_of_kind(
                            type_with_literals,
                            TypeFlags::from_bits(
                                TypeFlags::STRING.bits()
                                    | TypeFlags::TEMPLATE_LITERAL.bits()
                                    | TypeFlags::STRING_MAPPING.bits(),
                            ),
                        )
                    {
                        state.extract_types_of_kind(type_with_literals, TypeFlags::STRING_LITERAL)
                    } else if flags.intersects(TypeFlags::NUMBER) {
                        state.extract_types_of_kind(
                            type_with_literals,
                            TypeFlags::from_bits(
                                TypeFlags::NUMBER.bits() | TypeFlags::NUMBER_LITERAL.bits(),
                            ),
                        )
                    } else if flags.intersects(TypeFlags::BIG_INT) {
                        state.extract_types_of_kind(
                            type_with_literals,
                            TypeFlags::from_bits(
                                TypeFlags::BIG_INT.bits() | TypeFlags::BIG_INT_LITERAL.bits(),
                            ),
                        )
                    } else {
                        t
                    }))
                },
                false,
            )?
            .expect("mapper is total"))
    }

    /// tsc isStringLiteralLike (StringLiteral |
    /// NoSubstitutionTemplateLiteral).
    /// tsrs-native: kind probe (utilities one-liner).
    fn is_string_literal_like(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
        )
    }

    /// tsc isBooleanLiteral (TrueKeyword | FalseKeyword).
    /// tsrs-native: kind probe (utilities one-liner).
    fn is_boolean_literal(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword
        )
    }

    /// tsc isTypeUsableAsPropertyName (StringOrNumberLiteralOrUnique).
    /// tsrs-native: flags probe (checker one-liner).
    fn is_type_usable_as_property_name(&self, ty: TypeId) -> bool {
        self.tables
            .flags_of(ty)
            .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE)
    }

    /// tsc getPropertyNameFromType over the usable-as-property-name
    /// gate (the callers check first, so the miss arm is dead).
    /// tsrs-native: delegate to the ported tryGetNameFromType.
    fn get_property_name_from_type(&self, ty: TypeId) -> Option<String> {
        self.try_get_name_from_type(ty)
    }

    /// The literal text of a string-literal-like node.
    /// tsrs-native: NodeData accessor.
    fn string_literal_text(&self, node: NodeId) -> Option<String> {
        match self.data_of(node) {
            NodeData::StringLiteral(data) => Some(data.text.clone()),
            NodeData::NoSubstitutionTemplateLiteral(data) => Some(data.text.clone()),
            _ => None,
        }
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

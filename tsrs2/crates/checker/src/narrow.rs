//! M5 6.4: the narrowers — narrowType dispatch + sub-narrowers
//! (m5-flow-steps.md stage 6.4; checker-key-functions.md §4.5).
//!
//! tsc SHAPE FACT (70766-71460): narrowType and every sub-narrower
//! are CLOSURES inside getFlowTypeOfReference, reading the query's
//! `reference` binding; the port threads the same [`FlowQuery`] the
//! walk carries. `inlineLevel` alone is checker state (tsc 46453).
//!
//! Stage state (6.4 COMPLETE): every sub-narrower is live —
//! truthiness (+ the destructuring synthetic-reference entry),
//! equality/binary (instanceof, in, .constructor, boolean-literal,
//! aliased &&/||), typeof, the switch family (+ exhaustiveness,
//! pulled forward from 6.6), effects signatures + type predicates +
//! the call arm, optionality, and the const-variable guard inlining.
//! The query flag (`FlowQuery::traversed_inert_arm`) survives as the
//! narrow M6-DEFERRAL channel: the TS 5.5 body-inference predicate
//! precondition (getTypePredicateFromBody, flagged at
//! get_effects_signature — the uncertain no-effects verdict is never
//! memoized), the synthetic-reference generic-union-constraint
//! guard, and parser-recovery shapes. The [FLOW M5] failure-face
//! gates retire at 6.6 (they still shield the reachability
//! true-stub's dead-code divergence — m5-flow-steps.md 6.4 landing
//! note).

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{CheckMode, SymbolFlags, TypeData, TypeFacts, TypeFlags, TypeId};

use crate::flow::FlowQuery;
use crate::state::{CheckResult2, CheckerState, SignatureId, Unsupported};

impl<'a> CheckerState<'a> {
    /// The narrow-family caches (switch types, exhaustiveness,
    /// effects signatures, resolved type predicates) are state-side
    /// stand-ins for tsc links/signature fields and follow the links
    /// write discipline (greenfield §4.3, links.rs assert_writable):
    /// stable verdicts only, never written during speculation. Inert
    /// while speculation_depth is constant 0 — the M6 speculation
    /// transaction must trip HERE, not silently memoize a
    /// speculative value.
    fn assert_narrow_cache_writable(&self) {
        assert_eq!(
            self.speculation_depth, 0,
            "narrow-cache writes are forbidden during speculation (greenfield §4.3)"
        );
    }

    /// tsc-port: narrowType @6.0.3
    /// tsc-hash: 27ad08800fadb3f5234051fdb384cffe54850849b9ba6eca2c94f97e7979a821
    /// tsc-span: _tsc.js:71400-71439
    ///
    /// The dispatch (checker-key §4.5): optional-chain roots and
    /// `??`/`??=` left operands divert to optionality narrowing before
    /// the kind switch; parenthesized/nonnull/satisfies wrappers and
    /// `!` recurse; the Identifier arm inlines const-variable guards
    /// (`if (c)` where `const c = <guard>`, inlineLevel < 5, constant
    /// reference); kinds outside the switch narrow nothing (tsc
    /// returns the type unchanged — real semantics, not a stub).
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
                                    if let (false, Some(initializer)) = (annotated, initializer) {
                                        // tsc's && chain consults
                                        // isConstantReference LAST — its
                                        // binding-pattern arm is the
                                        // Unsupported escape, so reaching it
                                        // for annotated/initializer-less
                                        // consts would widen that channel
                                        // beyond tsc. A synthetic
                                        // destructuring reference never
                                        // inlines: the factory node carries
                                        // no resolvedSymbol, so tsc's access
                                        // arm is isReadonlySymbol(
                                        // unknownSymbol) = false (70385).
                                        let constant_reference = if query.synthetic_props.is_some()
                                        {
                                            false
                                        } else {
                                            self.is_constant_reference(query.reference)?
                                        };
                                        if constant_reference {
                                            // The const-variable guard INLINING
                                            // (live since 6.4h): narrow through
                                            // the const's initializer at the
                                            // bumped inlineLevel; the
                                            // recursion's answer IS the arm's
                                            // answer.
                                            self.inline_level += 1;
                                            let result = self.narrow_type(
                                                query,
                                                ty,
                                                initializer,
                                                assume_true,
                                            );
                                            self.inline_level -= 1;
                                            return result;
                                        }
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
    pub(crate) fn get_discriminant_property_access(
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

    /// tsc-port: narrowTypeByTypeof @6.0.3
    /// tsc-hash: 861a7384f0a0ca7e4843027df07ce42e1e88c9232ee15255bf4d2091984d648c
    /// tsc-span: _tsc.js:71081-71097
    ///
    /// `typeof x === "s"`: a matching operand narrows by the literal;
    /// a non-matching one still strips undefined|null when the
    /// reference sits inside an optional chain of it (strict, and the
    /// comparison proves the chain ran), then tries the
    /// discriminant-property path with the literal filter.
    fn narrow_type_by_typeof(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        typeof_expr: NodeId,
        operator: SyntaxKind,
        literal: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let assume_true = if operator == SyntaxKind::ExclamationEqualsToken
            || operator == SyntaxKind::ExclamationEqualsEqualsToken
        {
            !assume_true
        } else {
            assume_true
        };
        let operand = match self.data_of(typeof_expr) {
            NodeData::TypeOfExpression(data) => data.expression,
            _ => None,
        };
        let Some(operand) = operand else {
            query.traversed_inert_arm = true;
            return Ok(ty);
        };
        let Some(literal_text) = self.string_literal_text(literal) else {
            query.traversed_inert_arm = true;
            return Ok(ty);
        };
        let target = self.get_reference_candidate(operand);
        if !self.is_matching_query_reference(query, target)? {
            let mut ty = ty;
            if self
                .options
                .strict_option_value(self.options.strict_null_checks)
                && self.optional_chain_contains_query_reference(target, query)?
                && assume_true == (literal_text != "undefined")
            {
                ty = self.get_adjusted_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)?;
            }
            if let Some(access) = self.get_discriminant_property_access(query, target, ty)? {
                return self.narrow_type_by_discriminant(ty, access, |state, t| {
                    state.narrow_type_by_literal_expression(t, &literal_text, assume_true)
                });
            }
            return Ok(ty);
        }
        self.narrow_type_by_literal_expression(ty, &literal_text, assume_true)
    }

    /// tsc-port: narrowTypeByLiteralExpression @6.0.3
    /// tsc-hash: 21e339349711249f6b2d83a76c2cc2e8af3d4c9f4fabfe6bcea2ecf782856925
    /// tsc-span: _tsc.js:71098-71100
    fn narrow_type_by_literal_expression(
        &mut self,
        ty: TypeId,
        literal_text: &str,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        if assume_true {
            self.narrow_type_by_type_name(ty, literal_text)
        } else {
            let facts = typeof_ne_facts(literal_text).unwrap_or(TypeFacts::TYPEOF_NE_HOST_OBJECT);
            self.get_adjusted_type_with_facts(ty, facts)
        }
    }

    /// tsc-port: narrowTypeByTypeName @6.0.3
    /// tsc-hash: 29c46fe87223a05c3cfdc6b4df1ff9e99d1a501589ebe39e9f2c362e6b1aa17a
    /// tsc-span: _tsc.js:71139-71159
    fn narrow_type_by_type_name(&mut self, ty: TypeId, type_name: &str) -> CheckResult2<TypeId> {
        let intrinsics = &self.tables.intrinsics;
        let (implied, facts) = match type_name {
            "string" => (intrinsics.string, TypeFacts::TYPEOF_EQ_STRING),
            "number" => (intrinsics.number, TypeFacts::TYPEOF_EQ_NUMBER),
            "bigint" => (intrinsics.bigint, TypeFacts::TYPEOF_EQ_BIG_INT),
            "boolean" => (intrinsics.boolean, TypeFacts::TYPEOF_EQ_BOOLEAN),
            "symbol" => (intrinsics.es_symbol, TypeFacts::TYPEOF_EQ_SYMBOL),
            "object" => {
                if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
                    return Ok(ty);
                }
                let non_primitive = self.tables.intrinsics.non_primitive;
                let null = self.tables.intrinsics.null;
                let object_side =
                    self.narrow_type_by_type_facts(ty, non_primitive, TypeFacts::TYPEOF_EQ_OBJECT)?;
                let null_side = self.narrow_type_by_type_facts(ty, null, TypeFacts::EQ_NULL)?;
                return self.get_union_type_ex(
                    &[object_side, null_side],
                    tsrs2_types::UnionReduction::Literal,
                );
            }
            "function" => {
                if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
                    return Ok(ty);
                }
                let global_function = self.global_function_type()?;
                return self.narrow_type_by_type_facts(
                    ty,
                    global_function,
                    TypeFacts::TYPEOF_EQ_FUNCTION,
                );
            }
            "undefined" => (intrinsics.undefined, TypeFacts::EQ_UNDEFINED),
            _ => {
                let non_primitive = self.tables.intrinsics.non_primitive;
                return self.narrow_type_by_type_facts(
                    ty,
                    non_primitive,
                    TypeFacts::TYPEOF_EQ_HOST_OBJECT,
                );
            }
        };
        self.narrow_type_by_type_facts(ty, implied, facts)
    }

    /// tsc-port: narrowTypeByTypeFacts @6.0.3
    /// tsc-hash: c15871d7bcc1743a8154807aab765fb80998c0618528d0464deb51fe3a4f6dd5
    /// tsc-span: _tsc.js:71160-71177
    ///
    /// Per member: a strict subtype of the implied type keeps or dies
    /// by its facts (strict because `object` <: `{}`, and function
    /// types are `object` subtypes but typeof-classify as
    /// "function"); a supertype of the implied type is replaced by it
    /// (unknown/`{}`/toString-ish supertypes); overlapping domains
    /// (unconstrained type params vs string) intersect when the facts
    /// allow, else die.
    fn narrow_type_by_type_facts(
        &mut self,
        ty: TypeId,
        implied_type: TypeId,
        facts: TypeFacts,
    ) -> CheckResult2<TypeId> {
        Ok(self
            .map_type(
                ty,
                &mut |state, t| {
                    if state.is_type_strict_subtype_of(t, implied_type)? {
                        return Ok(Some(if state.has_type_facts(t, facts)? {
                            t
                        } else {
                            state.tables.intrinsics.never
                        }));
                    }
                    if state.is_type_subtype_of(implied_type, t)? {
                        return Ok(Some(implied_type));
                    }
                    if state.has_type_facts(t, facts)? {
                        state
                            .get_intersection_type(
                                &[t, implied_type],
                                tsrs2_types::IntersectionFlags::NONE,
                            )
                            .map(Some)
                    } else {
                        Ok(Some(state.tables.intrinsics.never))
                    }
                },
                false,
            )?
            .expect("mapper is total"))
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
            if let Some(record_symbol) = self.get_global_record_symbol()? {
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
    /// getGlobalTypeAliasSymbol("Record", 2, reportErrors=true): a
    /// noLib miss reports the locationless 2318, a non-alias or
    /// wrong-arity global Record reports 2317 and skips the widening
    /// — each once (the memo holds the unknownSymbol verdict; an
    /// Unsupported unwind stays unmemoized).
    fn get_global_record_symbol(&mut self) -> CheckResult2<Option<SymbolId>> {
        if let Some(memo) = self.deferred_global_record_symbol {
            return Ok(memo);
        }
        let symbol = self.get_global_type_alias_symbol("Record", 2, /*report_errors*/ true)?;
        self.deferred_global_record_symbol = Some(symbol);
        Ok(symbol)
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

    /// The inner isConstructedBy of narrowTypeByConstructor (71252-
    /// 71257 closure): class-vs-class is symbol identity; otherwise
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
    /// The `[Symbol.hasInstance]` PREDICATE consult is LIVE (6.4f):
    /// get_effects_signature resolves the hasInstance method through
    /// its BinaryExpression arm and a first-parameter identifier
    /// predicate narrows via getNarrowedType(checkDerived) — only the
    /// body-inference candidate family still flags there (recorded
    /// M6 deferral). Ordinary constructors — incl. the lib
    /// `Function[Symbol.hasInstance]` returning boolean — have no
    /// predicate in tsc either and take the prototype path exactly.
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
        let signature = self.get_effects_signature(Some(query), expr)?;
        let predicate = match signature {
            Some(signature) => self.get_type_predicate_of_signature(signature)?,
            None => None,
        };
        if let Some(predicate) = predicate {
            if predicate.kind == TypePredicateKind::Identifier && predicate.parameter_index == 0 {
                if let Some(predicate_type) = predicate.ty {
                    return self.get_narrowed_type(
                        ty,
                        predicate_type,
                        assume_true,
                        /*check_derived*/ true,
                    );
                }
            }
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

    /// tsc-port: getTypeOfSwitchClause @6.0.3
    /// tsc-hash: 885262335dc0056f9bc30ef213774cdcfc8140e7a931d31853ea544c4a582ac4
    /// tsc-span: _tsc.js:69932-69936
    fn get_type_of_switch_clause(&mut self, clause: NodeId) -> CheckResult2<TypeId> {
        if self.kind_of(clause) == SyntaxKind::CaseClause {
            let expression = match self.data_of(clause) {
                NodeData::CaseClause(data) => data.expression,
                _ => None,
            };
            if let Some(expression) = expression {
                let ty = self.get_type_of_expression(expression)?;
                return Ok(self.tables.get_regular_type_of_literal_type(ty));
            }
        }
        Ok(self.tables.intrinsics.never)
    }

    /// tsc-port: getSwitchClauseTypes @6.0.3
    /// tsc-hash: 783f50b40607b75acd60e220899c0a66e7630d5e0291ae05c70311c80d867862
    /// tsc-span: _tsc.js:69937-69947
    ///
    /// links.switchTypes lives state-side (see switch_types_cache);
    /// written only on full success so an Unsupported unwind
    /// recomputes.
    pub(crate) fn get_switch_clause_types(
        &mut self,
        switch_statement: NodeId,
    ) -> CheckResult2<Vec<TypeId>> {
        if let Some(cached) = self.switch_types_cache.get(&switch_statement) {
            return Ok(cached.clone());
        }
        let clauses = self.switch_clauses(switch_statement);
        let mut types = Vec::with_capacity(clauses.len());
        for clause in clauses {
            types.push(self.get_type_of_switch_clause(clause)?);
        }
        self.assert_narrow_cache_writable();
        self.switch_types_cache
            .insert(switch_statement, types.clone());
        Ok(types)
    }

    /// tsc-port: getSwitchClauseTypeOfWitnesses @6.0.3
    /// tsc-hash: da25d6f411fad510614461965d2844b36e4b28030ad9c56998d102a22499a4b6
    /// tsc-span: _tsc.js:69948-69958
    ///
    /// None when any case expression is not a string literal; a
    /// per-clause witness list otherwise (default clauses, DUPLICATE
    /// texts, and the falsy EMPTY text witness None).
    pub(crate) fn get_switch_clause_type_of_witnesses(
        &mut self,
        switch_statement: NodeId,
    ) -> Option<Vec<Option<String>>> {
        let clauses = self.switch_clauses(switch_statement);
        for &clause in &clauses {
            if self.kind_of(clause) == SyntaxKind::CaseClause {
                let expression = match self.data_of(clause) {
                    NodeData::CaseClause(data) => data.expression,
                    _ => None,
                };
                match expression {
                    Some(expression) if self.is_string_literal_like(expression) => {}
                    _ => return None,
                }
            }
        }
        let mut witnesses: Vec<Option<String>> = Vec::with_capacity(clauses.len());
        for clause in clauses {
            let text = if self.kind_of(clause) == SyntaxKind::CaseClause {
                let expression = match self.data_of(clause) {
                    NodeData::CaseClause(data) => data.expression,
                    _ => None,
                };
                expression.and_then(|expression| self.string_literal_text(expression))
            } else {
                None
            };
            let witness = match text {
                // tsc 69955 `text && !contains(...)`: the EMPTY case
                // text (`case "":`) is falsy and witnesses None, like
                // a default clause.
                Some(text) if !text.is_empty() && !witnesses.contains(&Some(text.clone())) => {
                    Some(text)
                }
                _ => None,
            };
            witnesses.push(witness);
        }
        Some(witnesses)
    }

    /// The clauses of a switch statement's case block.
    /// tsrs-native: NodeData accessor.
    fn switch_clauses(&self, switch_statement: NodeId) -> Vec<NodeId> {
        let case_block = match self.data_of(switch_statement) {
            NodeData::SwitchStatement(data) => data.case_block,
            _ => None,
        };
        let clauses = case_block.and_then(|case_block| match self.data_of(case_block) {
            NodeData::CaseBlock(data) => data.clauses,
            _ => None,
        });
        match clauses {
            Some(clauses) => self.binder.node_array(clauses).nodes.clone(),
            None => Vec::new(),
        }
    }

    /// tsc-port: eachTypeContainedIn @6.0.3
    /// tsc-hash: 38e89d7291b09cb24e59a43f0f002576f5b902e58fd4e66495da58d848b2fd47
    /// tsc-span: _tsc.js:69959-69961
    fn each_type_contained_in(&self, source: TypeId, types: &[TypeId]) -> bool {
        if self.tables.flags_of(source).intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(source).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            members.iter().all(|member| types.contains(member))
        } else {
            types.contains(&source)
        }
    }

    /// tsc-port: narrowTypeBySwitchOptionalChainContainment @6.0.3
    /// tsc-hash: ab5c9fd5a3c3bcabb45e4b4bf7efbd808b141c67013303cbd0bad4a539a43b0d
    /// tsc-span: _tsc.js:71101-71104
    pub(crate) fn narrow_type_by_switch_optional_chain_containment(
        &mut self,
        ty: TypeId,
        switch_statement: NodeId,
        clause_start: usize,
        clause_end: usize,
        clause_check: impl Fn(&Self, TypeId) -> bool,
    ) -> CheckResult2<TypeId> {
        let every_clause_checks = clause_start != clause_end && {
            let clause_types = self.get_switch_clause_types(switch_statement)?;
            clause_types[clause_start.min(clause_types.len())..clause_end.min(clause_types.len())]
                .iter()
                .all(|&t| clause_check(self, t))
        };
        if every_clause_checks {
            self.get_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: narrowTypeBySwitchOnDiscriminant @6.0.3
    /// tsc-hash: 18138a5989c8f9f7e1f8bc309f39907467fdd6e9812a9053230a8de101e2693d
    /// tsc-span: _tsc.js:71105-71138
    ///
    /// The switch-discriminant workhorse: unknown operands ground to
    /// the clause types (objects widened to nonPrimitive) when no
    /// default is in range; otherwise the comparable filter against
    /// the clause union (re-literalized), plus the default-clause
    /// complement that removes unit-like members hit by OTHER clauses.
    pub(crate) fn narrow_type_by_switch_on_discriminant(
        &mut self,
        ty: TypeId,
        switch_statement: NodeId,
        clause_start: usize,
        clause_end: usize,
    ) -> CheckResult2<TypeId> {
        let switch_types = self.get_switch_clause_types(switch_statement)?;
        if switch_types.is_empty() {
            return Ok(ty);
        }
        let clause_types =
            &switch_types[clause_start.min(switch_types.len())..clause_end.min(switch_types.len())];
        let never = self.tables.intrinsics.never;
        let has_default_clause = clause_start == clause_end || clause_types.contains(&never);
        if self.tables.flags_of(ty).intersects(TypeFlags::UNKNOWN) && !has_default_clause {
            let mut ground_clause_types: Option<Vec<TypeId>> = None;
            for (index, &t) in clause_types.iter().enumerate() {
                if self.tables.flags_of(t).intersects(TypeFlags::from_bits(
                    TypeFlags::PRIMITIVE.bits() | TypeFlags::NON_PRIMITIVE.bits(),
                )) {
                    if let Some(ground) = &mut ground_clause_types {
                        ground.push(t);
                    }
                } else if self.tables.flags_of(t).intersects(TypeFlags::OBJECT) {
                    let ground =
                        ground_clause_types.get_or_insert_with(|| clause_types[..index].to_vec());
                    ground.push(self.tables.intrinsics.non_primitive);
                } else {
                    return Ok(ty);
                }
            }
            let members = ground_clause_types.unwrap_or_else(|| clause_types.to_vec());
            return self.get_union_type_ex(&members, tsrs2_types::UnionReduction::Literal);
        }
        let discriminant_type =
            self.get_union_type_ex(clause_types, tsrs2_types::UnionReduction::Literal)?;
        let case_type = if self
            .tables
            .flags_of(discriminant_type)
            .intersects(TypeFlags::NEVER)
        {
            never
        } else {
            let filtered = self.filter_type_with(ty, |state, t| {
                state.are_types_comparable(discriminant_type, t)
            })?;
            self.replace_primitives_with_literals(filtered, discriminant_type)?
        };
        if !has_default_clause {
            return Ok(case_type);
        }
        let undefined = self.tables.intrinsics.undefined;
        let default_type = self.filter_type_with(ty, |state, t| {
            if !state.is_unit_like_type(t)? {
                return Ok(true);
            }
            let key = if state.tables.flags_of(t).intersects(TypeFlags::UNDEFINED) {
                undefined
            } else {
                let unit = state.extract_unit_type(t);
                state.tables.get_regular_type_of_literal_type(unit)
            };
            for &switch_type in &switch_types {
                if state.is_unit_type(switch_type)
                    && state.are_types_comparable(switch_type, key)?
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })?;
        if self.tables.flags_of(case_type).intersects(TypeFlags::NEVER) {
            Ok(default_type)
        } else {
            self.get_union_type_ex(
                &[case_type, default_type],
                tsrs2_types::UnionReduction::Literal,
            )
        }
    }

    /// tsc-port: extractUnitType @6.0.3
    /// tsc-hash: f5761c88def06490ad19b8cebf96d50a41b9a9df246fd236ed74316ade1b3016
    /// tsc-span: _tsc.js:67749-67751
    fn extract_unit_type(&self, ty: TypeId) -> TypeId {
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            if let TypeData::Intersection { types } = &self.tables.type_of(ty).data {
                for &member in types.iter() {
                    if self.is_unit_type(member) {
                        return member;
                    }
                }
            }
        }
        ty
    }

    /// tsc-port: narrowTypeBySwitchOnDiscriminantProperty @6.0.3
    /// tsc-hash: 10f991ce9ccf53a4cb15559594206fa4462dc57edffad1cf022ec2b402ca0c4a
    /// tsc-span: _tsc.js:70845-70854
    pub(crate) fn narrow_type_by_switch_on_discriminant_property(
        &mut self,
        ty: TypeId,
        access: NodeId,
        switch_statement: NodeId,
        clause_start: usize,
        clause_end: usize,
    ) -> CheckResult2<TypeId> {
        if clause_start < clause_end && self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let key_property_name = self.get_key_property_name(ty)?;
            if key_property_name.is_some()
                && key_property_name == self.get_accessed_property_name(access)?
            {
                let clause_types = {
                    let all = self.get_switch_clause_types(switch_statement)?;
                    all[clause_start.min(all.len())..clause_end.min(all.len())].to_vec()
                };
                let unknown = self.tables.intrinsics.unknown;
                let mut constituents = Vec::with_capacity(clause_types.len());
                for clause_type in clause_types {
                    let constituent = self
                        .get_constituent_type_for_key_type(ty, clause_type)?
                        .unwrap_or(unknown);
                    constituents.push(constituent);
                }
                let candidate =
                    self.get_union_type_ex(&constituents, tsrs2_types::UnionReduction::Literal)?;
                if candidate != unknown {
                    return Ok(candidate);
                }
            }
        }
        self.narrow_type_by_discriminant(ty, access, |state, t| {
            state.narrow_type_by_switch_on_discriminant(
                t,
                switch_statement,
                clause_start,
                clause_end,
            )
        })
    }

    /// tsc-port: getNotEqualFactsFromTypeofSwitch @6.0.3
    /// tsc-hash: d13615ff6fb7e7c3f5a60d7bc914e80ee8b4bd02b6d033abbab65c5fd6c26349
    /// tsc-span: _tsc.js:78912-78919
    fn get_not_equal_facts_from_typeof_switch(
        &self,
        start: usize,
        end: usize,
        witnesses: &[Option<String>],
    ) -> TypeFacts {
        let mut facts = TypeFacts::NONE;
        for (index, witness) in witnesses.iter().enumerate() {
            let witness = if index < start || index >= end {
                witness.as_deref()
            } else {
                None
            };
            if let Some(witness) = witness {
                facts |= typeof_ne_facts(witness).unwrap_or(TypeFacts::TYPEOF_NE_HOST_OBJECT);
            }
        }
        facts
    }

    /// tsc-port: narrowTypeBySwitchOnTypeOf @6.0.3
    /// tsc-hash: 62c9cbae314bc4eec676e93aa275920fad2d2cfe544133b8f0429ce58063a6ff
    /// tsc-span: _tsc.js:71178-71191
    pub(crate) fn narrow_type_by_switch_on_type_of(
        &mut self,
        ty: TypeId,
        switch_statement: NodeId,
        clause_start: usize,
        clause_end: usize,
    ) -> CheckResult2<TypeId> {
        let Some(witnesses) = self.get_switch_clause_type_of_witnesses(switch_statement) else {
            return Ok(ty);
        };
        let clauses = self.switch_clauses(switch_statement);
        let default_index = clauses
            .iter()
            .position(|&clause| self.kind_of(clause) == SyntaxKind::DefaultClause);
        let has_default_clause = clause_start == clause_end
            || default_index.is_some_and(|index| index >= clause_start && index < clause_end);
        if has_default_clause {
            let not_equal_facts =
                self.get_not_equal_facts_from_typeof_switch(clause_start, clause_end, &witnesses);
            return self.filter_type_with(ty, |state, t| {
                Ok(state.get_type_facts(t, not_equal_facts)? == not_equal_facts)
            });
        }
        let clause_witnesses =
            &witnesses[clause_start.min(witnesses.len())..clause_end.min(witnesses.len())];
        let mut members = Vec::with_capacity(clause_witnesses.len());
        for witness in clause_witnesses {
            members.push(match witness {
                Some(text) => {
                    let text = text.clone();
                    self.narrow_type_by_type_name(ty, &text)?
                }
                None => self.tables.intrinsics.never,
            });
        }
        self.get_union_type_ex(&members, tsrs2_types::UnionReduction::Literal)
    }

    /// tsc-port: narrowTypeBySwitchOnTrue @6.0.3
    /// tsc-hash: 138036cce7a43cd3cdfa47338c94beda7828824a47683ead033b186010af6e24
    /// tsc-span: _tsc.js:71192-71227
    ///
    /// `switch (true)`: clauses BEFORE the range narrow false; with a
    /// default in range, clauses AFTER narrow false too; otherwise
    /// the union of the in-range clauses each narrowed true.
    pub(crate) fn narrow_type_by_switch_on_true(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        switch_statement: NodeId,
        clause_start: usize,
        clause_end: usize,
    ) -> CheckResult2<TypeId> {
        let clauses = self.switch_clauses(switch_statement);
        let default_index = clauses
            .iter()
            .position(|&clause| self.kind_of(clause) == SyntaxKind::DefaultClause);
        let has_default_clause = clause_start == clause_end
            || default_index.is_some_and(|index| index >= clause_start && index < clause_end);
        let mut ty = ty;
        for &clause in clauses.iter().take(clause_start) {
            if self.kind_of(clause) == SyntaxKind::CaseClause {
                let expression = match self.data_of(clause) {
                    NodeData::CaseClause(data) => data.expression,
                    _ => None,
                };
                if let Some(expression) = expression {
                    ty = self.narrow_type(query, ty, expression, false)?;
                }
            }
        }
        if has_default_clause {
            for &clause in clauses.iter().skip(clause_end) {
                if self.kind_of(clause) == SyntaxKind::CaseClause {
                    let expression = match self.data_of(clause) {
                        NodeData::CaseClause(data) => data.expression,
                        _ => None,
                    };
                    if let Some(expression) = expression {
                        ty = self.narrow_type(query, ty, expression, false)?;
                    }
                }
            }
            return Ok(ty);
        }
        let range = &clauses[clause_start.min(clauses.len())..clause_end.min(clauses.len())];
        let mut members = Vec::with_capacity(range.len());
        for &clause in range {
            members.push(if self.kind_of(clause) == SyntaxKind::CaseClause {
                let expression = match self.data_of(clause) {
                    NodeData::CaseClause(data) => data.expression,
                    _ => None,
                };
                match expression {
                    Some(expression) => self.narrow_type(query, ty, expression, true)?,
                    None => self.tables.intrinsics.never,
                }
            } else {
                self.tables.intrinsics.never
            });
        }
        self.get_union_type_ex(&members, tsrs2_types::UnionReduction::Literal)
    }

    /// tsc-port: isExhaustiveSwitchStatement @6.0.3
    /// tsc-hash: 4632c64320e9d000229f241d8f97b9a633e9eea87743424785d86276544531b8
    /// tsc-span: _tsc.js:78920-78932
    ///
    /// PULLED FORWARD from the 6.6 slot: the 6.3 branch-label bypass
    /// consult was unobservable while the switch-clause arm reverted
    /// every crossing query; 6.4e makes the arm live, so the
    /// conservative-false stub would over-widen exhaustive-switch
    /// joins (an FP face). The remaining 6.6 consumers (unreachable
    /// code, implicit returns) stay 6.6. links.isExhaustive lives
    /// state-side; a re-entrant computation settles FALSE (the
    /// links.isExhaustive === 0 cycle protocol).
    pub(crate) fn is_exhaustive_switch_statement_real(
        &mut self,
        switch_statement: NodeId,
    ) -> CheckResult2<bool> {
        if let Some(&cached) = self.exhaustive_switch_cache.get(&switch_statement) {
            return Ok(cached);
        }
        if self.exhaustive_switch_computing.contains(&switch_statement) {
            self.assert_narrow_cache_writable();
            self.exhaustive_switch_cache.insert(switch_statement, false);
            return Ok(false);
        }
        self.exhaustive_switch_computing.insert(switch_statement);
        let computed = self.compute_exhaustive_switch_statement(switch_statement);
        self.exhaustive_switch_computing.remove(&switch_statement);
        let computed = computed?;
        self.assert_narrow_cache_writable();
        let result = *self
            .exhaustive_switch_cache
            .entry(switch_statement)
            .or_insert(computed);
        Ok(result)
    }

    /// tsc-port: computeExhaustiveSwitchStatement @6.0.3
    /// tsc-hash: da5a07c386f5757a1953c5ba1346eee199f6d58a93e0eed75c50bd19cc937ed7
    /// tsc-span: _tsc.js:78933-78955
    fn compute_exhaustive_switch_statement(
        &mut self,
        switch_statement: NodeId,
    ) -> CheckResult2<bool> {
        let expression = match self.data_of(switch_statement) {
            NodeData::SwitchStatement(data) => data.expression,
            _ => None,
        };
        let Some(expression) = expression else {
            return Ok(false);
        };
        if self.kind_of(expression) == SyntaxKind::TypeOfExpression {
            let Some(witnesses) = self.get_switch_clause_type_of_witnesses(switch_statement) else {
                return Ok(false);
            };
            let operand = match self.data_of(expression) {
                NodeData::TypeOfExpression(data) => data.expression,
                _ => None,
            };
            let Some(operand) = operand else {
                return Ok(false);
            };
            let operand_type = self.check_expression_cached(operand, CheckMode::NORMAL)?;
            let operand_constraint = self.get_base_constraint_or_type(operand_type)?;
            let not_equal_facts = self.get_not_equal_facts_from_typeof_switch(0, 0, &witnesses);
            if self
                .tables
                .flags_of(operand_constraint)
                .intersects(TypeFlags::ANY_OR_UNKNOWN)
            {
                let all = TypeFacts::ALL_TYPEOF_NE;
                return Ok(TypeFacts::from_bits(all.bits() & not_equal_facts.bits()) == all);
            }
            return Ok(!self.some_type_result(operand_constraint, |state, t| {
                Ok(state.get_type_facts(t, not_equal_facts)? == not_equal_facts)
            })?);
        }
        let expression_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
        let ty = self.get_base_constraint_or_type(expression_type)?;
        if !self.is_literal_type(ty) {
            return Ok(false);
        }
        let switch_types = self.get_switch_clause_types(switch_statement)?;
        if switch_types.is_empty() {
            return Ok(false);
        }
        for &switch_type in &switch_types {
            let never = self
                .tables
                .flags_of(switch_type)
                .intersects(TypeFlags::NEVER);
            if !self.is_unit_type(switch_type) && !never {
                return Ok(false);
            }
        }
        let mapped = self
            .map_type(
                ty,
                &mut |state, t| Ok(Some(state.tables.get_regular_type_of_literal_type(t))),
                false,
            )?
            .expect("mapper is total");
        Ok(self.each_type_contained_in(mapped, &switch_types))
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

    /// tsc-port: narrowTypeByCallExpression @6.0.3
    /// tsc-hash: 20a4999b28c285ad522470273a870cc8cb07e37f9c4d1bd09d0aa288dd8de23f
    /// tsc-span: _tsc.js:71351-71369
    ///
    /// A call with a matching argument consults its effects signature
    /// for a this/identifier predicate; separately, the
    /// `x.hasOwnProperty("p")` facts arm applies to missing-type
    /// access references.
    fn narrow_type_by_call_expression(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        call_expression: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        if self.has_matching_argument(query, call_expression)? {
            let source = self.binder.source_of_node(call_expression);
            let is_call_chain = node_util::is_optional_chain(source, call_expression);
            let signature = if assume_true || !is_call_chain {
                self.get_effects_signature(Some(query), call_expression)?
            } else {
                None
            };
            if let Some(signature) = signature {
                let predicate = self.get_type_predicate_of_signature(signature)?;
                if let Some(predicate) = predicate {
                    if matches!(
                        predicate.kind,
                        TypePredicateKind::This | TypePredicateKind::Identifier
                    ) {
                        return self.narrow_type_by_type_predicate(
                            query,
                            ty,
                            &predicate,
                            call_expression,
                            assume_true,
                        );
                    }
                }
            }
        }
        if self.contains_missing_type(ty) && self.query_reference_is_access(query) {
            let callee = match self.data_of(call_expression) {
                NodeData::CallExpression(data) => data.expression,
                _ => None,
            };
            if let Some(callee) = callee {
                if self.kind_of(callee) == SyntaxKind::PropertyAccessExpression {
                    let (callee_receiver, callee_name) = match self.data_of(callee) {
                        NodeData::PropertyAccessExpression(data) => (data.expression, data.name),
                        _ => (None, None),
                    };
                    let arguments = match self.data_of(call_expression) {
                        NodeData::CallExpression(data) => data.arguments,
                        _ => None,
                    };
                    let arguments: Vec<NodeId> = arguments
                        .map(|arguments| self.binder.node_array(arguments).nodes.clone())
                        .unwrap_or_default();
                    if let Some(callee_receiver) = callee_receiver {
                        let candidate = self.get_reference_candidate(callee_receiver);
                        if self.query_reference_receiver_matches(query, candidate)?
                            && self.escaped_text_of(callee_name) == Some("hasOwnProperty")
                            && arguments.len() == 1
                            && self.is_string_literal_like(arguments[0])
                        {
                            let argument_text = self.string_literal_text(arguments[0]);
                            let reference_name =
                                self.query_reference_accessed_property_name(query)?;
                            if reference_name.is_some()
                                && reference_name
                                    == argument_text
                                        .map(|text| tsrs2_syntax::escape_leading_underscores(&text))
                            {
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
                }
            }
        }
        Ok(ty)
    }

    /// tsc-port: narrowTypeByTypePredicate @6.0.3
    /// tsc-hash: d834c36c0a182f4d20f8ab06111ad6096a30a04fedce5efce70534d1f192a968
    /// tsc-span: _tsc.js:71370-71399
    pub(crate) fn narrow_type_by_type_predicate(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        predicate: &TypePredicate,
        call_expression: NodeId,
        assume_true: bool,
    ) -> CheckResult2<TypeId> {
        let Some(predicate_type) = predicate.ty else {
            return Ok(ty);
        };
        let global_object = self.global_object_type()?;
        let global_function = self.global_function_type()?;
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY)
            && (predicate_type == global_object || predicate_type == global_function)
        {
            return Ok(ty);
        }
        let Some(predicate_argument) = self.get_type_predicate_argument(predicate, call_expression)
        else {
            return Ok(ty);
        };
        if self.is_matching_query_reference(query, predicate_argument)? {
            return self.get_narrowed_type(ty, predicate_type, assume_true, false);
        }
        let mut ty = ty;
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        if strict_null_checks
            && self.optional_chain_contains_query_reference(predicate_argument, query)?
        {
            let strip = if assume_true {
                !self.has_type_facts(predicate_type, TypeFacts::EQ_UNDEFINED)?
            } else {
                let mut all_nullable = true;
                let members: Vec<TypeId> = if self
                    .tables
                    .flags_of(predicate_type)
                    .intersects(TypeFlags::UNION)
                {
                    match &self.tables.type_of(predicate_type).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        _ => unreachable!("union flag implies union data"),
                    }
                } else {
                    vec![predicate_type]
                };
                for member in members {
                    if !self.is_nullable_type(member)? {
                        all_nullable = false;
                        break;
                    }
                }
                all_nullable
            };
            if strip {
                ty = self.get_adjusted_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)?;
            }
        }
        if let Some(access) =
            self.get_discriminant_property_access(query, predicate_argument, ty)?
        {
            return self.narrow_type_by_discriminant(ty, access, |state, t| {
                state.get_narrowed_type(t, predicate_type, assume_true, false)
            });
        }
        Ok(ty)
    }

    /// tsc-port: narrowTypeByAssertion @6.0.3
    /// tsc-hash: f5fe27f124d8565cfe13ce6c3004a15ff860098f0d7add8381ef8c820884325e
    /// tsc-span: _tsc.js:70542-70565
    ///
    /// `asserts x` with a bare condition argument: false literals
    /// make the continuation unreachable; &&/|| recurse; everything
    /// else narrows true.
    pub(crate) fn narrow_type_by_assertion(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
    ) -> CheckResult2<TypeId> {
        let node = self.skip_parentheses(expr);
        if self.kind_of(node) == SyntaxKind::FalseKeyword {
            return Ok(self.tables.intrinsics.unreachable_never);
        }
        if self.kind_of(node) == SyntaxKind::BinaryExpression {
            let (left, operator_token, right) = match self.data_of(node) {
                NodeData::BinaryExpression(data) => (data.left, data.operator_token, data.right),
                _ => (None, None, None),
            };
            if let (Some(left), Some(operator_token), Some(right)) = (left, operator_token, right) {
                match self.kind_of(operator_token) {
                    SyntaxKind::AmpersandAmpersandToken => {
                        let narrowed = self.narrow_type_by_assertion(query, ty, left)?;
                        return self.narrow_type_by_assertion(query, narrowed, right);
                    }
                    SyntaxKind::BarBarToken => {
                        let left_narrowed = self.narrow_type_by_assertion(query, ty, left)?;
                        let right_narrowed = self.narrow_type_by_assertion(query, ty, right)?;
                        return self.get_union_type_ex(
                            &[left_narrowed, right_narrowed],
                            tsrs2_types::UnionReduction::Literal,
                        );
                    }
                    _ => {}
                }
            }
        }
        self.narrow_type(query, ty, node, true)
    }

    /// tsc-port: hasMatchingArgument @6.0.3
    /// tsc-hash: 62e36f5de476aec1cb78fd1c0a1747c9aa7d29c411df0363fc3b09ab4888ffe1
    /// tsc-span: _tsc.js:69644-69656
    fn has_matching_argument(
        &mut self,
        query: &FlowQuery,
        expression: NodeId,
    ) -> CheckResult2<bool> {
        let (callee, arguments) = match self.data_of(expression) {
            NodeData::CallExpression(data) => (data.expression, data.arguments),
            _ => (None, None),
        };
        let arguments: Vec<NodeId> = arguments
            .map(|arguments| self.binder.node_array(arguments).nodes.clone())
            .unwrap_or_default();
        for argument in arguments {
            if self.is_matching_query_reference(query, argument)?
                || self.contains_matching_query_reference(query, argument)?
                || self.optional_chain_contains_query_reference(argument, query)?
            {
                return Ok(true);
            }
        }
        if let Some(callee) = callee {
            if self.kind_of(callee) == SyntaxKind::PropertyAccessExpression {
                let receiver = match self.data_of(callee) {
                    NodeData::PropertyAccessExpression(data) => data.expression,
                    _ => None,
                };
                if let Some(receiver) = receiver {
                    if self.is_matching_query_reference(query, receiver)?
                        || self.contains_matching_query_reference(query, receiver)?
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: getTypePredicateArgument @6.0.3
    /// tsc-hash: 4f754a799784bbbbf763fae79dadffab5714969bdb8e4b88c87e44babe0415ad
    /// tsc-span: _tsc.js:70227-70233
    fn get_type_predicate_argument(
        &self,
        predicate: &TypePredicate,
        call_expression: NodeId,
    ) -> Option<NodeId> {
        if matches!(
            predicate.kind,
            TypePredicateKind::Identifier | TypePredicateKind::AssertsIdentifier
        ) {
            let arguments = match self.data_of(call_expression) {
                NodeData::CallExpression(data) => data.arguments,
                _ => None,
            }?;
            let nodes = &self.binder.node_array(arguments).nodes;
            let index = usize::try_from(predicate.parameter_index).ok()?;
            return nodes.get(index).copied();
        }
        let callee = match self.data_of(call_expression) {
            NodeData::CallExpression(data) => data.expression,
            _ => None,
        }?;
        let invoked = self.skip_parentheses(callee);
        let receiver = match self.data_of(invoked) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        }?;
        Some(self.skip_parentheses(receiver))
    }

    /// tsc-port: getEffectsSignature @6.0.3
    /// tsc-hash: d1f2f2cc1da46e57b21e670fbcbc943d53197784afedc259759faf408d802e08
    /// tsc-span: _tsc.js:70194-70223
    ///
    /// links.effectsSignature lives state-side (None = the memoized
    /// unknownSignature verdict). A signature that could carry a
    /// BODY-INFERRED predicate (TS 5.5 getTypePredicateFromBody — an
    /// M6-adjacent unported family: annotation-free normal function
    /// with parameters and a `boolean` return) FLAGS the query — in
    /// the some() sweep when no definite member decides the
    /// selection, and on the selected candidate — because tsc might
    /// resolve a predicate there that we cannot, and the pass-through
    /// answer would be over-wide.
    ///
    /// `query: None` is the reachability walk (isReachableFlowNodeWorker
    /// 70279 — 6.6): there is no flag channel there, so the uncertain
    /// verdict Errs instead — a "reachable" answer computed past an
    /// undecided asserts-false/never candidate could surface as a
    /// 2534/2366-family FP once the dead-code gates are gone, and an
    /// Err also keeps both reachability caches unwritten (the memo
    /// outlives the walk; an unflagged undecided verdict must not).
    pub(crate) fn get_effects_signature(
        &mut self,
        mut query: Option<&mut FlowQuery>,
        node: NodeId,
    ) -> CheckResult2<Option<SignatureId>> {
        if let Some(&cached) = self.effects_signature_cache.get(&node) {
            // Every cached entry passed the body-inference probes
            // below at insert time (uncertain verdicts return early,
            // unmemoized), so a hit needs no re-probe.
            return Ok(cached);
        }
        let func_type: Option<TypeId> = if self.kind_of(node) == SyntaxKind::BinaryExpression {
            let right = match self.data_of(node) {
                NodeData::BinaryExpression(data) => data.right,
                _ => None,
            };
            match right {
                Some(right) => {
                    let right_type = self.check_non_null_expression(right)?;
                    self.get_symbol_has_instance_method_of_object_type(right_type)?
                }
                None => None,
            }
        } else {
            let parent_is_expression_statement = self
                .parent_of(node)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ExpressionStatement);
            let callee = match self.data_of(node) {
                NodeData::CallExpression(data) => data.expression,
                _ => None,
            };
            match callee {
                Some(callee) if parent_is_expression_statement => {
                    self.get_type_of_dotted_name(callee)?
                }
                Some(callee) if self.kind_of(callee) != SyntaxKind::SuperKeyword => {
                    let source = self.binder.source_of_node(node);
                    if node_util::is_optional_chain(source, node) {
                        let checked = self.check_expression(callee, CheckMode::NORMAL)?;
                        let optional = self.get_optional_expression_type(checked, callee)?;
                        Some(self.check_non_null_type(optional, callee)?)
                    } else {
                        Some(self.check_non_null_expression(callee)?)
                    }
                }
                _ => None,
            }
        };
        let apparent = match func_type {
            Some(func_type) => self.get_apparent_type(func_type)?,
            None => self.tables.intrinsics.unknown,
        };
        let signatures =
            self.get_signatures_of_type(apparent, crate::structural::SignatureKind::Call)?;
        let candidate = if signatures.len() == 1
            && self.signature_of(signatures[0]).type_parameters.is_none()
        {
            Some(signatures[0])
        } else {
            // tsc's `some(signatures, hasTypePredicateOrNeverReturnType)`
            // reaches getTypePredicateOfSignature's body-inference arm
            // PER MEMBER: a definite predicate/never member decides
            // some() = true regardless of the uncertain ones, but with
            // no definite member a body-inference candidate could flip
            // the verdict — the selection itself is then unreproducible.
            let mut any_effects = false;
            let mut any_uncertain = false;
            for &signature in &signatures {
                if self.has_type_predicate_or_never_return_type(signature)? {
                    any_effects = true;
                    break;
                }
                if !any_uncertain {
                    any_uncertain = self.signature_may_have_body_inferred_predicate(signature)?;
                }
            }
            if any_effects {
                Some(self.get_resolved_signature(node, CheckMode::NORMAL)?)
            } else {
                if any_uncertain {
                    let Some(query) = query.as_deref_mut() else {
                        return Err(Unsupported::new(
                            "body-inferred type predicate candidate in a reachability \
                             effects consult (getTypePredicateFromBody, M6)",
                        ));
                    };
                    query.traversed_inert_arm = true;
                    return Ok(None);
                }
                None
            }
        };
        let mut result = None;
        if let Some(candidate) = candidate {
            if self.signature_may_have_body_inferred_predicate(candidate)? {
                // tsc would run getTypePredicateFromBody here and may
                // find a predicate (even alongside a composite verdict
                // whose members we resolved without body inference) —
                // unreproducible until the family ports; flag
                // (declared-type revert, FP-safe) and DO NOT memoize:
                // the verdict is not final (a memo hit would skip this
                // probe and leak the unflagged wide answer — caught
                // live by the loop fixpoint pin).
                let Some(query) = query else {
                    return Err(Unsupported::new(
                        "body-inferred type predicate candidate in a reachability \
                         effects consult (getTypePredicateFromBody, M6)",
                    ));
                };
                query.traversed_inert_arm = true;
                return Ok(None);
            }
            if self.has_type_predicate_or_never_return_type(candidate)? {
                result = Some(candidate);
            }
        }
        self.assert_narrow_cache_writable();
        self.effects_signature_cache.insert(node, result);
        Ok(result)
    }

    /// tsc-port: hasTypePredicateOrNeverReturnType @6.0.3
    /// tsc-hash: edea2a64243c8de82922422ee9ae3abbc46b81fb3d084d0fc2c78d883c9e5fb8
    /// tsc-span: _tsc.js:70224-70226
    fn has_type_predicate_or_never_return_type(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<bool> {
        if self.get_type_predicate_of_signature(signature)?.is_some() {
            return Ok(true);
        }
        if let Some(declaration) = self.signature_of(signature).declaration {
            if let Some(return_type_node) = self.effective_return_type_node(declaration) {
                let annotated = self.get_type_from_type_node(return_type_node)?;
                return Ok(self.tables.flags_of(annotated).intersects(TypeFlags::NEVER));
            }
        }
        Ok(false)
    }

    /// The getTypePredicateFromBody precondition (59783-59788 plus
    /// the function's own bails at 79017-79028): an annotation-free
    /// NORMAL function/method with parameters and a `boolean` return
    /// could carry a TS 5.5 body-inferred predicate in tsc. Consumers
    /// FLAG when it holds (conservative superset — over-flagging
    /// reverts to the declared type; the remaining slack is the
    /// single-return expression probe, which is the family itself).
    /// tsrs-native: temporary M6-deferral probe (retires with the
    /// body-inference port).
    fn signature_may_have_body_inferred_predicate(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<bool> {
        let Some(declaration) = self.signature_of(signature).declaration else {
            return Ok(false);
        };
        let kind = self.kind_of(declaration);
        if !node_util::is_function_like_declaration_kind(kind) {
            return Ok(false);
        }
        // getTypePredicateFromBody bails outright on constructors and
        // accessors (79017-79022) and on async/generator functions
        // (getFunctionFlags !== Normal, 79028).
        if matches!(
            kind,
            SyntaxKind::Constructor | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
        ) {
            return Ok(false);
        }
        if self.get_function_flags(declaration) != 0 {
            return Ok(false);
        }
        if self.effective_return_type_node(declaration).is_some() {
            return Ok(false);
        }
        if self.get_parameter_count(signature)? == 0 {
            return Ok(false);
        }
        // 6.6f cycle guard: this probe can run inside the SAME
        // signature's return-type computation (functionHasImplicitReturn
        // → isReachableFlowNode → getEffectsSignature → here; mutual
        // recursion closes the loop — the readonlyRestParameters 7023
        // FP face). tsc's equivalent cycle lands in getTypePredicate-
        // FromBody's checkExpression, which circularity-breaks to NO
        // predicate and MEMOIZES noTypePredicate — the observable is
        // "no effects, walk past", so the in-progress answer here is a
        // faithful FALSE, not an uncertain flag.
        if self
            .resolution_targets
            .iter()
            .zip(self.resolution_property_names.iter())
            .any(|(target, property)| {
                matches!(
                    target,
                    crate::state::ResolutionTarget::Signature(in_progress)
                        if *in_progress == signature
                ) && *property == tsrs2_types::TypeSystemPropertyName::RESOLVED_RETURN_TYPE
            })
        {
            return Ok(false);
        }
        // 59783 tests Boolean (256) proper, not BooleanLike: a
        // literal-typed return can never pass the body probe's own
        // Boolean gate on the return expression (79048).
        let return_type = self.get_return_type_of_signature(signature)?;
        Ok(self
            .tables
            .flags_of(return_type)
            .intersects(TypeFlags::BOOLEAN))
    }

    /// tsc-port: getTypeOfDottedName @6.0.3
    /// tsc-hash: 38b49cbcd2c8cfb7bf9ce191be44619ba9196ed6e9180795f9e1dc6f0141fc48
    /// tsc-span: _tsc.js:70162-70193
    ///
    /// The diagnostic parameter is the assertion-signature
    /// elaboration (2775 family) — every flow consumer passes none,
    /// so the related-info arm is elided with it.
    fn get_type_of_dotted_name(&mut self, node: NodeId) -> CheckResult2<Option<TypeId>> {
        if self.node_flags(node) & tsrs2_types::NodeFlags::IN_WITH_STATEMENT.bits() != 0 {
            return Ok(None);
        }
        match self.kind_of(node) {
            SyntaxKind::Identifier => {
                let Some(symbol) = self.get_resolved_symbol(node)? else {
                    return Ok(None);
                };
                let export_symbol = self.get_export_symbol_of_value_symbol_if_exported(symbol);
                self.get_explicit_type_of_symbol(export_symbol)
            }
            SyntaxKind::ThisKeyword => self.get_explicit_this_type(node),
            SyntaxKind::SuperKeyword => self.check_super_expression(node).map(Some),
            SyntaxKind::PropertyAccessExpression => {
                let (receiver, name) = match self.data_of(node) {
                    NodeData::PropertyAccessExpression(data) => (data.expression, data.name),
                    _ => (None, None),
                };
                let (Some(receiver), Some(name)) = (receiver, name) else {
                    return Ok(None);
                };
                let Some(receiver_type) = self.get_type_of_dotted_name(receiver)? else {
                    return Ok(None);
                };
                let prop = if self.kind_of(name) == SyntaxKind::PrivateIdentifier {
                    let Some(type_symbol) = self.tables.type_of(receiver_type).symbol else {
                        return Ok(None);
                    };
                    let Some(text) = self.escaped_text_of(Some(name)).map(str::to_owned) else {
                        return Ok(None);
                    };
                    // tsc getSymbolNameForPrivateIdentifier (15905)
                    // keys `__#<symbolId>@<description>` with the
                    // binder's lazily-assigned symbol id — a
                    // checker-side numeric reconstruction sits in the
                    // wrong id space (the 6.6-review 2775 FP face:
                    // `this.#p1(v)` never resolved). Only an OWN
                    // member of the receiver's class can carry that
                    // class's id, so recover the exact mangled key
                    // from the class's own members table and route
                    // the typed lookup through it.
                    let suffix = format!("@{text}");
                    let lookup = self
                        .get_members_of_symbol(type_symbol)?
                        .keys()
                        .find(|name| name.starts_with("__#") && name.ends_with(&suffix))
                        .cloned();
                    match lookup {
                        Some(lookup) => self.get_property_of_type_full(receiver_type, &lookup)?,
                        None => None,
                    }
                } else {
                    let Some(text) = self.escaped_text_of(Some(name)).map(str::to_owned) else {
                        return Ok(None);
                    };
                    self.get_property_of_type_full(receiver_type, &text)?
                };
                match prop {
                    Some(prop) => self.get_explicit_type_of_symbol(prop),
                    None => Ok(None),
                }
            }
            SyntaxKind::ParenthesizedExpression => {
                let inner = match self.data_of(node) {
                    NodeData::ParenthesizedExpression(data) => data.expression,
                    _ => None,
                };
                match inner {
                    Some(inner) => self.get_type_of_dotted_name(inner),
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: isDeclarationWithExplicitTypeAnnotation @6.0.3
    /// tsc-hash: 1ce22ff5fa71bee60fc931c39179d1552c778f61d4e1fecaba74c9f4a9c54b63
    /// tsc-span: _tsc.js:70118-70120
    ///
    /// The JS-file initializer arm is checkJs band — elided with it.
    fn is_declaration_with_explicit_type_annotation(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::VariableDeclaration
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::Parameter
        ) && self.effective_type_annotation_node(node).is_some()
    }

    /// tsc-port: getExplicitTypeOfSymbol @6.0.3
    /// tsc-hash: e746c3000673eed6afae1fa9727123a0a6bca22174a94bcd656ccd7cb6ee876c
    /// tsc-span: _tsc.js:70121-70161
    ///
    /// The diagnostic-related-info arm rides with the elided
    /// assertion elaboration (see get_type_of_dotted_name).
    fn get_explicit_type_of_symbol(&mut self, symbol: SymbolId) -> CheckResult2<Option<TypeId>> {
        let symbol = self.resolve_symbol_shallow(symbol)?;
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(
            SymbolFlags::FUNCTION
                | SymbolFlags::METHOD
                | SymbolFlags::CLASS
                | SymbolFlags::VALUE_MODULE,
        ) {
            return self.get_type_of_symbol(symbol).map(Some);
        }
        if flags.intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY) {
            if self
                .get_check_flags(symbol)
                .intersects(tsrs2_types::CheckFlags::MAPPED)
            {
                let origin = self.links.symbol(symbol).synthetic_origin;
                if let Some(origin) = origin {
                    if self.get_explicit_type_of_symbol(origin)?.is_some() {
                        return self.get_type_of_symbol(symbol).map(Some);
                    }
                }
                return Ok(None);
            }
            let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
                return Ok(None);
            };
            if self.is_declaration_with_explicit_type_annotation(declaration) {
                return self.get_type_of_symbol(symbol).map(Some);
            }
            if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
                let statement = self
                    .parent_of(declaration)
                    .and_then(|parent| self.parent_of(parent));
                if let Some(statement) = statement {
                    if self.kind_of(statement) == SyntaxKind::ForOfStatement {
                        let (expression, await_modifier) = match self.data_of(statement) {
                            NodeData::ForOfStatement(data) => {
                                (data.expression, data.await_modifier)
                            }
                            _ => (None, None),
                        };
                        if let Some(expression) = expression {
                            if let Some(expression_type) =
                                self.get_type_of_dotted_name(expression)?
                            {
                                let use_flags = if await_modifier.is_some() {
                                    tsrs2_types::IterationUse::FOR_AWAIT_OF
                                } else {
                                    tsrs2_types::IterationUse::FOR_OF
                                };
                                let undefined = self.tables.intrinsics.undefined;
                                return self
                                    .check_iterated_type_or_element_type(
                                        use_flags,
                                        expression_type,
                                        undefined,
                                        None,
                                    )
                                    .map(Some);
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: getExplicitThisType @6.0.3
    /// tsc-hash: 0052ddcf8fe3e397778fc1edb69a5ba3298660e29f1eb9d2d63116370c84dd02
    /// tsc-span: _tsc.js:72464-72482
    fn get_explicit_this_type(&mut self, node: NodeId) -> CheckResult2<Option<TypeId>> {
        let source = self.binder.source_of_node(node);
        let Some(container) =
            node_util::get_this_container(source, node, /*include_arrow_functions*/ false)
        else {
            return Ok(None);
        };
        if node_util::is_function_like_kind(self.kind_of(container)) {
            let signature = self.get_signature_from_declaration(container)?;
            if let Some(this_parameter) = self.signature_of(signature).this_parameter {
                return self.get_explicit_type_of_symbol(this_parameter);
            }
        }
        let parent = self.parent_of(container);
        if let Some(parent) = parent {
            if matches!(
                self.kind_of(parent),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) {
                let Some(symbol) = self.get_symbol_of_declaration_opt(parent) else {
                    return Ok(None);
                };
                if self.is_static_element(container) {
                    return self.get_type_of_symbol(symbol).map(Some);
                }
                let declared = self.get_declared_type_of_symbol_slice(symbol)?;
                return Ok(self.this_type_of_interface(declared));
            }
        }
        Ok(None)
    }

    /// The declared class/interface's thisType read (tsc
    /// getDeclaredTypeOfSymbol(...).thisType).
    /// tsrs-native: TypeData accessor (GenericType stamp).
    fn this_type_of_interface(&self, ty: TypeId) -> Option<TypeId> {
        match &self.tables.type_of(ty).data {
            TypeData::GenericType { this_type, .. } => Some(*this_type),
            _ => None,
        }
    }

    /// tsc resolveSymbol (49113-49115, the alias-following prelude of
    /// getExplicitTypeOfSymbol) via isNonLocalAlias (49109-49112): a
    /// MERGED Alias|Value/Type/Namespace symbol keeps its own face
    /// (only the assignment-declaration alias overrides, 49111's
    /// second disjunct).
    /// tsrs-native: delegate to the modules family.
    fn resolve_symbol_shallow(&mut self, symbol: SymbolId) -> CheckResult2<SymbolId> {
        let flags = self.binder.symbol(symbol).flags;
        let excludes = SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE;
        let non_local_alias = (flags & (SymbolFlags::ALIAS | excludes)) == SymbolFlags::ALIAS
            || (flags.intersects(SymbolFlags::ALIAS) && flags.intersects(SymbolFlags::ASSIGNMENT));
        if non_local_alias {
            return self.resolve_alias(symbol);
        }
        Ok(symbol)
    }

    /// tsc-port: getTypePredicateOfSignature @6.0.3
    /// tsc-hash: e0ff01ec0d9d97ea4232841d047e9ab5431b4b3e969c445c4e712c4be1eb44b5
    /// tsc-span: _tsc.js:59765-59794
    ///
    /// signature.resolvedTypePredicate lives state-side (None = the
    /// memoized noTypePredicate). Instantiated signatures propagate
    /// their target's predicate through the mapper; composite
    /// signatures combine member predicates; declared predicates
    /// materialize from the TypePredicate return node. The
    /// getTypePredicateFromBody arm (TS 5.5 inferred predicates) is
    /// M6-adjacent and unported — its precondition FLAGS at the
    /// consumers (see get_effects_signature); the jsdoc arm is
    /// checkJs band.
    pub(crate) fn get_type_predicate_of_signature(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<TypePredicate>> {
        if let Some(cached) = self.resolved_type_predicates.get(&signature) {
            return Ok(cached.clone());
        }
        let result = self.compute_type_predicate_of_signature(signature)?;
        self.assert_narrow_cache_writable();
        self.resolved_type_predicates
            .insert(signature, result.clone());
        Ok(result)
    }

    /// The computation behind the memo (same tsc span).
    /// tsrs-native: memo split of getTypePredicateOfSignature.
    fn compute_type_predicate_of_signature(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<TypePredicate>> {
        let sig = self.signature_of(signature);
        if let Some(target) = sig.target {
            let mapper = sig.mapper;
            let Some(target_predicate) = self.get_type_predicate_of_signature(target)? else {
                return Ok(None);
            };
            let ty = match target_predicate.ty {
                Some(ty) => Some(self.instantiate_type(ty, mapper)?),
                None => None,
            };
            return Ok(Some(TypePredicate {
                ty,
                ..target_predicate
            }));
        }
        if let Some(composite_signatures) = sig.composite_signatures.clone() {
            let composite_kind = sig.composite_kind;
            return self
                .get_union_or_intersection_type_predicate(&composite_signatures, composite_kind);
        }
        let Some(declaration) = sig.declaration else {
            return Ok(None);
        };
        let Some(return_type_node) = self.effective_return_type_node(declaration) else {
            return Ok(None);
        };
        if self.kind_of(return_type_node) != SyntaxKind::TypePredicate {
            return Ok(None);
        }
        self.create_type_predicate_from_type_predicate_node(return_type_node, signature)
            .map(Some)
    }

    /// tsc-port: createTypePredicateFromTypePredicateNode @6.0.3
    /// tsc-hash: 8c1568e0a9c1f385ca2c68d11b9a8567819284d4e115854d507a5b4a6ba0c8b7
    /// tsc-span: _tsc.js:59795-59806
    fn create_type_predicate_from_type_predicate_node(
        &mut self,
        node: NodeId,
        signature: SignatureId,
    ) -> CheckResult2<TypePredicate> {
        let (asserts_modifier, parameter_name, type_node) = match self.data_of(node) {
            NodeData::TypePredicate(data) => {
                (data.asserts_modifier, data.parameter_name, data.r#type)
            }
            _ => (None, None, None),
        };
        let ty = match type_node {
            Some(type_node) => Some(self.get_type_from_type_node(type_node)?),
            None => None,
        };
        let asserts = asserts_modifier.is_some();
        let is_this = parameter_name
            .is_some_and(|parameter_name| self.kind_of(parameter_name) == SyntaxKind::ThisType);
        if is_this {
            return Ok(TypePredicate {
                kind: if asserts {
                    TypePredicateKind::AssertsThis
                } else {
                    TypePredicateKind::This
                },
                parameter_name: None,
                parameter_index: -1,
                ty,
            });
        }
        let name = parameter_name
            .and_then(|parameter_name| self.escaped_text_of(Some(parameter_name)))
            .map(str::to_owned);
        let parameter_index = match &name {
            Some(name) => {
                let parameters = self.signature_of(signature).parameters.clone();
                parameters
                    .iter()
                    .position(|&parameter| self.binder.symbol(parameter).escaped_name == *name)
                    .map_or(-1, |index| index as i64)
            }
            None => -1,
        };
        Ok(TypePredicate {
            kind: if asserts {
                TypePredicateKind::AssertsIdentifier
            } else {
                TypePredicateKind::Identifier
            },
            parameter_name: name,
            parameter_index,
            ty,
        })
    }

    /// tsc-port: getUnionOrIntersectionTypePredicate @6.0.3
    /// tsc-hash: 648edf0fb2d4d64618af102ff294ebff3e93525fb11def6efbfe04bacd3d9103
    /// tsc-span: _tsc.js:61586-61608
    fn get_union_or_intersection_type_predicate(
        &mut self,
        signatures: &[SignatureId],
        kind: Option<TypeFlags>,
    ) -> CheckResult2<Option<TypePredicate>> {
        let mut last: Option<TypePredicate> = None;
        let mut types: Vec<TypeId> = Vec::new();
        let is_intersection = kind == Some(TypeFlags::INTERSECTION);
        for &sig in signatures {
            if let Some(pred) = self.get_type_predicate_of_signature(sig)? {
                let kinds_match = last.as_ref().is_none_or(|last| {
                    last.kind == pred.kind && last.parameter_index == pred.parameter_index
                });
                if !matches!(
                    pred.kind,
                    TypePredicateKind::This | TypePredicateKind::Identifier
                ) || !kinds_match
                {
                    return Ok(None);
                }
                if let Some(ty) = pred.ty {
                    types.push(ty);
                }
                last = Some(pred);
            } else {
                if !is_intersection {
                    let return_type = self.get_return_type_of_signature(sig)?;
                    let false_fresh = self.tables.intrinsics.false_fresh;
                    let false_regular = self.tables.intrinsics.false_regular;
                    if return_type == false_fresh || return_type == false_regular {
                        continue;
                    }
                }
                return Ok(None);
            }
        }
        let Some(last) = last else {
            return Ok(None);
        };
        let composite_type = if is_intersection {
            self.get_intersection_type(&types, tsrs2_types::IntersectionFlags::NONE)?
        } else {
            self.get_union_type_ex(&types, tsrs2_types::UnionReduction::Literal)?
        };
        Ok(Some(TypePredicate {
            ty: Some(composite_type),
            ..last
        }))
    }

    /// tsc-port: narrowTypeByOptionality @6.0.3
    /// tsc-hash: 275372eca5f30971df70543fde5e12283bdc3a1febb2e1d5efb8dd21017818e6
    /// tsc-span: _tsc.js:71440-71449
    ///
    /// Optional-chain roots and `??`/`??=` left operands: a matching
    /// reference keeps/sheds undefined|null by the presence face; a
    /// non-matching one tries the discriminant-property path with the
    /// plain facts filter.
    fn narrow_type_by_optionality(
        &mut self,
        query: &mut FlowQuery,
        ty: TypeId,
        expr: NodeId,
        assume_present: bool,
    ) -> CheckResult2<TypeId> {
        let facts = if assume_present {
            TypeFacts::NE_UNDEFINED_OR_NULL
        } else {
            TypeFacts::EQ_UNDEFINED_OR_NULL
        };
        if self.is_matching_query_reference(query, expr)? {
            return self.get_adjusted_type_with_facts(ty, facts);
        }
        if let Some(access) = self.get_discriminant_property_access(query, expr, ty)? {
            return self.narrow_type_by_discriminant(ty, access, |state, t| {
                state.get_type_with_facts(t, facts)
            });
        }
        Ok(ty)
    }
}

/// tsc TypePredicateKind (This=0 | Identifier=1 | AssertsThis=2 |
/// AssertsIdentifier=3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypePredicateKind {
    This,
    Identifier,
    AssertsThis,
    AssertsIdentifier,
}

/// tsc-port: createTypePredicate @6.0.3
/// tsc-hash: 717a030e43a47be7075488eb54a1efe7aec3f542134f16e4cbe6666f19dd4e69
/// tsc-span: _tsc.js:59531-59533
///
/// The struct IS the factory (no behavior beyond field storage);
/// parameter_index mirrors tsc's findIndex result, -1 included.
#[derive(Clone, Debug)]
pub(crate) struct TypePredicate {
    pub(crate) kind: TypePredicateKind,
    pub(crate) parameter_name: Option<String>,
    pub(crate) parameter_index: i64,
    pub(crate) ty: Option<TypeId>,
}

/// tsc-port: typeofNEFacts @6.0.3
/// tsc-hash: c3cfcd34c8c39a75c323f299600e780301d6788e11dd017922291e223b4b5c4d
/// tsc-span: _tsc.js:46376-46385
fn typeof_ne_facts(text: &str) -> Option<TypeFacts> {
    Some(match text {
        "string" => TypeFacts::TYPEOF_NE_STRING,
        "number" => TypeFacts::TYPEOF_NE_NUMBER,
        "bigint" => TypeFacts::TYPEOF_NE_BIG_INT,
        "boolean" => TypeFacts::TYPEOF_NE_BOOLEAN,
        "symbol" => TypeFacts::TYPEOF_NE_SYMBOL,
        "undefined" => TypeFacts::NE_UNDEFINED,
        "object" => TypeFacts::TYPEOF_NE_OBJECT,
        "function" => TypeFacts::TYPEOF_NE_FUNCTION,
        _ => return None,
    })
}

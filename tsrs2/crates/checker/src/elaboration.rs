//! Phase 9.4: relation-error elaboration.
//!
//! tsc keeps `elaborateError` and its object/array/arrow helpers beside
//! the relation reporter. Earlier extraction slices left two local
//! stand-ins: a reporting walk in `operators.rs` and a disposition-only
//! walk in `calls.rs`. This module gives those decisions one owner before
//! 9.4b widens the reporting callers.

use tsrs2_diags::{gen as diagnostics, DiagnosticMessage, RelatedInfo};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{AccessFlags, CheckMode, TypeData, TypeFlags, TypeId, UnionReduction};

use crate::relate::RelationKind;
use crate::state::{CheckResult2, CheckerState, SignatureKind, Unsupported};

/// The semantic result of an elaboration attempt.
///
/// `Declined` is tsc's ordinary `false` result: the caller must emit its
/// relation head. `Reported` means an inner row was emitted and the
/// caller must suppress that head. `Unsupported` remains reserved for a
/// known unported elaboration branch; it is never used to mean
/// `Declined`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ElaborationOutcome {
    Declined,
    Reported,
}

impl ElaborationOutcome {
    /// tsrs-native: typed replacement for tsc's boolean
    /// elaborateError result at legacy call sites.
    pub(crate) fn reported(self) -> bool {
        self == Self::Reported
    }

    fn from_reported(reported: bool) -> Self {
        if reported {
            Self::Reported
        } else {
            Self::Declined
        }
    }
}

/// Read-only result used by call applicability until 9.4b routes that
/// path through the reporting engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ElaborationDisposition {
    Declined,
    DidYouMean { node: NodeId, related: RelatedInfo },
}

impl<'a> CheckerState<'a> {
    /// isOrHasGenericConditional (63954-63956).
    fn is_or_has_generic_conditional(&self, ty: TypeId) -> bool {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::CONDITIONAL) {
            return true;
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            if let TypeData::Intersection { types } = &self.tables.type_of(ty).data {
                return types
                    .to_vec()
                    .iter()
                    .any(|&ty| self.is_or_has_generic_conditional(ty));
            }
        }
        false
    }

    /// The shared predicate inside
    /// elaborateDidYouMeanToCallOrConstruct (64063-64091).
    ///
    /// The construct-signature family wins before the call-signature
    /// family. Both the reporting engine and the call-applicability
    /// probe use this exact decision.
    fn did_you_mean_signature_kind(
        &mut self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<Option<SignatureKind>> {
        // elaborateDidYouMeanToCallOrConstruct materializes both lists
        // before testing either, then gives construct signatures
        // reporting priority.
        let call_signatures = self.get_signatures_of_type(source, SignatureKind::Call)?;
        let construct_signatures = self.get_signatures_of_type(source, SignatureKind::Construct)?;
        for (kind, signatures) in [
            (SignatureKind::Construct, construct_signatures),
            (SignatureKind::Call, call_signatures),
        ] {
            for signature in signatures {
                let return_type = self.get_return_type_of_signature(signature)?;
                if self
                    .tables
                    .flags_of(return_type)
                    .intersects(TypeFlags::ANY | TypeFlags::NEVER)
                {
                    continue;
                }
                if self.check_type_related_to(return_type, target, relation)? {
                    return Ok(Some(kind));
                }
            }
        }
        Ok(None)
    }

    /// tsrs-native: report-free elaborateError probe for call
    /// applicability; retired when 9.4b routes those callers through
    /// the reporting engine.
    ///
    /// `Declined` means the ordinary relation head is correct.
    /// `DidYouMean` preserves the walked diagnostic node and related
    /// row. An `Unsupported` result identifies a branch that would
    /// report an inner row but is not routed through the common
    /// reporting engine until 9.4b/9.4c.
    pub(crate) fn probe_elaboration_disposition(
        &mut self,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<ElaborationDisposition> {
        if self.is_or_has_generic_conditional(target) {
            return Ok(ElaborationDisposition::Declined);
        }
        let mut walk = node;
        loop {
            // 63959-63967: the did-you-mean probe runs per recursion
            // level, reporting at the CURRENT node.
            if let Some(kind) = self.did_you_mean_signature_kind(source, target, relation)? {
                let message = if kind == SignatureKind::Construct {
                    &diagnostics::Did_you_mean_to_use_new_with_this_expression
                } else {
                    &diagnostics::Did_you_mean_to_call_this_expression
                };
                let related = self.related_info_for_node(walk, message, &[]);
                return Ok(ElaborationDisposition::DidYouMean {
                    node: walk,
                    related,
                });
            }
            match self.kind_of(walk) {
                SyntaxKind::AsExpression => {
                    let NodeData::AsExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    let is_const = data
                        .r#type
                        .is_some_and(|type_node| self.is_const_type_reference_node(type_node));
                    if !is_const {
                        return Ok(ElaborationDisposition::Declined);
                    }
                    match data.expression {
                        Some(expression) => walk = expression,
                        None => return Ok(ElaborationDisposition::Declined),
                    }
                }
                SyntaxKind::JsxExpression => {
                    let NodeData::JsxExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    match data.expression {
                        Some(expression) => walk = expression,
                        None => return Ok(ElaborationDisposition::Declined),
                    }
                }
                SyntaxKind::ParenthesizedExpression => {
                    let NodeData::ParenthesizedExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    match data.expression {
                        Some(expression) => walk = expression,
                        None => return Ok(ElaborationDisposition::Declined),
                    }
                }
                SyntaxKind::BinaryExpression => {
                    let NodeData::BinaryExpression(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    let operator = data.operator_token.map(|token| self.kind_of(token));
                    match operator {
                        Some(SyntaxKind::EqualsToken | SyntaxKind::CommaToken) => {
                            match data.right {
                                Some(right) => walk = right,
                                None => return Ok(ElaborationDisposition::Declined),
                            }
                        }
                        _ => return Ok(ElaborationDisposition::Declined),
                    }
                }
                SyntaxKind::ObjectLiteralExpression => {
                    // elaborateObjectLiteral (64456): the primitive/
                    // never-target early-out falls back to the plain
                    // head — whose object-literal source display is T2
                    // anyway — so the blanket escape loses nothing.
                    return Err(Unsupported::new(
                        "elaborateObjectLiteral (elementwise elaboration, T2)",
                    ));
                }
                SyntaxKind::ArrayLiteralExpression => {
                    // elaborateArrayLiteral (64410): decide whether the
                    // elementwise walk WOULD report — if not, tsc falls
                    // back to the plain head at the literal (live).
                    if self
                        .array_literal_elaboration_would_report(walk, source, target, relation)?
                    {
                        return Err(Unsupported::new(
                            "elaborateArrayLiteral (elementwise elaboration, T2)",
                        ));
                    }
                    return Ok(ElaborationDisposition::Declined);
                }
                SyntaxKind::JsxAttributes => {
                    return Err(Unsupported::new(
                        "elaborateJsxComponents (elementwise elaboration, T2)",
                    ));
                }
                SyntaxKind::ArrowFunction => {
                    // elaborateArrowFunction gates (64024-64038): an
                    // expression body, no annotated parameters, a
                    // single-call-signature source, and a callable
                    // target make the elaboration recurse into the
                    // return expression.
                    let NodeData::ArrowFunction(data) = self.data_of(walk) else {
                        unreachable!("kind/data agree");
                    };
                    let body_is_block = data
                        .body
                        .is_some_and(|body| self.kind_of(body) == SyntaxKind::Block);
                    if body_is_block {
                        return Ok(ElaborationDisposition::Declined);
                    }
                    let parameters = self.nodes_of(data.parameters);
                    let has_typed_parameter = parameters.iter().any(|&parameter| {
                        matches!(self.data_of(parameter), NodeData::Parameter(p) if p.r#type.is_some())
                    });
                    if has_typed_parameter {
                        return Ok(ElaborationDisposition::Declined);
                    }
                    // 64031: getSingleCallSignature(source) is the
                    // elaborateArrowFunction source gate.
                    if self.get_single_call_signature(source)?.is_none() {
                        return Ok(ElaborationDisposition::Declined);
                    }
                    if self
                        .get_signatures_of_type(target, SignatureKind::Call)?
                        .is_empty()
                    {
                        return Ok(ElaborationDisposition::Declined);
                    }
                    return Err(Unsupported::new(
                        "elaborateArrowFunction (return-position elaboration, T2)",
                    ));
                }
                _ => return Ok(ElaborationDisposition::Declined),
            }
        }
    }

    /// The elaborateArrayLiteral decision (64410-64431 +
    /// generateLimitedTupleElements 64398 + elaborateElementwise's
    /// per-element verdicts): true when tsc's elaboration would emit
    /// inner rows instead of the plain head.
    fn array_literal_elaboration_would_report(
        &mut self,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<bool> {
        if self
            .tables
            .flags_of(target)
            .intersects(TypeFlags::from_bits(
                TypeFlags::PRIMITIVE.bits() | TypeFlags::NEVER.bits(),
            ))
        {
            return Ok(false);
        }
        let elements = match self.data_of(node) {
            NodeData::ArrayLiteralExpression(data) => self.nodes_of(data.elements),
            _ => return Ok(false),
        };
        // Target-side pass first: an element can only produce a row
        // when the target has a matching indexed access — deciding
        // this before the forced-tuple re-check keeps no-index targets
        // out of the contextual element reads.
        let mut candidates: Vec<(usize, TypeId)> = Vec::new();
        for (index, &element) in elements.iter().enumerate() {
            if self.is_tuple_like_type(target)?
                && self
                    .get_property_of_type_full(target, &index.to_string())?
                    .is_none()
            {
                continue;
            }
            if self.kind_of(element) == SyntaxKind::OmittedExpression {
                continue;
            }
            let name_type = self.tables.get_number_literal_type(index as f64);
            // getBestMatchIndexedAccessTypeOrUndefined (64103-64114):
            // the direct indexed access, then — union targets only —
            // the same probe over getBestMatchingType's constituent.
            let mut target_prop = self.get_indexed_access_type_or_undefined(
                target,
                name_type,
                AccessFlags::NONE,
                None,
                None,
                None,
            )?;
            if target_prop.is_none() && self.tables.flags_of(target).intersects(TypeFlags::UNION) {
                if let Some(best) = self.get_best_matching_type(source, target)? {
                    target_prop = self.get_indexed_access_type_or_undefined(
                        best,
                        name_type,
                        AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )?;
                }
            }
            let Some(target_prop) = target_prop else {
                continue;
            };
            if self
                .tables
                .flags_of(target_prop)
                .intersects(TypeFlags::INDEXED_ACCESS)
            {
                continue;
            }
            candidates.push((index, target_prop));
        }
        if candidates.is_empty() {
            return Ok(false);
        }
        let tupleized = if self.is_tuple_like_type(source)? {
            source
        } else {
            // 64416-64423: re-check as a forced tuple under the target
            // context (re-runs dedupe against the original check).
            self.push_contextual_type(node, Some(target), /*is_cache*/ false);
            let result =
                self.check_array_literal(node, CheckMode::CONTEXTUAL, /*force_tuple*/ true);
            self.pop_contextual_type();
            let tupleized = result?;
            if !self.is_tuple_like_type(tupleized)? {
                return Ok(false);
            }
            tupleized
        };
        for (index, target_prop) in candidates {
            let name_type = self.tables.get_number_literal_type(index as f64);
            let Some(source_prop) = self.get_indexed_access_type_or_undefined(
                tupleized,
                name_type,
                AccessFlags::NONE,
                None,
                None,
                None,
            )?
            else {
                continue;
            };
            if !self.check_type_related_to(source_prop, target_prop, relation)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: elaborateDidYouMeanToCallOrConstruct @6.0.3
    /// tsc-hash: a720dfb07510cb077601fddf116e7c7fa5f96c9d967ed77b514d4e5b36795c31
    /// tsc-span: _tsc.js:64063-64091
    ///
    /// A failed source with call/construct signatures whose
    /// return/instance type fits the target re-reports AT THE
    /// EXPRESSION (construct signatures probe first) and adds the
    /// did-you-mean related row; the Any/Never return guard is tsc's.
    fn elaborate_did_you_mean_to_call_or_construct(
        &mut self,
        node: NodeId,
        source: TypeId,
        target: TypeId,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult2<ElaborationOutcome> {
        let Some(kind) =
            self.did_you_mean_signature_kind(source, target, RelationKind::Assignable)?
        else {
            return Ok(ElaborationOutcome::Declined);
        };
        let before = self.diagnostics.len();
        self.check_type_assignable_to(source, target, Some(node), head_message)?;
        if self.diagnostics.len() > before {
            let related = self.related_info_for_node(
                node,
                if kind == SignatureKind::Construct {
                    &diagnostics::Did_you_mean_to_use_new_with_this_expression
                } else {
                    &diagnostics::Did_you_mean_to_call_this_expression
                },
                &[],
            );
            if let Some(diagnostic) = self.diagnostics.last_mut() {
                diagnostic.related.push(related);
            }
        }
        Ok(ElaborationOutcome::Reported)
    }

    /// tsc-port: getBestMatchIndexedAccessTypeOrUndefined @6.0.3
    /// tsc-hash: d9c9a56511cb15f6d99180834836ddea10da6cc2af84f172c7932f6490c2e349
    /// tsc-span: _tsc.js:64103-64114
    fn member_elaboration_target_type(
        &mut self,
        expression: NodeId,
        target_type: TypeId,
        name_text: &str,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(property) = self.get_property_of_type_full(target_type, name_text)? {
            return Ok(Some(self.get_type_of_symbol(property)?));
        }
        let apparent = self.get_apparent_type(target_type)?;
        if let Some(info) = self.get_applicable_index_info_for_name_info(apparent, name_text)? {
            return Ok(Some(info.value_type));
        }
        if self
            .tables
            .flags_of(target_type)
            .intersects(TypeFlags::UNION)
        {
            let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
            if let Some(best) = self.get_best_matching_type(source_type, target_type)? {
                if let Some(property) = self.get_property_of_type_full(best, name_text)? {
                    return Ok(Some(self.get_type_of_symbol(property)?));
                }
                let apparent = self.get_apparent_type(best)?;
                if let Some(info) =
                    self.get_applicable_index_info_for_name_info(apparent, name_text)?
                {
                    return Ok(Some(info.value_type));
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: elaborateElementwise @6.0.3 (the report-pair tail)
    /// tsc-hash: c289d4a4008697be6117b4bcd7c5f21e756946f8ccf08d921769996736688326
    /// tsc-span: _tsc.js:64165-64171
    fn remove_missing_for_member_report(
        &mut self,
        expression: NodeId,
        target_type: TypeId,
        name_text: &str,
        actual: TypeId,
        expected: TypeId,
    ) -> CheckResult2<(TypeId, TypeId)> {
        let target_is_optional = self
            .get_property_of_type_full(target_type, name_text)?
            .is_some_and(|property| {
                self.binder
                    .symbol(property)
                    .flags
                    .intersects(tsrs2_types::SymbolFlags::OPTIONAL)
            });
        let source_is_optional = if target_is_optional {
            let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
            self.get_property_of_type_full(source_type, name_text)?
                .is_some_and(|property| {
                    self.binder
                        .symbol(property)
                        .flags
                        .intersects(tsrs2_types::SymbolFlags::OPTIONAL)
                })
        } else {
            false
        };
        let expected = self.remove_missing_type(expected, target_is_optional);
        let actual = self.remove_missing_type(actual, target_is_optional && source_is_optional);
        Ok((actual, expected))
    }

    /// tsrs-native: the currently live assignability/reporting subset
    /// of elaborateError (63957-64460).
    ///
    /// `probe_head` carries elaborateError's headMessage into the
    /// entry did-you-mean probe. `None` keeps the satisfies band's
    /// callable-source containment decision; every other caller and
    /// both inner recursions pass the generic relation head.
    pub(crate) fn elaborate_literal_assignment(
        &mut self,
        expression: NodeId,
        target_type: TypeId,
        probe_head: Option<&'static DiagnosticMessage>,
    ) -> CheckResult2<ElaborationOutcome> {
        // elaborateError's entry probe (63959-63966): runs BEFORE the
        // recursion arms on every entry.
        if let Some(head_message) = probe_head {
            let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
            if self
                .elaborate_did_you_mean_to_call_or_construct(
                    expression,
                    source_type,
                    target_type,
                    head_message,
                )?
                .reported()
            {
                return Ok(ElaborationOutcome::Reported);
            }
        }
        // elaborateError's recursion arms (63968-63983): parens and
        // const-assertions descend into the operand, `=`/comma
        // binaries descend into the RIGHT operand. Satisfies has NO
        // arm.
        match self.data_of(expression) {
            NodeData::ParenthesizedExpression(data) => {
                if let Some(inner) = data.expression {
                    return self.elaborate_literal_assignment(inner, target_type, probe_head);
                }
            }
            NodeData::AsExpression(data) => {
                if let (Some(inner), Some(type_node)) = (data.expression, data.r#type) {
                    if self.is_const_type_reference_node(type_node) {
                        return self.elaborate_literal_assignment(inner, target_type, probe_head);
                    }
                }
            }
            NodeData::BinaryExpression(data) => {
                if let (Some(operator), Some(right)) = (data.operator_token, data.right) {
                    if matches!(
                        self.kind_of(operator),
                        SyntaxKind::EqualsToken | SyntaxKind::CommaToken
                    ) {
                        return self.elaborate_literal_assignment(right, target_type, probe_head);
                    }
                }
            }
            _ => {}
        }
        let before = self.diagnostics.len();
        match self.data_of(expression) {
            NodeData::ArrowFunction(data) => {
                let data = data.clone();
                let Some(body) = data.body else {
                    return Ok(ElaborationOutcome::from_reported(
                        self.diagnostics.len() > before,
                    ));
                };
                if matches!(self.data_of(body), NodeData::Block(_)) {
                    return Ok(ElaborationOutcome::from_reported(
                        self.diagnostics.len() > before,
                    ));
                }
                let any_annotated = self.nodes_of(data.parameters).iter().any(|&parameter| {
                    matches!(self.data_of(parameter), NodeData::Parameter(data)
                        if data.r#type.is_some())
                });
                if any_annotated {
                    return Ok(ElaborationOutcome::from_reported(
                        self.diagnostics.len() > before,
                    ));
                }
                let source = self.check_expression_cached(expression, CheckMode::NORMAL)?;
                let Some(source_signature) = self.get_single_call_signature(source)? else {
                    return Ok(ElaborationOutcome::from_reported(
                        self.diagnostics.len() > before,
                    ));
                };
                let target_signatures =
                    self.get_signatures_of_type(target_type, SignatureKind::Call)?;
                if target_signatures.is_empty() {
                    return Ok(ElaborationOutcome::from_reported(
                        self.diagnostics.len() > before,
                    ));
                }
                let source_return = self.get_return_type_of_signature(source_signature)?;
                let mut target_returns = Vec::with_capacity(target_signatures.len());
                for signature in target_signatures {
                    target_returns.push(self.get_return_type_of_signature(signature)?);
                }
                let target_return =
                    self.get_union_type_ex(&target_returns, UnionReduction::Literal)?;
                if self.is_type_assignable_to(source_return, target_return)? {
                    return Ok(ElaborationOutcome::from_reported(
                        self.diagnostics.len() > before,
                    ));
                }
                if self
                    .elaborate_literal_assignment(
                        body,
                        target_return,
                        Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                    )?
                    .reported()
                {
                    return Ok(ElaborationOutcome::Reported);
                }
                self.check_type_assignable_to(
                    source_return,
                    target_return,
                    Some(body),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
                return Ok(ElaborationOutcome::Reported);
            }
            NodeData::ObjectLiteralExpression(data) => {
                let properties = self.nodes_of(data.properties);
                for property in properties {
                    let (name, initializer, member_lookup) = match self.data_of(property) {
                        NodeData::PropertyAssignment(data) => match (data.name, data.initializer) {
                            (Some(name), Some(initializer)) => (name, Some(initializer), false),
                            _ => continue,
                        },
                        NodeData::ShorthandPropertyAssignment(data) => match data.name {
                            Some(name) => (name, None, false),
                            None => continue,
                        },
                        NodeData::MethodDeclaration(data) => match data.name {
                            Some(name) => (name, None, true),
                            None => continue,
                        },
                        NodeData::GetAccessor(data) => match data.name {
                            Some(name) => (name, None, true),
                            None => continue,
                        },
                        NodeData::SetAccessor(data) => match data.name {
                            Some(name) => (name, None, true),
                            None => continue,
                        },
                        _ => continue,
                    };
                    let name_type = self.get_literal_type_from_property_name(name)?;
                    let Some(name_text) = self.property_name_from_type_usable(name_type) else {
                        continue;
                    };
                    let expected = match self.member_elaboration_target_type(
                        expression,
                        target_type,
                        &name_text,
                    )? {
                        Some(expected) => expected,
                        None => continue,
                    };
                    let actual = if member_lookup {
                        let source_type =
                            self.check_expression_cached(expression, CheckMode::NORMAL)?;
                        match self.get_property_of_type_full(source_type, &name_text)? {
                            Some(source_property) => self.get_type_of_symbol(source_property)?,
                            None => continue,
                        }
                    } else {
                        match initializer {
                            Some(initializer) => {
                                self.check_expression_cached(initializer, CheckMode::NORMAL)?
                            }
                            None => self.check_expression_cached(name, CheckMode::NORMAL)?,
                        }
                    };
                    if self.is_type_assignable_to(actual, expected)? {
                        continue;
                    }
                    if let Some(initializer) = initializer {
                        if self
                            .elaborate_literal_assignment(
                                initializer,
                                expected,
                                Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                            )?
                            .reported()
                        {
                            continue;
                        }
                    }
                    let computed_non_literal = !member_lookup
                        && match self.data_of(name) {
                            NodeData::ComputedPropertyName(data) => {
                                data.expression.is_some_and(|expression| {
                                    !matches!(
                                        self.kind_of(expression),
                                        SyntaxKind::StringLiteral
                                            | SyntaxKind::NoSubstitutionTemplateLiteral
                                            | SyntaxKind::NumericLiteral
                                    )
                                })
                            }
                            _ => false,
                        };
                    let message = if computed_non_literal {
                        &diagnostics::Type_of_computed_property_s_value_is_0_which_is_not_assignable_to_type_1
                    } else {
                        &diagnostics::Type_0_is_not_assignable_to_type_1
                    };
                    let (actual, expected) = self.remove_missing_for_member_report(
                        expression,
                        target_type,
                        &name_text,
                        actual,
                        expected,
                    )?;
                    self.check_type_assignable_to(actual, expected, Some(name), message)?;
                }
            }
            NodeData::ArrayLiteralExpression(data) => {
                let elements = self.nodes_of(data.elements);
                if elements
                    .iter()
                    .any(|&element| self.kind_of(element) == SyntaxKind::SpreadElement)
                {
                    return Err(Unsupported::new(
                        "failed array-literal relation with a spread element \
                         (elaborateArrayLiteral tupleization; M6 close -> phase-9 2xxx \
                         sweep, M7)",
                    ));
                }
                for (index, element) in elements.into_iter().enumerate() {
                    if matches!(
                        self.kind_of(element),
                        SyntaxKind::OmittedExpression | SyntaxKind::SpreadElement
                    ) {
                        continue;
                    }
                    let index_name = index.to_string();
                    let expected = if self.is_tuple_like_type(target_type)?
                        && self
                            .get_property_of_type_full(target_type, &index_name)?
                            .is_none()
                    {
                        continue;
                    } else {
                        match self.member_elaboration_target_type(
                            expression,
                            target_type,
                            &index_name,
                        )? {
                            Some(expected) => expected,
                            None => continue,
                        }
                    };
                    let actual = self.check_expression_cached(element, CheckMode::NORMAL)?;
                    if self.is_type_assignable_to(actual, expected)? {
                        continue;
                    }
                    let error_node = self.get_effective_check_node(element);
                    if self
                        .elaborate_literal_assignment(
                            error_node,
                            expected,
                            Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                        )?
                        .reported()
                    {
                        continue;
                    }
                    let (actual, expected) = self.remove_missing_for_member_report(
                        expression,
                        target_type,
                        &index_name,
                        actual,
                        expected,
                    )?;
                    self.check_type_assignable_to(
                        actual,
                        expected,
                        Some(error_node),
                        &diagnostics::Type_0_is_not_assignable_to_type_1,
                    )?;
                }
            }
            _ => {}
        }
        Ok(ElaborationOutcome::from_reported(
            self.diagnostics.len() > before,
        ))
    }
}

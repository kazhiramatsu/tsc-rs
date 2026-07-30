//! Phase 9.4: relation-error elaboration.
//!
//! The common reporter owns assignment, return, ordinary call
//! applicability, and JSX applicability. `ElaborationOutcome` keeps
//! tsc's "reported an inner row" decision separate from an ordinary
//! declined walk, while applicability captures the emitted diagnostics
//! as overload-selection data.

use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{AccessFlags, CheckMode, TypeData, TypeFlags, TypeId, UnionReduction};

use crate::relate::RelationKind;
use crate::state::{CheckResult2, CheckerState, SignatureKind};

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
    pub(crate) fn member_elaboration_target_type(
        &mut self,
        source_type: TypeId,
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
    pub(crate) fn remove_missing_for_member_report(
        &mut self,
        source_type: TypeId,
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

    /// tsc-port: elaborateElementwise @6.0.3
    /// tsc-hash: ba522e034925ca5fe1f8233c0d876dafe1973315638a5f524a6eac6d0b0c3505
    /// tsc-span: _tsc.js:64126-64205
    ///
    /// The provenance tail after the inner relation row has been
    /// created. The oracle host runs `noLib: true` and passes every
    /// vendored lib as an ordinary root, so its `libFiles` set is
    /// empty: `host.isSourceFileDefaultLibrary` is false for every
    /// declaration in this program model.
    pub(crate) fn attach_elementwise_elaboration_related(
        &mut self,
        diagnostic_index: usize,
        target_type: TypeId,
        name_type: TypeId,
    ) -> CheckResult2<()> {
        let property_name = self.property_name_from_type_usable(name_type);
        let target_property = match property_name.as_deref() {
            Some(property_name) => self.get_property_of_type_full(target_type, property_name)?,
            None => None,
        };

        if target_property.is_none() {
            if let Some(declaration) = self
                .get_applicable_index_info(target_type, name_type)?
                .and_then(|info| info.declaration)
            {
                let related = self.related_info_for_node(
                    declaration,
                    &diagnostics::The_expected_type_comes_from_this_index_signature,
                    &[],
                );
                if let Some(diagnostic) = self.diagnostics.get_mut(diagnostic_index) {
                    diagnostic.related.push(related);
                }
                return Ok(());
            }
        }

        let target_node = target_property
            .and_then(|property| self.binder.symbol(property).declarations.first().copied())
            .or_else(|| {
                self.tables
                    .type_of(target_type)
                    .symbol
                    .and_then(|symbol| self.binder.symbol(symbol).declarations.first().copied())
            });
        let Some(target_node) = target_node else {
            return Ok(());
        };

        let property_display = match property_name {
            Some(ref property_name)
                if !self
                    .tables
                    .flags_of(name_type)
                    .intersects(TypeFlags::UNIQUE_ES_SYMBOL) =>
            {
                tsrs2_binder::unescape_leading_underscores(property_name).to_owned()
            }
            _ => {
                let Ok(display) = self.type_to_string_slice(name_type) else {
                    return Ok(());
                };
                display
            }
        };
        let Ok(target_text) = self.type_to_string_slice(target_type) else {
            return Ok(());
        };
        let related = self.related_info_for_node(
            target_node,
            &diagnostics::The_expected_type_comes_from_property_0_which_is_declared_here_on_type_1,
            &[&property_display, &target_text],
        );
        if let Some(diagnostic) = self.diagnostics.get_mut(diagnostic_index) {
            diagnostic.related.push(related);
        }
        Ok(())
    }

    /// tsc-port: elaborateArrowFunction @6.0.3
    /// tsc-hash: 592e789cf5d2404080e9d8b355099c7a7f0e7580287399e52d8b25a4950b9cd7
    /// tsc-span: _tsc.js:64024-64102
    fn attach_arrow_return_elaboration_related(
        &mut self,
        diagnostic_index: usize,
        target_type: TypeId,
    ) {
        let declaration = self
            .tables
            .type_of(target_type)
            .symbol
            .and_then(|symbol| self.binder.symbol(symbol).declarations.first().copied());
        let Some(declaration) = declaration else {
            return;
        };
        let related = self.related_info_for_node(
            declaration,
            &diagnostics::The_expected_type_comes_from_the_return_type_of_this_signature,
            &[],
        );
        if let Some(diagnostic) = self.diagnostics.get_mut(diagnostic_index) {
            diagnostic.related.push(related);
        }
    }

    /// tsrs-native: elaborateJsxComponents @6.0.3 children slice
    /// (`_tsc.js` 64312-64378).
    ///
    /// Attribute checking has already synthesized the source's
    /// `children` property. For multiple semantic children tsc builds
    /// a fresh tuple from the individual child expressions and walks
    /// the array-like target elementwise, so diagnostics stay on each
    /// child rather than collapsing to the opening tag.
    fn elaborate_jsx_children(
        &mut self,
        attributes: NodeId,
        target_type: TypeId,
    ) -> CheckResult2<bool> {
        let Some(opening) = self.parent_of(attributes) else {
            return Ok(false);
        };
        if self.kind_of(opening) != SyntaxKind::JsxOpeningElement {
            return Ok(false);
        }
        let Some(containing_element) = self.parent_of(opening) else {
            return Ok(false);
        };
        let children = match self.data_of(containing_element) {
            NodeData::JsxElement(data) if data.opening_element == Some(opening) => {
                self.nodes_of(data.children)
            }
            _ => return Ok(false),
        };
        let semantic_children: Vec<NodeId> = children
            .into_iter()
            .filter(|&child| self.is_semantic_jsx_child(child))
            .collect();
        if semantic_children.is_empty() {
            return Ok(false);
        }
        let jsx_namespace = self.get_jsx_namespace_at(attributes)?;
        let children_name = self
            .get_jsx_element_children_property_name(jsx_namespace)?
            .unwrap_or_else(|| "children".to_owned());
        let name_type = self.tables.get_string_literal_type(&children_name);
        let Some(children_target) = self.get_indexed_access_type_or_undefined(
            target_type,
            name_type,
            AccessFlags::NONE,
            None,
            None,
            None,
        )?
        else {
            return Ok(false);
        };
        let target_parts = match &self.tables.type_of(children_target).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => vec![children_target],
        };
        let mut indexed_target_parts = Vec::new();
        for target_part in target_parts {
            if let Some(element) = self.get_element_type_of_array_type(target_part)? {
                indexed_target_parts.push(element);
            } else if self.is_tuple_like_type(target_part)? {
                indexed_target_parts.extend(self.get_type_arguments(target_part)?);
            }
        }
        let indexed_target = if indexed_target_parts.is_empty() {
            None
        } else {
            Some(self.get_union_type_ex(&indexed_target_parts, UnionReduction::Literal)?)
        };
        let expected = if semantic_children.len() > 1 {
            match indexed_target {
                Some(expected) => expected,
                None => return Ok(false),
            }
        } else {
            // Single-child failures keep the enclosing JSX relation
            // head in the currently supported corpus (the complete
            // scalar/array 2745/2747 cardinality ladder remains owned
            // by the source-level reporter below).
            return Ok(false);
        };

        let mut reported = false;
        for child in semantic_children {
            let actual =
                self.check_expression_for_mutable_location(child, CheckMode::NORMAL, false)?;
            if self.is_type_assignable_to(actual, expected)? {
                continue;
            }
            if self
                .elaborate_literal_assignment(
                    child,
                    expected,
                    Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                )?
                .reported()
            {
                reported = true;
                continue;
            }
            self.check_type_assignable_to(
                actual,
                expected,
                Some(child),
                &diagnostics::Type_0_is_not_assignable_to_type_1,
            )?;
            reported = true;
        }
        Ok(reported)
    }

    /// tsc-port: checkTypeAssignableToAndOptionallyElaborate @6.0.3
    /// tsc-hash: dbd7908806e20f7e4764fbdf33970aba20ca29fd3bba2bf210cee82985102c06
    /// tsc-span: _tsc.js:63934-63946
    ///
    /// The verdict probe owns the original source/target pair. A
    /// failed pair is elaborated first; when elaboration declines, the
    /// ordinary reporting entry performs its existing read-source /
    /// write-target normalization.
    pub(crate) fn check_type_assignable_to_and_optionally_elaborate(
        &mut self,
        source_type: TypeId,
        target_type: TypeId,
        error_node: Option<NodeId>,
        expression: NodeId,
        head_message: &'static DiagnosticMessage,
    ) -> CheckResult2<bool> {
        if self.is_type_assignable_to(source_type, target_type)? {
            return Ok(true);
        }
        if error_node.is_some()
            && self
                .elaborate_assignment_relation(
                    expression,
                    source_type,
                    target_type,
                    Some(head_message),
                )?
                .reported()
        {
            return Ok(false);
        }
        self.check_type_assignable_to(source_type, target_type, error_node, head_message)
    }

    /// tsrs-native: the currently live assignability/reporting subset
    /// of elaborateError (63957-64460).
    pub(crate) fn elaborate_literal_assignment(
        &mut self,
        expression: NodeId,
        target_type: TypeId,
        probe_head: Option<&'static DiagnosticMessage>,
    ) -> CheckResult2<ElaborationOutcome> {
        let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
        self.elaborate_assignment_relation(expression, source_type, target_type, probe_head)
    }

    /// tsc-port: elaborateError @6.0.3
    /// tsc-hash: cf474114c976f5967a2be3275091b181fd65ee1a99c08cc8ea1fbf435695d421
    /// tsc-span: _tsc.js:63957-64091
    ///
    /// `source_type` is explicit because elaborateError preserves it
    /// while peeling transparent syntax. Elementwise recursion passes
    /// the indexed source member instead.
    fn elaborate_assignment_relation(
        &mut self,
        expression: NodeId,
        source_type: TypeId,
        target_type: TypeId,
        probe_head: Option<&'static DiagnosticMessage>,
    ) -> CheckResult2<ElaborationOutcome> {
        if self.is_or_has_generic_conditional(target_type) {
            return Ok(ElaborationOutcome::Declined);
        }
        // elaborateError's entry probe (63959-63966): runs BEFORE the
        // recursion arms on every entry.
        if let Some(head_message) = probe_head {
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
                    return self.elaborate_assignment_relation(
                        inner,
                        source_type,
                        target_type,
                        probe_head,
                    );
                }
            }
            NodeData::AsExpression(data) => {
                if let (Some(inner), Some(type_node)) = (data.expression, data.r#type) {
                    if self.is_const_type_reference_node(type_node) {
                        return self.elaborate_assignment_relation(
                            inner,
                            source_type,
                            target_type,
                            probe_head,
                        );
                    }
                }
            }
            NodeData::JsxExpression(data) => {
                if let Some(inner) = data.expression {
                    return self.elaborate_assignment_relation(
                        inner,
                        source_type,
                        target_type,
                        probe_head,
                    );
                }
            }
            NodeData::BinaryExpression(data) => {
                if let (Some(operator), Some(right)) = (data.operator_token, data.right) {
                    if matches!(
                        self.kind_of(operator),
                        SyntaxKind::EqualsToken | SyntaxKind::CommaToken
                    ) {
                        return self.elaborate_assignment_relation(
                            right,
                            source_type,
                            target_type,
                            probe_head,
                        );
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
                let Some(source_signature) = self.get_single_call_signature(source_type)? else {
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
                    .elaborate_assignment_relation(
                        body,
                        source_return,
                        target_return,
                        Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                    )?
                    .reported()
                {
                    return Ok(ElaborationOutcome::Reported);
                }
                let diagnostics_before_report = self.diagnostics.len();
                self.check_type_assignable_to(
                    source_return,
                    target_return,
                    Some(body),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
                if self.diagnostics.len() > diagnostics_before_report {
                    let diagnostic_index = self.diagnostics.len() - 1;
                    self.attach_arrow_return_elaboration_related(diagnostic_index, target_type);
                }
                return Ok(ElaborationOutcome::Reported);
            }
            NodeData::ObjectLiteralExpression(data) => {
                // elaborateObjectLiteral (64456): primitive and Never
                // targets decline before generating member entries.
                if self
                    .tables
                    .flags_of(target_type)
                    .intersects(TypeFlags::from_bits(
                        TypeFlags::PRIMITIVE.bits() | TypeFlags::NEVER.bits(),
                    ))
                {
                    return Ok(ElaborationOutcome::Declined);
                }
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
                        source_type,
                        target_type,
                        &name_text,
                    )? {
                        Some(expected) => expected,
                        None => continue,
                    };
                    // elaborateElementwise (64131-64148) compares the
                    // indexed SOURCE property first. In particular,
                    // a mutable object property has already widened
                    // `{ a: 1 }` to `number`; reading the initializer
                    // directly here would incorrectly resurrect the
                    // fresh literal `1`.
                    let Some(source_property_type) = self.get_indexed_access_type_or_undefined(
                        source_type,
                        name_type,
                        AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )?
                    else {
                        continue;
                    };
                    if self.is_type_assignable_to(source_property_type, expected)? {
                        continue;
                    }
                    if let Some(initializer) = initializer {
                        if self
                            .elaborate_assignment_relation(
                                initializer,
                                source_property_type,
                                expected,
                                Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                            )?
                            .reported()
                        {
                            continue;
                        }
                    }
                    // checkExpressionForMutableLocationWithContextualType
                    // (64115-64125): the syntax-specific source is
                    // rechecked under the indexed source property,
                    // preserving an explicit `as const` while keeping
                    // an ordinary mutable literal widened.
                    let specific_source = if member_lookup {
                        source_property_type
                    } else if let Some(initializer) = initializer {
                        self.push_contextual_type(
                            initializer,
                            Some(source_property_type),
                            /*is_cache*/ false,
                        );
                        let result = self.check_expression_for_mutable_location(
                            initializer,
                            CheckMode::CONTEXTUAL,
                            /*force_tuple*/ false,
                        );
                        self.pop_contextual_type();
                        result?
                    } else {
                        source_property_type
                    };
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
                    let original_expected = expected;
                    let (source_property_type, expected) = self.remove_missing_for_member_report(
                        source_type,
                        target_type,
                        &name_text,
                        source_property_type,
                        expected,
                    )?;
                    let (specific_source, _) = self.remove_missing_for_member_report(
                        source_type,
                        target_type,
                        &name_text,
                        specific_source,
                        original_expected,
                    )?;
                    let diagnostics_before_report = self.diagnostics.len();
                    let specific_related = self.check_type_assignable_to(
                        specific_source,
                        expected,
                        Some(name),
                        message,
                    )?;
                    // 64168-64170: if contextual rechecking made the
                    // syntax-specific source pass, report against the
                    // indexed source property that originally failed.
                    if specific_related && specific_source != source_property_type {
                        self.check_type_assignable_to(
                            source_property_type,
                            expected,
                            Some(name),
                            message,
                        )?;
                    }
                    if self.diagnostics.len() > diagnostics_before_report {
                        let diagnostic_index = self.diagnostics.len() - 1;
                        self.attach_elementwise_elaboration_related(
                            diagnostic_index,
                            target_type,
                            name_type,
                        )?;
                    }
                }
            }
            NodeData::ArrayLiteralExpression(data) => {
                let elements = self.nodes_of(data.elements);
                // elaborateArrayLiteral @6.0.3, _tsc.js:64410-64431
                // vendored span hash:
                // 226140f17e9a3411add9f3a938acc8794d00b550a22d5adfcb775c5c8f9b8bc5
                //
                // A non-tuple source is checked again under the target
                // context with forceTuple. This is load-bearing for
                // spread elements: generateLimitedTupleElements indexes
                // the tupleized SOURCE by the syntax-element position,
                // rather than comparing the SpreadElement expression's
                // array type directly.
                if self
                    .tables
                    .flags_of(target_type)
                    .intersects(TypeFlags::from_bits(
                        TypeFlags::PRIMITIVE.bits() | TypeFlags::NEVER.bits(),
                    ))
                {
                    return Ok(ElaborationOutcome::Declined);
                }
                let tupleized_source = if self.is_tuple_like_type(source_type)? {
                    source_type
                } else {
                    self.push_contextual_type(
                        expression,
                        Some(target_type),
                        /*is_cache*/ false,
                    );
                    let result = self.check_array_literal(
                        expression,
                        CheckMode::CONTEXTUAL,
                        /*force_tuple*/ true,
                    );
                    self.pop_contextual_type();
                    let tupleized = result?;
                    if !self.is_tuple_like_type(tupleized)? {
                        return Ok(ElaborationOutcome::Declined);
                    }
                    tupleized
                };
                for (index, element) in elements.into_iter().enumerate() {
                    if self.kind_of(element) == SyntaxKind::OmittedExpression {
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
                            tupleized_source,
                            target_type,
                            &index_name,
                        )? {
                            Some(expected) => expected,
                            None => continue,
                        }
                    };
                    let name_type = self.tables.get_number_literal_type(index as f64);
                    let Some(actual) = self.get_indexed_access_type_or_undefined(
                        tupleized_source,
                        name_type,
                        AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )?
                    else {
                        continue;
                    };
                    if self.is_type_assignable_to(actual, expected)? {
                        continue;
                    }
                    let error_node = self.get_effective_check_node(element);
                    if self
                        .elaborate_assignment_relation(
                            error_node,
                            actual,
                            expected,
                            Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                        )?
                        .reported()
                    {
                        continue;
                    }
                    // elaborateElementwise 64153-64157 rechecks the
                    // syntax element under the indexed source type.
                    // A tupleized mutable source can contain `boolean`,
                    // while rechecking `true` in that context produces
                    // the regular singleton used in tsc's diagnostic.
                    let specific_source = {
                        self.push_contextual_type(
                            error_node,
                            Some(actual),
                            /*is_cache*/ false,
                        );
                        let result = self.check_expression_for_mutable_location(
                            error_node,
                            CheckMode::CONTEXTUAL,
                            /*force_tuple*/ false,
                        );
                        self.pop_contextual_type();
                        result?
                    };
                    let original_expected = expected;
                    let (source_property_type, expected) = self.remove_missing_for_member_report(
                        tupleized_source,
                        target_type,
                        &index_name,
                        actual,
                        expected,
                    )?;
                    let (specific_source, _) = self.remove_missing_for_member_report(
                        tupleized_source,
                        target_type,
                        &index_name,
                        specific_source,
                        original_expected,
                    )?;
                    let diagnostics_before_report = self.diagnostics.len();
                    let specific_related = self.check_type_assignable_to(
                        specific_source,
                        expected,
                        Some(error_node),
                        &diagnostics::Type_0_is_not_assignable_to_type_1,
                    )?;
                    if specific_related && specific_source != source_property_type {
                        self.check_type_assignable_to(
                            source_property_type,
                            expected,
                            Some(error_node),
                            &diagnostics::Type_0_is_not_assignable_to_type_1,
                        )?;
                    }
                    if self.diagnostics.len() > diagnostics_before_report {
                        let diagnostic_index = self.diagnostics.len() - 1;
                        self.attach_elementwise_elaboration_related(
                            diagnostic_index,
                            target_type,
                            name_type,
                        )?;
                    }
                }
            }
            NodeData::JsxAttributes(_) => {
                let attributes_reported = self.elaborate_jsx_named_attributes(
                    expression,
                    source_type,
                    target_type,
                    RelationKind::Assignable,
                )?;
                let children_reported = self.elaborate_jsx_children(expression, target_type)?;
                if attributes_reported || children_reported {
                    return Ok(ElaborationOutcome::Reported);
                }
            }
            _ => {}
        }
        Ok(ElaborationOutcome::from_reported(
            self.diagnostics.len() > before,
        ))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_diags::gen as diagnostics;
    use tsrs2_syntax::SyntaxKind;
    use tsrs2_types::{CheckMode, CompilerOptions};

    use crate::state::test_support::with_program_state;

    fn jsx_attributes_optional_elaboration_codes(attribute: &str) -> Vec<u32> {
        let text = format!(
            "declare namespace JSX {{\n\
               interface Element {{}}\n\
               interface IntrinsicElements {{ x: {{ n: number }} }}\n\
             }}\n\
             declare const bad: {{ n: string }};\n\
             (<x {attribute} />);\n"
        );
        let options = CompilerOptions {
            jsx: Some(1),
            ..CompilerOptions::default()
        };
        with_program_state(&[("a.tsx", &text)], &options, |state| {
            state.check_source_file(0);
            let (attributes, target_node, opening) = {
                let source = state.binder.source(0);
                let attributes = source
                    .arena
                    .node_ids()
                    .find(|&node| source.arena.node(node).kind == SyntaxKind::JsxAttributes)
                    .expect("JSX attributes");
                let target_node = source
                    .arena
                    .node_ids()
                    .find(|&node| source.arena.node(node).kind == SyntaxKind::TypeLiteral)
                    .expect("intrinsic attributes target");
                let opening = source
                    .arena
                    .node(attributes)
                    .parent
                    .expect("opening element");
                (attributes, target_node, opening)
            };
            let source_type = state
                .check_expression_cached(attributes, CheckMode::NORMAL)
                .expect("JSX attributes source");
            let target_type = state
                .get_type_from_type_node(target_node)
                .expect("JSX attributes target");
            state.diagnostics.clear();
            state
                .check_type_assignable_to_and_optionally_elaborate(
                    source_type,
                    target_type,
                    Some(opening),
                    attributes,
                    &diagnostics::Type_0_does_not_satisfy_the_expected_type_1,
                )
                .expect("optional elaboration");
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.file_name.is_some())
                .map(|diagnostic| diagnostic.code())
                .collect()
        })
    }

    #[test]
    fn jsx_attributes_optional_elaboration_reports_named_member() {
        assert_eq!(jsx_attributes_optional_elaboration_codes("n=\"s\""), [2322]);
    }

    #[test]
    fn jsx_attributes_optional_elaboration_decline_reports_relation_head() {
        assert_eq!(
            jsx_attributes_optional_elaboration_codes("{...bad}"),
            [1360]
        );
    }
}

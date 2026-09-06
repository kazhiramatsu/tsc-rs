//! Phase 9.4: relation-error elaboration.
//!
//! The common reporter owns assignment, return, ordinary call
//! applicability, and JSX applicability. `ElaborationOutcome` keeps
//! tsc's "reported an inner row" decision separate from an ordinary
//! declined walk, while applicability captures the emitted diagnostics
//! as overload-selection data.

use tsc_diagnostics::{
    gen as diagnostics, Diagnostic, DiagnosticMessage, MessageChain, RelatedInfo,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{
    AccessFlags, CheckMode, IterationTypeKind, IterationUse, TypeData, TypeFlags, TypeId,
    UnionReduction,
};

use crate::relate::RelationKind;
use crate::state::{CheckResult, CheckerState, SignatureKind};

/// The semantic result of an elaboration attempt.
///
/// `Declined` is tsc's ordinary `false` result: the caller must emit its
/// relation head. `Reported` means an inner row was emitted and the
/// caller must suppress that head. A typed `Err(CheckAbort)` is the
/// separate transaction-unwind channel and is never conflated with
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

/// Explicit diagnostic ownership for one `elaborateError` frame.
///
/// tsc writes relation rows either to the program collection or to an
/// `errorOutputContainer`. Keeping that choice in the frame prevents
/// overload applicability from guessing ownership from a diagnostic's
/// source span, and keeps lazily forced global diagnostics outside the
/// container.
pub(crate) struct ElaborationDiagnosticSink {
    destination: ElaborationDiagnosticDestination,
    reports: usize,
    containing_message_chain: Option<MessageChain>,
}

enum ElaborationDiagnosticDestination {
    Program,
    Captured(Vec<CapturedElaborationDiagnostic>),
}

struct CapturedElaborationDiagnostic {
    diagnostic: Diagnostic,
    used_containing_message_chain: bool,
}

impl ElaborationDiagnosticSink {
    fn program() -> Self {
        Self {
            destination: ElaborationDiagnosticDestination::Program,
            reports: 0,
            containing_message_chain: None,
        }
    }

    fn captured(containing_message_chain: Option<MessageChain>) -> Self {
        Self {
            destination: ElaborationDiagnosticDestination::Captured(Vec::new()),
            reports: 0,
            containing_message_chain,
        }
    }

    /// tsrs-native: retain whether the relation boundary consumed the
    /// applicability run's containing chain. Captured rows are finalized only
    /// after the complete elaboration walk has advanced that shared chain.
    pub(crate) fn publish_relation(
        &mut self,
        state: &mut CheckerState,
        diagnostic: Diagnostic,
        used_containing_message_chain: bool,
    ) {
        self.reports += 1;
        match &mut self.destination {
            ElaborationDiagnosticDestination::Program => {
                state.push_error_diagnostic(diagnostic);
            }
            ElaborationDiagnosticDestination::Captured(diagnostics) => {
                diagnostics.push(CapturedElaborationDiagnostic {
                    diagnostic,
                    used_containing_message_chain,
                })
            }
        }
    }

    /// tsc's JSX cardinality rows call `error()` and then also append
    /// that diagnostic to a skip-logging output container.
    fn publish_and_capture(&mut self, state: &mut CheckerState, diagnostic: Diagnostic) {
        self.reports += 1;
        state.push_error_diagnostic(diagnostic.clone());
        if let ElaborationDiagnosticDestination::Captured(diagnostics) = &mut self.destination {
            diagnostics.push(CapturedElaborationDiagnostic {
                diagnostic,
                used_containing_message_chain: false,
            });
        }
    }

    /// tsrs-native: snapshot the typed elaboration sink's report counter for
    /// nested elementwise-report detection.
    pub(crate) fn mark(&self) -> usize {
        self.reports
    }

    /// tsrs-native: compare a report-counter snapshot without inspecting or
    /// borrowing the sink's owned diagnostic destination.
    pub(crate) fn reported_since(&self, mark: usize) -> bool {
        self.reports > mark
    }

    fn into_captured(self) -> Vec<Diagnostic> {
        match self.destination {
            ElaborationDiagnosticDestination::Program => {
                unreachable!("captured elaboration wrapper owns a captured sink")
            }
            ElaborationDiagnosticDestination::Captured(diagnostics) => {
                let completed_chain = self.containing_message_chain;
                diagnostics
                    .into_iter()
                    .map(|mut captured| {
                        if captured.used_containing_message_chain {
                            captured.diagnostic.message = completed_chain
                                .clone()
                                .expect("a chain-consuming relation keeps its containing chain");
                        }
                        captured.diagnostic
                    })
                    .collect()
            }
        }
    }
}

impl<'a> CheckerState<'a> {
    /// tsrs-native: the captured-elaboration face of checkTypeRelatedTo's
    /// containingMessageChain callback. A relation-produced row advances the
    /// one chain owned by the applicability run; fallback diagnostics which
    /// bypass that relation output remain unwrapped.
    pub(crate) fn capture_type_assignable_to_diagnostic_for_sink(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
        head_message: &'static DiagnosticMessage,
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<(bool, Option<Diagnostic>, bool)> {
        self.capture_type_assignable_to_diagnostic_with_containing_chain(
            source,
            target,
            error_node,
            head_message,
            &mut sink.containing_message_chain,
        )
    }

    /// tsrs-native: direct shared-chain adapter used by applicability paths
    /// which do not own an elaboration sink.
    pub(crate) fn capture_type_assignable_to_diagnostic_with_containing_chain(
        &mut self,
        source: TypeId,
        target: TypeId,
        error_node: NodeId,
        head_message: &'static DiagnosticMessage,
        containing_message_chain: &mut Option<MessageChain>,
    ) -> CheckResult<(bool, Option<Diagnostic>, bool)> {
        let Some(containing_message_chain) = containing_message_chain.as_mut() else {
            let (related, diagnostic) = self.capture_type_assignable_to_diagnostic(
                source,
                target,
                error_node,
                head_message,
            )?;
            return Ok((related, diagnostic, false));
        };
        let generic_head = std::ptr::eq(
            head_message,
            &diagnostics::Type_0_is_not_assignable_to_type_1,
        );
        let (related, output) = self.check_relation_with_shared_message_chain_at(
            source,
            target,
            RelationKind::Assignable,
            (!generic_head).then_some(head_message),
            containing_message_chain,
            Some(error_node),
        )?;
        if let Some(output) = output {
            let mut diagnostic =
                self.create_error(output.error_node.or(Some(error_node)), head_message, &[]);
            diagnostic.message = output.message;
            diagnostic.related = output.related;
            return Ok((related, Some(diagnostic), true));
        }
        if related {
            return Ok((true, None, false));
        }

        // Overflow and contained-abort fallbacks are direct diagnostics in
        // the Rust projection. They never observed the shared upstream
        // callback, so keep them direct instead of fabricating a prefix.
        let (related, diagnostic) =
            self.capture_type_assignable_to_diagnostic(source, target, error_node, head_message)?;
        Ok((related, diagnostic, false))
    }

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
    ) -> CheckResult<Option<SignatureKind>> {
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
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<ElaborationOutcome> {
        let Some(kind) =
            self.did_you_mean_signature_kind(source, target, RelationKind::Assignable)?
        else {
            return Ok(ElaborationOutcome::Declined);
        };
        let (_, mut diagnostic, used_containing_message_chain) = self
            .capture_type_assignable_to_diagnostic_for_sink(
                source,
                target,
                node,
                head_message,
                sink,
            )?;
        if let Some(diagnostic) = &mut diagnostic {
            let related = self.related_info_for_node(
                node,
                if kind == SignatureKind::Construct {
                    &diagnostics::Did_you_mean_to_use_new_with_this_expression
                } else {
                    &diagnostics::Did_you_mean_to_call_this_expression
                },
                &[],
            );
            diagnostic.related.push(related);
        }
        if let Some(diagnostic) = diagnostic {
            sink.publish_relation(self, diagnostic, used_containing_message_chain);
            return Ok(ElaborationOutcome::Reported);
        }
        Ok(ElaborationOutcome::Declined)
    }

    /// tsc-port: getBestMatchIndexedAccessTypeOrUndefined @6.0.3
    /// tsc-hash: d9c9a56511cb15f6d99180834836ddea10da6cc2af84f172c7932f6490c2e349
    /// tsc-span: _tsc.js:64103-64114
    pub(crate) fn member_elaboration_target_type(
        &mut self,
        source_type: TypeId,
        target_type: TypeId,
        name_type: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let mut indexed = self.get_indexed_access_type_or_undefined(
            target_type,
            name_type,
            AccessFlags::NONE,
            None,
            None,
            None,
        )?;
        if indexed.is_none()
            && self
                .tables
                .flags_of(target_type)
                .intersects(TypeFlags::UNION)
        {
            if let Some(best) = self.get_best_matching_type(source_type, target_type)? {
                indexed = self.get_indexed_access_type_or_undefined(
                    best,
                    name_type,
                    AccessFlags::NONE,
                    None,
                    None,
                    None,
                )?;
            }
        }
        // elaborateElementwise deliberately declines a still-deferred
        // indexed access. Reporting the member in isolation would lose
        // the outer object relation and choose the member's source span.
        Ok(indexed.filter(|&ty| {
            !self
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::INDEXED_ACCESS)
        }))
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
    ) -> CheckResult<(TypeId, TypeId)> {
        let target_is_optional = self
            .get_property_of_type_full(target_type, name_text)?
            .is_some_and(|property| {
                self.binder
                    .symbol(property)
                    .flags
                    .intersects(tsc_types::SymbolFlags::OPTIONAL)
            });
        let source_is_optional = if target_is_optional {
            self.get_property_of_type_full(source_type, name_text)?
                .is_some_and(|property| {
                    self.binder
                        .symbol(property)
                        .flags
                        .intersects(tsc_types::SymbolFlags::OPTIONAL)
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
    pub(crate) fn elementwise_elaboration_related(
        &mut self,
        target_type: TypeId,
        name_type: TypeId,
    ) -> CheckResult<Option<RelatedInfo>> {
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
                return Ok(Some(self.related_info_for_node(
                    declaration,
                    &diagnostics::The_expected_type_comes_from_this_index_signature,
                    &[],
                )));
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
            return Ok(None);
        };

        let property_display = match property_name {
            Some(ref property_name)
                if !self
                    .tables
                    .flags_of(name_type)
                    .intersects(TypeFlags::UNIQUE_ES_SYMBOL) =>
            {
                tsc_binder::unescape_leading_underscores(property_name).to_owned()
            }
            _ => {
                let Ok(display) = self.type_to_string_slice(name_type) else {
                    return Ok(None);
                };
                display
            }
        };
        let Ok(target_text) = self.type_to_string_slice(target_type) else {
            return Ok(None);
        };
        Ok(Some(self.related_info_for_node(
            target_node,
            &diagnostics::The_expected_type_comes_from_property_0_which_is_declared_here_on_type_1,
            &[&property_display, &target_text],
        )))
    }

    /// tsc-port: elaborateArrowFunction @6.0.3
    /// tsc-hash: 592e789cf5d2404080e9d8b355099c7a7f0e7580287399e52d8b25a4950b9cd7
    /// tsc-span: _tsc.js:64024-64102
    fn arrow_return_elaboration_related(&mut self, target_type: TypeId) -> Option<RelatedInfo> {
        let declaration = self
            .tables
            .type_of(target_type)
            .symbol
            .and_then(|symbol| self.binder.symbol(symbol).declarations.first().copied());
        let declaration = declaration?;
        Some(self.related_info_for_node(
            declaration,
            &diagnostics::The_expected_type_comes_from_the_return_type_of_this_signature,
            &[],
        ))
    }

    /// filterType(childrenTargetType, predicate), specialized to the
    /// array-like/non-array-like partition in elaborateJsxComponents.
    fn partition_jsx_children_target(
        &mut self,
        children_target: TypeId,
    ) -> CheckResult<(TypeId, TypeId)> {
        let parts = match &self.tables.type_of(children_target).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => vec![children_target],
        };
        let iterable = self.get_global_iterable_type(/*report_errors*/ false)?;
        let any_iterable = if iterable == self.empty_generic_type {
            None
        } else {
            Some(self.create_iterable_type(self.tables.intrinsics.any)?)
        };
        let mut array_like = Vec::new();
        let mut non_array_like = Vec::new();
        for part in parts {
            let is_array_like = if let Some(any_iterable) = any_iterable {
                self.is_type_assignable_to(part, any_iterable)?
            } else {
                self.is_array_like_type(part)? || self.is_tuple_like_type(part)?
            };
            if is_array_like {
                array_like.push(part);
            } else {
                non_array_like.push(part);
            }
        }
        let array_like = self.get_union_type_ex(&array_like, UnionReduction::Literal)?;
        let non_array_like = self.get_union_type_ex(&non_array_like, UnionReduction::Literal)?;
        Ok((array_like, non_array_like))
    }

    /// The report-pair body shared by elaborateElementwise and
    /// elaborateIterableOrArrayLikeTargetElementwise for JSX children.
    /// `inner_expression` deliberately differs from `error_node` for a
    /// JsxExpression: nested elaboration enters the expression, while a
    /// non-elaborated relation row remains anchored to the JSX child.
    #[allow(clippy::too_many_arguments)]
    fn elaborate_jsx_child_pair(
        &mut self,
        error_node: NodeId,
        inner_expression: Option<NodeId>,
        source_container: TypeId,
        target_container: TypeId,
        related_target: Option<TypeId>,
        name_type: TypeId,
        source_property_type: TypeId,
        target_property_type: TypeId,
        relation: RelationKind,
        invalid_text: Option<(&str, &str, &str)>,
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<bool> {
        if self.check_type_related_to(source_property_type, target_property_type, relation)? {
            return Ok(false);
        }
        if let Some(inner_expression) = inner_expression {
            if self
                .elaborate_assignment_relation(
                    inner_expression,
                    source_property_type,
                    target_property_type,
                    None,
                    sink,
                )?
                .reported()
            {
                return Ok(true);
            }
        }

        let specific_source = if let Some(inner_expression) = inner_expression {
            self.push_contextual_type(
                inner_expression,
                Some(source_property_type),
                /*is_cache*/ false,
            );
            let result = self.check_expression_for_mutable_location(
                inner_expression,
                CheckMode::CONTEXTUAL,
                /*force_tuple*/ false,
            );
            self.pop_contextual_type();
            result?
        } else {
            source_property_type
        };
        let name_text = self
            .property_name_from_type_usable(name_type)
            .unwrap_or_default();
        let original_target = target_property_type;
        let (source_property_type, target_property_type) = self.remove_missing_for_member_report(
            source_container,
            target_container,
            &name_text,
            source_property_type,
            target_property_type,
        )?;
        let (specific_source, _) = self.remove_missing_for_member_report(
            source_container,
            target_container,
            &name_text,
            specific_source,
            original_target,
        )?;
        let mut used_containing_message_chain = false;
        let mut diagnostic = if let Some((tag_name, children_name, children_target)) = invalid_text
        {
            Some(self.create_error(
                Some(error_node),
                &diagnostics::_0_components_don_t_accept_text_as_child_elements_Text_in_JSX_has_the_type_string_but_the_expected_type_of_1_is_2,
                &[tag_name, children_name, children_target],
            ))
        } else {
            let (specific_related, mut diagnostic, used_chain) = self
                .capture_type_assignable_to_diagnostic_for_sink(
                    specific_source,
                    target_property_type,
                    error_node,
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                    sink,
                )?;
            used_containing_message_chain = used_chain;
            if specific_related && specific_source != source_property_type {
                let (_, fallback, fallback_used_chain) = self
                    .capture_type_assignable_to_diagnostic_for_sink(
                        source_property_type,
                        target_property_type,
                        error_node,
                        &diagnostics::Type_0_is_not_assignable_to_type_1,
                        sink,
                    )?;
                if fallback.is_some() {
                    diagnostic = fallback;
                    used_containing_message_chain = fallback_used_chain;
                }
            }
            diagnostic
        };
        if let (Some(diagnostic), Some(related_target)) = (&mut diagnostic, related_target) {
            if let Some(related) =
                self.elementwise_elaboration_related(related_target, name_type)?
            {
                diagnostic.related.push(related);
            }
        }
        if let Some(diagnostic) = diagnostic {
            sink.publish_relation(self, diagnostic, used_containing_message_chain);
            return Ok(true);
        }
        Ok(false)
    }

    fn jsx_child_inner_expression(&self, child: NodeId) -> Option<NodeId> {
        match self.data_of(child) {
            NodeData::JsxExpression(data) => data.expression,
            NodeData::JsxElement(_)
            | NodeData::JsxSelfClosingElement(_)
            | NodeData::JsxFragment(_) => Some(child),
            NodeData::JsxText(_) => None,
            _ => None,
        }
    }

    /// elaborateIterableOrArrayLikeTargetElementwise for the tuple
    /// synthesized from multiple semantic JSX children.
    #[allow(clippy::too_many_arguments)]
    fn elaborate_multiple_jsx_children(
        &mut self,
        containing_element: NodeId,
        semantic_children: &[NodeId],
        array_like_target: TypeId,
        children_name: &str,
        children_target: TypeId,
        tag_name_text: &str,
        relation: RelationKind,
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<bool> {
        let child_types = self.check_jsx_children(containing_element, CheckMode::NORMAL)?;
        let tuple_source =
            self.create_tuple_type_forced(&child_types, None, /*readonly*/ false, None)?;

        // Iterable targets need their yielded type in addition to the
        // ordinary numeric indexed access supplied by arrays/tuples.
        let target_parts = match &self.tables.type_of(array_like_target).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => vec![array_like_target],
        };
        let mut tuple_or_array_parts = Vec::new();
        let mut iterable_only_parts = Vec::new();
        for part in target_parts {
            if self.is_array_like_type(part)? || self.is_tuple_like_type(part)? {
                tuple_or_array_parts.push(part);
            } else {
                iterable_only_parts.push(part);
            }
        }
        let tuple_or_array_target =
            self.get_union_type_ex(&tuple_or_array_parts, UnionReduction::Literal)?;
        let iterable_only_target =
            self.get_union_type_ex(&iterable_only_parts, UnionReduction::Literal)?;
        let iteration_type = if iterable_only_target == self.tables.intrinsics.never {
            None
        } else {
            self.get_iteration_type_of_iterable(
                IterationUse::FOR_OF,
                IterationTypeKind::YIELD,
                iterable_only_target,
                None,
            )?
        };

        let children_target_text = self.type_to_string_slice(children_target)?;
        let mut reported = false;
        for (index, &child) in semantic_children.iter().enumerate() {
            let name_type = self.tables.get_number_literal_type(index as f64);
            let indexed_target = if tuple_or_array_target == self.tables.intrinsics.never {
                None
            } else {
                self.member_elaboration_target_type(tuple_source, tuple_or_array_target, name_type)?
            };
            let target_property_type = match (iteration_type, indexed_target) {
                (Some(iteration), Some(indexed)) => {
                    Some(self.get_union_type_ex(&[iteration, indexed], UnionReduction::Literal)?)
                }
                (Some(iteration), None) => Some(iteration),
                (None, Some(indexed)) => Some(indexed),
                (None, None) => None,
            };
            let Some(target_property_type) = target_property_type else {
                continue;
            };
            let Some(source_property_type) = self.get_indexed_access_type_or_undefined(
                tuple_source,
                name_type,
                AccessFlags::NONE,
                None,
                None,
                None,
            )?
            else {
                continue;
            };
            let invalid_text = (self.kind_of(child) == SyntaxKind::JsxText).then_some((
                tag_name_text,
                children_name,
                children_target_text.as_str(),
            ));
            if self.elaborate_jsx_child_pair(
                child,
                self.jsx_child_inner_expression(child),
                tuple_source,
                tuple_or_array_target,
                None,
                name_type,
                source_property_type,
                target_property_type,
                relation,
                invalid_text,
                sink,
            )? {
                reported = true;
            }
        }
        Ok(reported)
    }

    /// tsc-port: elaborateJsxComponents @6.0.3 children slice
    /// tsc-hash: f9cd0f08f4fca00f4deaa1607244478608e6fe98975c19e5836677da74a11b08
    /// tsc-span: _tsc.js:64310-64385
    fn elaborate_jsx_children(
        &mut self,
        attributes: NodeId,
        source_type: TypeId,
        target_type: TypeId,
        relation: RelationKind,
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<bool> {
        let Some(opening) = self.parent_of(attributes) else {
            return Ok(false);
        };
        if self.kind_of(opening) != SyntaxKind::JsxOpeningElement {
            return Ok(false);
        }
        let Some(containing_element) = self.parent_of(opening) else {
            return Ok(false);
        };
        let (children, tag_name) = match self.data_of(containing_element) {
            NodeData::JsxElement(data) if data.opening_element == Some(opening) => {
                let tag_name = match self.data_of(opening) {
                    NodeData::JsxOpeningElement(opening) => opening.tag_name,
                    _ => None,
                };
                (self.nodes_of(data.children), tag_name)
            }
            _ => return Ok(false),
        };
        let Some(tag_name) = tag_name else {
            return Ok(false);
        };
        let semantic_children: Vec<NodeId> = children
            .into_iter()
            .filter(|&child| self.is_semantic_jsx_child(child))
            .collect();
        if semantic_children.is_empty() {
            return Ok(false);
        }
        let jsx_namespace = self.get_jsx_namespace_at(attributes)?;
        let escaped_children_name = self
            .get_jsx_element_children_property_name(jsx_namespace)?
            .unwrap_or_else(|| "children".to_owned());
        let children_name =
            tsc_binder::unescape_leading_underscores(&escaped_children_name).to_owned();
        let name_type = self.tables.get_string_literal_type(&children_name);
        let children_target = self.get_indexed_access_type(
            target_type,
            name_type,
            AccessFlags::NONE,
            None,
            None,
            None,
        )?;
        let (array_like_target, non_array_like_target) =
            self.partition_jsx_children_target(children_target)?;
        let source_children = self.get_indexed_access_type(
            source_type,
            name_type,
            AccessFlags::NONE,
            None,
            None,
            None,
        )?;
        let tag_name_text = self.text_of_node(tag_name)?;
        let children_target_text = self.type_to_string_slice(children_target)?;

        if semantic_children.len() > 1 {
            if array_like_target != self.tables.intrinsics.never {
                return self.elaborate_multiple_jsx_children(
                    containing_element,
                    &semantic_children,
                    array_like_target,
                    &children_name,
                    children_target,
                    &tag_name_text,
                    relation,
                    sink,
                );
            }
            if !self.check_type_related_to(source_children, children_target, relation)? {
                let diagnostic = self.create_error(
                    Some(tag_name),
                    &diagnostics::This_JSX_tag_s_0_prop_expects_a_single_child_of_type_1_but_multiple_children_were_provided,
                    &[&children_name, &children_target_text],
                );
                sink.publish_and_capture(self, diagnostic);
                return Ok(true);
            }
            return Ok(false);
        }

        if non_array_like_target != self.tables.intrinsics.never {
            let Some(target_property_type) =
                self.member_elaboration_target_type(source_type, target_type, name_type)?
            else {
                return Ok(false);
            };
            let Some(source_property_type) = self.get_indexed_access_type_or_undefined(
                source_type,
                name_type,
                AccessFlags::NONE,
                None,
                None,
                None,
            )?
            else {
                return Ok(false);
            };
            let child = semantic_children[0];
            let invalid_text = (self.kind_of(child) == SyntaxKind::JsxText).then_some((
                tag_name_text.as_str(),
                children_name.as_str(),
                children_target_text.as_str(),
            ));
            return self.elaborate_jsx_child_pair(
                child,
                self.jsx_child_inner_expression(child),
                source_type,
                target_type,
                Some(target_type),
                name_type,
                source_property_type,
                target_property_type,
                relation,
                invalid_text,
                sink,
            );
        }
        if !self.check_type_related_to(source_children, children_target, relation)? {
            let diagnostic = self.create_error(
                Some(tag_name),
                &diagnostics::This_JSX_tag_s_0_prop_expects_type_1_which_requires_multiple_children_but_only_a_single_child_was_provided,
                &[&children_name, &children_target_text],
            );
            sink.publish_and_capture(self, diagnostic);
            return Ok(true);
        }
        Ok(false)
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
    ) -> CheckResult<bool> {
        if self.is_type_assignable_to(source_type, target_type)? {
            return Ok(true);
        }
        if error_node.is_some()
            && self
                .elaborate_literal_assignment_from_types(
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
    ) -> CheckResult<ElaborationOutcome> {
        let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
        self.elaborate_literal_assignment_from_types(
            expression,
            source_type,
            target_type,
            probe_head,
        )
    }

    /// tsrs-native: adapt elaborateError's assignment relation to an
    /// explicitly borrowed Rust diagnostic sink.
    pub(crate) fn elaborate_literal_assignment_into_sink(
        &mut self,
        expression: NodeId,
        target_type: TypeId,
        probe_head: Option<&'static DiagnosticMessage>,
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<ElaborationOutcome> {
        let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
        self.elaborate_assignment_relation(expression, source_type, target_type, probe_head, sink)
    }

    fn elaborate_literal_assignment_from_types(
        &mut self,
        expression: NodeId,
        source_type: TypeId,
        target_type: TypeId,
        probe_head: Option<&'static DiagnosticMessage>,
    ) -> CheckResult<ElaborationOutcome> {
        let mut sink = ElaborationDiagnosticSink::program();
        self.elaborate_assignment_relation(
            expression,
            source_type,
            target_type,
            probe_head,
            &mut sink,
        )
    }

    /// tsrs-native: own the diagnostics produced through tsc's
    /// errorOutputContainer face for call and JSX applicability.
    ///
    /// The `errorOutputContainer` face consumed by call/JSX
    /// applicability. Only diagnostics explicitly reported by the
    /// elaboration frame are returned; lazy global diagnostics stay in
    /// the program sink.
    pub(crate) fn capture_literal_assignment_elaboration(
        &mut self,
        expression: NodeId,
        target_type: TypeId,
        probe_head: Option<&'static DiagnosticMessage>,
        containing_message_chain: Option<MessageChain>,
    ) -> CheckResult<(ElaborationOutcome, Vec<Diagnostic>)> {
        let source_type = self.check_expression_cached(expression, CheckMode::NORMAL)?;
        let mut sink = ElaborationDiagnosticSink::captured(containing_message_chain);
        let outcome = self.elaborate_assignment_relation(
            expression,
            source_type,
            target_type,
            probe_head,
            &mut sink,
        )?;
        Ok((outcome, sink.into_captured()))
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
        sink: &mut ElaborationDiagnosticSink,
    ) -> CheckResult<ElaborationOutcome> {
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
                    sink,
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
                        sink,
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
                            sink,
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
                        sink,
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
                            sink,
                        );
                    }
                }
            }
            _ => {}
        }
        let report_mark = sink.mark();
        match self.data_of(expression) {
            NodeData::ArrowFunction(data) => {
                let data = data.clone();
                let Some(body) = data.body else {
                    return Ok(ElaborationOutcome::Declined);
                };
                if matches!(self.data_of(body), NodeData::Block(_)) {
                    return Ok(ElaborationOutcome::Declined);
                }
                let any_annotated = self.nodes_of(data.parameters).iter().any(|&parameter| {
                    matches!(self.data_of(parameter), NodeData::Parameter(data)
                        if data.r#type.is_some())
                });
                if any_annotated {
                    return Ok(ElaborationOutcome::Declined);
                }
                let Some(source_signature) = self.get_single_call_signature(source_type)? else {
                    return Ok(ElaborationOutcome::Declined);
                };
                let target_signatures =
                    self.get_signatures_of_type(target_type, SignatureKind::Call)?;
                if target_signatures.is_empty() {
                    return Ok(ElaborationOutcome::Declined);
                }
                let source_return = self.get_return_type_of_signature(source_signature)?;
                let mut target_returns = Vec::with_capacity(target_signatures.len());
                for signature in target_signatures {
                    target_returns.push(self.get_return_type_of_signature(signature)?);
                }
                let target_return =
                    self.get_union_type_ex(&target_returns, UnionReduction::Literal)?;
                if self.is_type_assignable_to(source_return, target_return)? {
                    return Ok(ElaborationOutcome::Declined);
                }
                if self
                    .elaborate_assignment_relation(
                        body,
                        source_return,
                        target_return,
                        Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                        sink,
                    )?
                    .reported()
                {
                    return Ok(ElaborationOutcome::Reported);
                }
                let (_, mut diagnostic, used_containing_message_chain) = self
                    .capture_type_assignable_to_diagnostic_for_sink(
                        source_return,
                        target_return,
                        body,
                        &diagnostics::Type_0_is_not_assignable_to_type_1,
                        sink,
                    )?;
                if let Some(diagnostic) = &mut diagnostic {
                    if let Some(related) = self.arrow_return_elaboration_related(target_type) {
                        diagnostic.related.push(related);
                    }
                }
                if let Some(diagnostic) = diagnostic {
                    sink.publish_relation(self, diagnostic, used_containing_message_chain);
                    return Ok(ElaborationOutcome::Reported);
                }
                return Ok(ElaborationOutcome::Declined);
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
                        name_type,
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
                                sink,
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
                    let (specific_related, mut diagnostic, mut used_containing_message_chain) =
                        self.capture_type_assignable_to_diagnostic_for_sink(
                            specific_source,
                            expected,
                            name,
                            message,
                            sink,
                        )?;
                    // 64168-64170: if contextual rechecking made the
                    // syntax-specific source pass, report against the
                    // indexed source property that originally failed.
                    if specific_related && specific_source != source_property_type {
                        let (_, fallback, fallback_used_chain) = self
                            .capture_type_assignable_to_diagnostic_for_sink(
                                source_property_type,
                                expected,
                                name,
                                message,
                                sink,
                            )?;
                        if fallback.is_some() {
                            diagnostic = fallback;
                            used_containing_message_chain = fallback_used_chain;
                        }
                    }
                    if let Some(diagnostic) = &mut diagnostic {
                        if let Some(related) =
                            self.elementwise_elaboration_related(target_type, name_type)?
                        {
                            diagnostic.related.push(related);
                        }
                    }
                    if let Some(diagnostic) = diagnostic {
                        sink.publish_relation(self, diagnostic, used_containing_message_chain);
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
                    let name_type = self.tables.get_number_literal_type(index as f64);
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
                            name_type,
                        )? {
                            Some(expected) => expected,
                            None => continue,
                        }
                    };
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
                            sink,
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
                    let (specific_related, mut diagnostic, mut used_containing_message_chain) =
                        self.capture_type_assignable_to_diagnostic_for_sink(
                            specific_source,
                            expected,
                            error_node,
                            &diagnostics::Type_0_is_not_assignable_to_type_1,
                            sink,
                        )?;
                    if specific_related && specific_source != source_property_type {
                        let (_, fallback, fallback_used_chain) = self
                            .capture_type_assignable_to_diagnostic_for_sink(
                                source_property_type,
                                expected,
                                error_node,
                                &diagnostics::Type_0_is_not_assignable_to_type_1,
                                sink,
                            )?;
                        if fallback.is_some() {
                            diagnostic = fallback;
                            used_containing_message_chain = fallback_used_chain;
                        }
                    }
                    if let Some(diagnostic) = &mut diagnostic {
                        if let Some(related) =
                            self.elementwise_elaboration_related(target_type, name_type)?
                        {
                            diagnostic.related.push(related);
                        }
                    }
                    if let Some(diagnostic) = diagnostic {
                        sink.publish_relation(self, diagnostic, used_containing_message_chain);
                    }
                }
            }
            NodeData::JsxAttributes(_) => {
                let attributes_walk_reported = self.elaborate_jsx_named_attributes(
                    expression,
                    source_type,
                    target_type,
                    RelationKind::Assignable,
                    sink,
                )?;
                let children_walk_reported = self.elaborate_jsx_children(
                    expression,
                    source_type,
                    target_type,
                    RelationKind::Assignable,
                    sink,
                )?;
                if attributes_walk_reported || children_walk_reported {
                    return Ok(ElaborationOutcome::Reported);
                }
            }
            _ => {}
        }
        Ok(ElaborationOutcome::from_reported(
            sink.reported_since(report_mark),
        ))
    }
}

#[cfg(test)]
#[path = "../tests/unit/elaboration/tests.rs"]
mod tests;

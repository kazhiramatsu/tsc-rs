//! structuredTypeRelatedTo and the structural relation arms
//! (m3-types-relations-steps.md stage 4.6, checker-key §1.4).
//!
//! Arm ORDER is tsc's (the dispatch is pinned even where an arm's
//! input types are unconstructible before M4 — those arms report
//! Unsupported with the blocking machinery named). LIVE in M3:
//! identity dispatch, tuple targets, propertiesRelatedTo,
//! signaturesRelatedTo (compareSignaturesRelated + arity helpers),
//! indexInfosRelatedTo/membersRelatedToIndexInfo, the TemplateLiteral
//! target arm, and typeRelatedToDiscriminatedType with synthetic
//! union/intersection properties. DEAD until M4: type-parameter,
//! keyof/Index, IndexedAccess, Conditional, Substitution, Mapped,
//! StringMapping, reference variance (relateVariances falls through
//! to structural for tuples by construction — getVariances gives
//! tuples arrayVariances, but tuples are excluded from the reference
//! fast path and take the propertiesRelatedTo tuple arm).

use tsrs2_binder::SymbolId;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    AccessFlags, CheckFlags, ElementFlags, IndexFlags, IntersectionState, ModifierFlags,
    ObjectFlags, PseudoBigInt, RecursionFlags, SymbolFlags, TemplateText, Ternary,
    TupleTargetFlags, TypeData, TypeFlags, TypeId, UnionReduction,
};

use crate::engine::{is_false, is_true, ternary_and, RelationChecker};
use crate::relate::RelationKind;
pub use crate::state::SignatureKind;
use crate::state::{CheckResult2, CheckerState, IndexInfo, SignatureId, Unsupported};

/// tsc SignatureCheckMode (inlined const enum).
mod check_mode {
    pub const NONE: i32 = 0;
    pub const BIVARIANT_CALLBACK: i32 = 1;
    pub const STRICT_CALLBACK: i32 = 2;
    pub const CALLBACK: i32 = 3;
    pub const IGNORE_RETURN_TYPES: i32 = 4;
    pub const STRICT_ARITY: i32 = 8;
    pub const STRICT_TOP_SIGNATURE: i32 = 16;
}

impl<'r, 'a> RelationChecker<'r, 'a> {
    /// `type.target.readonly` for tuple references (the fast-path
    /// guards at 66095-66097).
    fn tuple_target_readonly(&self, ty: TypeId) -> bool {
        let target = self.st.tables.reference_target(ty);
        match &self.st.tables.type_of(target).data {
            TypeData::TupleTarget(data) => data.readonly,
            _ => unreachable!("tuple references target tuple targets"),
        }
    }

    /// tsc-port: relateVariances @6.0.3
    /// tsc-hash: 916b6e76908a21c730b24b7d5624ff5e318879f740e4b2be422b1491e906a2d2
    /// tsc-span: _tsc.js:66488-66507
    ///
    /// `Some(result)` is a definite verdict; `None` falls through to
    /// the structural arms. reportErrors=false collapses the tail:
    /// the AllowsStructuralFallback and covariant-void paths reach the
    /// structural fallback with varianceCheckFailed=false, and the
    /// remaining path — `varianceCheckFailed && !(reportErrors2 &&
    /// some invariant)` — always returns False, so varianceCheckFailed
    /// can never be true at a structural fallback and the errorInfo
    /// juggling (originalErrorInfo/resetErrorInfo, 66468-66471) stays
    /// elided with the error machinery. The `variances !== emptyArray`
    /// conjunct is always true here: both call sites pre-answer the
    /// in-progress sentinel with Ternary.Unknown.
    fn relate_variances(
        &mut self,
        source_type_arguments: &[TypeId],
        target_type_arguments: &[TypeId],
        variances: &[tsrs2_types::VarianceFlags],
        report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Option<Ternary>> {
        let result = self.type_arguments_related_to(
            source_type_arguments,
            target_type_arguments,
            variances,
            report_errors,
            intersection_state,
        )?;
        if !is_false(result) {
            return Ok(Some(result));
        }
        if variances
            .iter()
            .any(|v| v.intersects(tsrs2_types::VarianceFlags::ALLOWS_STRUCTURAL_FALLBACK))
        {
            return Ok(None);
        }
        let allow_structural_fallback = self
            .st
            .has_covariant_void_argument(target_type_arguments, variances);
        if !allow_structural_fallback {
            return Ok(Some(Ternary::FALSE));
        }
        Ok(None)
    }

    /// tsc-port: structuredTypeRelatedTo @6.0.3
    /// tsc-hash: bccafd822efb034656afe7f2bc249a4f735cb74edadd3218be0428f5000a973c
    /// tsc-span: _tsc.js:65872-65929
    ///
    /// getEffectiveConstraintOfIntersection contributes only when an
    /// intersection member is Instantiable — unconstructible in M3, so
    /// the constraint retry is faithfully None. The optionalsOnly
    /// intersection-source arm keys on getApparentType being
    /// structured — the M3 apparent slice covers it.
    pub(crate) fn structured_type_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let mut result = self.structured_type_related_to_worker(
            source,
            target,
            report_errors,
            intersection_state,
        )?;
        if self.relation != RelationKind::Identity {
            // Intersection-source constraint retry (65876-65890):
            // getEffectiveConstraintOfIntersection is None without
            // Instantiable members.
            if is_true(result)
                && !intersection_state.intersects(IntersectionState::TARGET)
                && self.flags(target).intersects(TypeFlags::INTERSECTION)
                && self.flags(source).intersects(TypeFlags::from_bits(
                    TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
                ))
            {
                let props = self.properties_related_to(
                    source,
                    target,
                    /*excluded_properties*/ None,
                    /*optionals_only*/ false,
                    IntersectionState::NONE,
                )?;
                result = ternary_and(result, props);
                if is_true(result)
                    && self.st.is_object_literal_type(source)
                    && self
                        .st
                        .tables
                        .object_flags_of(source)
                        .intersects(ObjectFlags::FRESH_LITERAL)
                {
                    let index = self.index_signatures_related_to(
                        source,
                        target,
                        /*source_is_primitive*/ false,
                        IntersectionState::NONE,
                    )?;
                    result = ternary_and(result, index);
                }
            } else if is_true(result)
                && self.is_non_generic_object_type(target)
                && !self.st.tables.is_tuple_type(target)
                && self.flags(source).intersects(TypeFlags::INTERSECTION)
                && self
                    .st
                    .get_apparent_type(source)
                    .map(|apparent| self.flags(apparent).intersects(TypeFlags::STRUCTURED_TYPE))
                    .unwrap_or(false)
                && !self.union_members(source).iter().any(|&t| {
                    t == target
                        || self
                            .st
                            .tables
                            .object_flags_of(t)
                            .intersects(ObjectFlags::NON_INFERRABLE_TYPE)
                })
            {
                let props = self.properties_related_to(
                    source,
                    target,
                    /*excluded_properties*/ None,
                    /*optionals_only*/ true,
                    intersection_state,
                )?;
                result = ternary_and(result, props);
            }
        }
        Ok(result)
    }

    fn is_non_generic_object_type(&self, ty: TypeId) -> bool {
        self.flags(ty).intersects(TypeFlags::OBJECT)
    }

    /// tsc-port: structuredTypeRelatedToWorker @6.0.3
    /// tsc-hash: ea2b96cb6d324bdf3b0ad08fc094695206962947eb05d268c3b4cd227eaf008b
    /// tsc-span: _tsc.js:65942-66507
    ///
    /// The M3 arm dispositions, in tsc's order:
    /// - identity: unionOrIntersection/TemplateLiteral arms LIVE;
    ///   Index/IndexedAccess/Conditional/Substitution/StringMapping
    ///   flags are unconstructible.
    /// - alias-variance block: alias symbols are M4 (guard false).
    /// - single-element generic tuples: generic tuples are M4.
    /// - target TypeParameter/Index/IndexedAccess/Mapped/Conditional
    ///   arms: Unsupported (M4 5.1-5.3).
    /// - target TemplateLiteral arm LIVE (the 4.2/4.3 template stub
    ///   call sites route here); target StringMapping Unsupported.
    /// - source TypeVariable/Index/Conditional arms Unsupported; the
    ///   source TemplateLiteral/StringMapping constraint arms reduce
    ///   to getBaseConstraintOrType = stringType for templates.
    /// - final object block: getApparentType M3 slice; the reference
    ///   variance fast path excludes tuples by construction and no
    ///   other same-target references exist before M4.
    #[allow(clippy::collapsible_if)]
    fn structured_type_related_to_worker(
        &mut self,
        source: TypeId,
        target: TypeId,
        report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let mut source = source;
        let mut source_flags = self.flags(source);
        let target_flags = self.flags(target);
        if self.relation == RelationKind::Identity {
            if source_flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
                let mut result = self.each_type_related_to_some_type(source, target)?;
                if is_true(result) {
                    result =
                        ternary_and(result, self.each_type_related_to_some_type(target, source)?);
                }
                return Ok(result);
            }
            if source_flags.intersects(TypeFlags::INDEX) {
                // 65956-65964: keyof identity compares the operands
                // (flags equality holds — engine.rs isIdenticalTo).
                let TypeData::Index {
                    ty: source_inner, ..
                } = self.st.tables.type_of(source).data
                else {
                    unreachable!("index flag implies index data");
                };
                let TypeData::Index {
                    ty: target_inner, ..
                } = self.st.tables.type_of(target).data
                else {
                    unreachable!("index flag implies index data");
                };
                return self.is_related_to(
                    source_inner,
                    target_inner,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                );
            }
            if source_flags.intersects(TypeFlags::INDEXED_ACCESS) {
                // 65965-65983: componentwise object/index identity;
                // failure falls through to the non-object tail.
                let TypeData::IndexedAccess {
                    object_type: source_object,
                    index_type: source_index,
                    ..
                } = self.st.tables.type_of(source).data
                else {
                    unreachable!("indexed-access flag implies indexed-access data");
                };
                let TypeData::IndexedAccess {
                    object_type: target_object,
                    index_type: target_index,
                    ..
                } = self.st.tables.type_of(target).data
                else {
                    unreachable!("indexed-access flag implies indexed-access data");
                };
                let mut result = self.is_related_to(
                    source_object,
                    target_object,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if !is_false(result) {
                    result = ternary_and(
                        result,
                        self.is_related_to(
                            source_index,
                            target_index,
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            IntersectionState::NONE,
                        )?,
                    );
                    if !is_false(result) {
                        return Ok(result);
                    }
                }
            }
            if source_flags.intersects(TypeFlags::from_bits(
                TypeFlags::CONDITIONAL.bits() | TypeFlags::SUBSTITUTION.bits(),
            )) {
                // 65984-66039: those TypeFlags are unconstructible
                // before their type nodes land.
                return Err(Unsupported::new(
                    "identity for conditional/substitution types (unported family, M8-stub)",
                ));
            }
            if source_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                let (source_texts, source_types) = self.template_parts(source);
                let (target_texts, target_types) = self.template_parts(target);
                if source_texts == target_texts {
                    let mut result = Ternary::TRUE;
                    for (i, &s) in source_types.iter().enumerate() {
                        let related = self.is_related_to(
                            s,
                            target_types[i],
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            IntersectionState::NONE,
                        )?;
                        if !is_true(related) {
                            return Ok(Ternary::FALSE);
                        }
                        result = ternary_and(result, related);
                    }
                    return Ok(result);
                }
            }
            if source_flags.intersects(TypeFlags::STRING_MAPPING) {
                // 66059-66069: same intrinsic symbol → operand
                // identity; different symbols fall through to the
                // non-object tail.
                if self.st.tables.type_of(source).symbol == self.st.tables.type_of(target).symbol {
                    let TypeData::StringMapping { ty: source_inner } =
                        self.st.tables.type_of(source).data
                    else {
                        unreachable!("string-mapping flag implies string-mapping data");
                    };
                    let TypeData::StringMapping { ty: target_inner } =
                        self.st.tables.type_of(target).data
                    else {
                        unreachable!("string-mapping flag implies string-mapping data");
                    };
                    return self.is_related_to(
                        source_inner,
                        target_inner,
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    );
                }
            }
            if !source_flags.intersects(TypeFlags::OBJECT) {
                return Ok(Ternary::FALSE);
            }
        } else if source_flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
            || target_flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
        {
            let result = self.union_or_intersection_related_to(
                source,
                target,
                report_errors,
                intersection_state,
            )?;
            if is_true(result) {
                return Ok(result);
            }
            if !(source_flags.intersects(TypeFlags::INSTANTIABLE)
                || (source_flags.intersects(TypeFlags::OBJECT)
                    && target_flags.intersects(TypeFlags::UNION))
                || (source_flags.intersects(TypeFlags::INTERSECTION)
                    && target_flags.intersects(TypeFlags::from_bits(
                        TypeFlags::OBJECT.bits()
                            | TypeFlags::UNION.bits()
                            | TypeFlags::INSTANTIABLE.bits(),
                    ))))
            {
                return Ok(Ternary::FALSE);
            }
        }
        // 66081-66094: the same-alias variance fast path.
        if source_flags.intersects(TypeFlags::from_bits(
            TypeFlags::OBJECT.bits() | TypeFlags::CONDITIONAL.bits(),
        )) {
            let source_alias = self.st.tables.type_of(source).alias_symbol;
            let source_alias_arguments =
                self.st.tables.type_of(source).alias_type_arguments.clone();
            let target_alias = self.st.tables.type_of(target).alias_symbol;
            if let (Some(alias_symbol), Some(source_arguments)) =
                (source_alias, source_alias_arguments)
            {
                if Some(alias_symbol) == target_alias
                    && !(self.st.is_marker_type(source) || self.st.is_marker_type(target))
                {
                    match self.st.get_alias_variances(alias_symbol)? {
                        crate::variance::VariancesResult::InProgress => {
                            return Ok(Ternary::UNKNOWN);
                        }
                        crate::variance::VariancesResult::Known(variances) => {
                            let target_arguments = self
                                .st
                                .tables
                                .type_of(target)
                                .alias_type_arguments
                                .clone()
                                .expect("same-alias pairs both carry alias arguments");
                            let params = self.st.links.symbol(alias_symbol).type_parameters.clone();
                            let min_arguments =
                                self.st.get_min_type_argument_count(params.as_deref());
                            let source_types = self
                                .st
                                .fill_missing_type_arguments(
                                    Some(&source_arguments),
                                    params.as_deref(),
                                    min_arguments,
                                    /*is_javascript_implicit_any*/ false,
                                )?
                                .unwrap_or_default();
                            let target_types = self
                                .st
                                .fill_missing_type_arguments(
                                    Some(&target_arguments),
                                    params.as_deref(),
                                    min_arguments,
                                    /*is_javascript_implicit_any*/ false,
                                )?
                                .unwrap_or_default();
                            if let Some(variance_result) = self.relate_variances(
                                &source_types,
                                &target_types,
                                &variances,
                                report_errors,
                                intersection_state,
                            )? {
                                return Ok(variance_result);
                            }
                        }
                    }
                }
            }
        }
        // 66095-66097: single-element generic tuple fast paths.
        if self.st.is_single_element_generic_tuple_type(source)
            && !self.tuple_target_readonly(source)
        {
            let element = self.st.get_type_arguments(source)?[0];
            let result = self.is_related_to(
                element,
                target,
                RecursionFlags::SOURCE,
                /*report_errors*/ false,
                IntersectionState::NONE,
            )?;
            if !is_false(result) {
                return Ok(result);
            }
        }
        if self.st.is_single_element_generic_tuple_type(target) {
            let readonly = self.tuple_target_readonly(target);
            let gate = if readonly {
                true
            } else {
                let constraint = self.st.get_base_constraint_or_type(source)?;
                self.st.is_mutable_array_or_tuple(constraint)?
            };
            if gate {
                let element = self.st.get_type_arguments(target)?[0];
                let result = self.is_related_to(
                    source,
                    element,
                    RecursionFlags::TARGET,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if !is_false(result) {
                    return Ok(result);
                }
            }
        }
        if target_flags.intersects(TypeFlags::TYPE_PARAMETER) {
            // 66098-66107: the mapped-source index-signature sub-arm
            // is dead — mapped types are unconstructible until M8
            // (getObjectFlags(source) & Mapped never set).
            if self.relation == RelationKind::Comparable
                && source_flags.intersects(TypeFlags::TYPE_PARAMETER)
            {
                // 66108-66120: chase the source constraint while it
                // still mentions type parameters.
                let mut constraint = self.st.get_constraint_of_type_parameter(source)?;
                while let Some(current) = constraint {
                    if !self.st.some_type(current, |st, c| {
                        st.tables.flags_of(c).intersects(TypeFlags::TYPE_PARAMETER)
                    }) {
                        break;
                    }
                    let result = self.is_related_to(
                        current,
                        target,
                        RecursionFlags::SOURCE,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    )?;
                    if !is_false(result) {
                        return Ok(result);
                    }
                    constraint = self.st.get_constraint_of_type_parameter(current)?;
                }
                return Ok(Ternary::FALSE);
            }
        }
        if target_flags.intersects(TypeFlags::INDEX) {
            // 66126-66163: keyof targets.
            let TypeData::Index {
                ty: target_type,
                index_flags: target_index_flags,
            } = self.st.tables.type_of(target).data
            else {
                unreachable!("index flag implies index data");
            };
            // 66128-66138: keyof S related to keyof T when T is
            // related to S (contravariant operands).
            if source_flags.intersects(TypeFlags::INDEX) {
                let TypeData::Index {
                    ty: source_type, ..
                } = self.st.tables.type_of(source).data
                else {
                    unreachable!("index flag implies index data");
                };
                let result = self.is_related_to(
                    target_type,
                    source_type,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if !is_false(result) {
                    return Ok(result);
                }
            }
            if self.st.tables.is_tuple_type(target_type) {
                // 66139-66143: a type relates to keyof [tuple] through
                // the union of the tuple's known keys.
                let known_keys = self.st.get_known_keys_of_tuple_type(target_type)?;
                let result = self.is_related_to(
                    source,
                    known_keys,
                    RecursionFlags::TARGET,
                    report_errors,
                    IntersectionState::NONE,
                )?;
                if !is_false(result) {
                    return Ok(result);
                }
            } else {
                // 66144-66150: S related to keyof T when S relates to
                // keyof C over the simplified form or constraint of T
                // (TRUE verdicts only).
                let constraint = self.st.get_simplified_type_or_constraint(target_type)?;
                if let Some(constraint) = constraint {
                    let index_flags = IndexFlags::from_bits(
                        target_index_flags.bits() | IndexFlags::NO_REDUCIBLE_CHECK.bits(),
                    );
                    let keyof_constraint = self.st.get_index_type(constraint, index_flags)?;
                    if is_true(self.is_related_to(
                        source,
                        keyof_constraint,
                        RecursionFlags::TARGET,
                        report_errors,
                        IntersectionState::NONE,
                    )?) {
                        return Ok(Ternary::TRUE);
                    }
                }
                // 66151-66162 isGenericMappedType(targetType): mapped
                // types are unconstructible until M8, the arm is
                // vacuously false.
            }
        }
        if target_flags.intersects(TypeFlags::INDEXED_ACCESS) {
            // 66164-66207: indexed-access targets.
            let TypeData::IndexedAccess {
                object_type: target_object,
                index_type: target_index,
                ..
            } = self.st.tables.type_of(target).data
            else {
                unreachable!("indexed-access flag implies indexed-access data");
            };
            if source_flags.intersects(TypeFlags::INDEXED_ACCESS) {
                // 66165-66177: componentwise object/index relation;
                // the originalErrorInfo juggling is display machinery
                // — unported like the 66468-66471 precedent.
                let TypeData::IndexedAccess {
                    object_type: source_object,
                    index_type: source_index,
                    ..
                } = self.st.tables.type_of(source).data
                else {
                    unreachable!("indexed-access flag implies indexed-access data");
                };
                let mut result = self.is_related_to(
                    source_object,
                    target_object,
                    RecursionFlags::BOTH,
                    report_errors,
                    IntersectionState::NONE,
                )?;
                if !is_false(result) {
                    result = ternary_and(
                        result,
                        self.is_related_to(
                            source_index,
                            target_index,
                            RecursionFlags::BOTH,
                            report_errors,
                            IntersectionState::NONE,
                        )?,
                    );
                }
                if !is_false(result) {
                    return Ok(result);
                }
            }
            if self.relation == RelationKind::Assignable
                || self.relation == RelationKind::Comparable
            {
                // 66178-66203: S related to T[K] through the base-
                // constraint re-access of T[K] in the writing
                // direction.
                let base_object = self
                    .st
                    .get_base_constraint_of_type(target_object)?
                    .unwrap_or(target_object);
                let base_index = self
                    .st
                    .get_base_constraint_of_type(target_index)?
                    .unwrap_or(target_index);
                if !self.st.tables.is_generic_object_type(base_object)
                    && !self.st.tables.is_generic_index_type(base_index)
                {
                    let access_flags = AccessFlags::from_bits(
                        AccessFlags::WRITING.bits()
                            | if base_object != target_object {
                                AccessFlags::NO_INDEX_SIGNATURES.bits()
                            } else {
                                0
                            },
                    );
                    let constraint = self.st.get_indexed_access_type_or_undefined(
                        base_object,
                        base_index,
                        access_flags,
                        None,
                        None,
                        None,
                    )?;
                    if let Some(constraint) = constraint {
                        let result = self.is_related_to(
                            source,
                            constraint,
                            RecursionFlags::TARGET,
                            report_errors,
                            intersection_state,
                        )?;
                        if !is_false(result) {
                            return Ok(result);
                        }
                    }
                }
            }
        }
        if self.is_generic_mapped_type(target) && self.relation != RelationKind::Identity {
            return Err(Unsupported::new(
                "mapped-type targets (unported family, M8-stub)",
            ));
        }
        if target_flags.intersects(TypeFlags::CONDITIONAL) {
            return Err(Unsupported::new(
                "conditional targets (unported family, M8-stub)",
            ));
        }
        if target_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            if source_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                if self.relation == RelationKind::Comparable {
                    return Ok(
                        if self
                            .st
                            .template_literal_types_definitely_unrelated(source, target)
                        {
                            Ternary::FALSE
                        } else {
                            Ternary::TRUE
                        },
                    );
                }
                // 66279: template-vs-template outside comparable
                // reports unreliable variance through the marker
                // walk.
                let mapper = self.st.report_unreliable_mapper;
                self.st.instantiate_type(source, Some(mapper))?;
            }
            if self
                .st
                .is_type_matched_by_template_literal_type(source, target)?
            {
                return Ok(Ternary::TRUE);
            }
        } else if target_flags.intersects(TypeFlags::STRING_MAPPING) {
            // 66284-66290: non-mapping sources relate through
            // isMemberOfStringMapping; mapping-vs-mapping falls through
            // to the source arm below.
            if !source_flags.intersects(TypeFlags::STRING_MAPPING)
                && self.st.is_member_of_string_mapping(source, target)?
            {
                return Ok(Ternary::TRUE);
            }
        }
        if source_flags.intersects(TypeFlags::TYPE_VARIABLE) {
            // 66292: both-indexed-access pairs skip the constraint
            // chase (they compared componentwise above — the
            // indexed-access arms are the keyof follow-up).
            if !(source_flags.intersects(TypeFlags::INDEXED_ACCESS)
                && target_flags.intersects(TypeFlags::INDEXED_ACCESS))
            {
                let constraint = match self.st.get_constraint_of_type(source)? {
                    Some(constraint) => constraint,
                    None => self.st.tables.intrinsics.unknown,
                };
                let result = self.is_related_to(
                    constraint,
                    target,
                    RecursionFlags::SOURCE,
                    /*report_errors*/ false,
                    intersection_state,
                )?;
                if !is_false(result) {
                    return Ok(result);
                }
                // 66306-66313: retry with the source as this-argument;
                // the reportErrors expression is error machinery.
                let this_constraint = self.st.get_type_with_this_argument(
                    constraint,
                    Some(source),
                    /*need_apparent_type*/ false,
                )?;
                let result = self.is_related_to(
                    this_constraint,
                    target,
                    RecursionFlags::SOURCE,
                    /*report_errors*/ false,
                    intersection_state,
                )?;
                if !is_false(result) {
                    return Ok(result);
                }
                // isMappedTypeGenericIndexedAccess (66314): mapped
                // types are unconstructible until M8, the guard is
                // vacuously false.
            }
            // 66292-66443 is ONE else-if chain: a type-variable source
            // whose constraint chase failed exits it — the worker tail
            // returns FALSE, never the object block.
        } else if source_flags.intersects(TypeFlags::INDEX) {
            // 66325-66337: keyof sources relate through
            // string | number | symbol; the deferred-mapped-index
            // branch is vacuously dead (mapped types are
            // unconstructible until M8, so isDeferredMappedIndex is
            // constant false and reportErrors passes through).
            let string_number_symbol = self.st.tables.intrinsics.string_number_symbol;
            let result = self.is_related_to(
                string_number_symbol,
                target,
                RecursionFlags::SOURCE,
                report_errors,
                IntersectionState::NONE,
            )?;
            if !is_false(result) {
                return Ok(result);
            }
        } else if source_flags.intersects(TypeFlags::TEMPLATE_LITERAL)
            && !target_flags.intersects(TypeFlags::OBJECT)
        {
            if !target_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                // 66338-66344: relate through the template's base
                // constraint only when it differs from the template
                // itself (a self-constrained concrete template takes
                // no constraint step — relpin p414 pins the 2352).
                let constraint = self.st.get_base_constraint_of_type(source)?;
                if let Some(constraint) = constraint {
                    if constraint != source {
                        let related = self.is_related_to(
                            constraint,
                            target,
                            RecursionFlags::SOURCE,
                            report_errors,
                            IntersectionState::NONE,
                        )?;
                        if !is_false(related) {
                            return Ok(related);
                        }
                    }
                }
            }
        } else if source_flags.intersects(TypeFlags::STRING_MAPPING) {
            // 66345-66358.
            if target_flags.intersects(TypeFlags::STRING_MAPPING) {
                if self.st.tables.type_of(source).symbol != self.st.tables.type_of(target).symbol {
                    return Ok(Ternary::FALSE);
                }
                let TypeData::StringMapping { ty: source_inner } =
                    self.st.tables.type_of(source).data
                else {
                    unreachable!("string-mapping flag implies string-mapping data");
                };
                let TypeData::StringMapping { ty: target_inner } =
                    self.st.tables.type_of(target).data
                else {
                    unreachable!("string-mapping flag implies string-mapping data");
                };
                let related = self.is_related_to(
                    source_inner,
                    target_inner,
                    RecursionFlags::BOTH,
                    report_errors,
                    IntersectionState::NONE,
                )?;
                if is_true(related) {
                    return Ok(related);
                }
            } else {
                let constraint = self.st.get_base_constraint_of_type(source)?;
                if let Some(constraint) = constraint {
                    let related = self.is_related_to(
                        constraint,
                        target,
                        RecursionFlags::SOURCE,
                        report_errors,
                        IntersectionState::NONE,
                    )?;
                    if is_true(related) {
                        return Ok(related);
                    }
                }
            }
        } else if source_flags.intersects(TypeFlags::CONDITIONAL) {
            return Err(Unsupported::new(
                "conditional sources (unported family, M8-stub)",
            ));
        } else {
            // Stubbed guard: tsc gates this band behind
            // `!(source TEMPLATE_LITERAL && target OBJECT && <predicate>)`;
            // the predicate is unported (stub false), so the band always
            // runs. Partial mapped targets (66404-66406) are M4. Generic
            // mapped sources/targets handled above.
            let source_is_primitive = source_flags.intersects(TypeFlags::PRIMITIVE);
            if self.relation != RelationKind::Identity {
                source = self.st.get_apparent_type(source)?;
                source_flags = self.flags(source);
            }
            // 66420-66431: the same-target reference variance fast
            // path.
            let source_object_flags = self.st.tables.object_flags_of(source);
            let target_object_flags = self.st.tables.object_flags_of(target);
            if source_object_flags.intersects(ObjectFlags::REFERENCE)
                && target_object_flags.intersects(ObjectFlags::REFERENCE)
                && self.st.tables.reference_target(source)
                    == self.st.tables.reference_target(target)
                && !self.st.tables.is_tuple_type(source)
                && !(self.st.is_marker_type(source) || self.st.is_marker_type(target))
            {
                if self.st.is_empty_array_literal_type(source)? {
                    return Ok(Ternary::TRUE);
                }
                let reference_target = self.st.tables.reference_target(source);
                match self.st.get_variances(reference_target)? {
                    crate::variance::VariancesResult::InProgress => {
                        return Ok(Ternary::UNKNOWN);
                    }
                    crate::variance::VariancesResult::Known(variances) => {
                        let source_arguments = self.st.get_type_arguments(source)?;
                        let target_arguments = self.st.get_type_arguments(target)?;
                        if let Some(variance_result) = self.relate_variances(
                            &source_arguments,
                            &target_arguments,
                            &variances,
                            report_errors,
                            intersection_state,
                        )? {
                            return Ok(variance_result);
                        }
                    }
                }
            } else {
                // 66432: `isReadonlyArrayType(target) ? everyType(
                // source, isArrayOrTupleType) : isArrayType(target) &&
                // everyType(source, t => isTupleType(t) &&
                // !t.target.readonly)` — the global array targets
                // resolve once up front so the everyType closures stay
                // read-only.
                let global_array = self.st.global_array_type()?;
                let global_readonly = self.st.global_readonly_array_type()?;
                let is_array = |st: &crate::state::CheckerState, t: TypeId| {
                    st.tables
                        .object_flags_of(t)
                        .intersects(ObjectFlags::REFERENCE)
                        && {
                            let target = st.tables.reference_target(t);
                            target == global_array || target == global_readonly
                        }
                };
                let target_is_readonly_array = self
                    .st
                    .tables
                    .object_flags_of(target)
                    .intersects(ObjectFlags::REFERENCE)
                    && self.st.tables.reference_target(target) == global_readonly;
                let relates_through_number_index = if target_is_readonly_array {
                    self.st.every_type(source, |st, t| {
                        is_array(st, t) || st.tables.is_tuple_type(t)
                    })
                } else {
                    let target_is_array = self
                        .st
                        .tables
                        .object_flags_of(target)
                        .intersects(ObjectFlags::REFERENCE)
                        && self.st.tables.reference_target(target) == global_array;
                    target_is_array
                        && self.st.every_type(source, |st, t| {
                            st.tables.is_tuple_type(t) && {
                                let tuple_target = st.tables.reference_target(t);
                                match &st.tables.type_of(tuple_target).data {
                                    TypeData::TupleTarget(data) => !data.readonly,
                                    _ => false,
                                }
                            }
                        })
                };
                if relates_through_number_index {
                    // 66432-66438: (readonly) array targets relate through
                    // the number index types.
                    if self.relation != RelationKind::Identity {
                        let source_index = self
                            .st
                            .get_index_type_of_type(source, self.st.tables.intrinsics.number)?
                            .unwrap_or(self.st.tables.intrinsics.any);
                        let target_index = self
                            .st
                            .get_index_type_of_type(target, self.st.tables.intrinsics.number)?
                            .unwrap_or(self.st.tables.intrinsics.any);
                        return self.is_related_to(
                            source_index,
                            target_index,
                            RecursionFlags::BOTH,
                            report_errors,
                            IntersectionState::NONE,
                        );
                    } else {
                        return Ok(Ternary::FALSE);
                    }
                } else if self.st.is_generic_tuple_type(source)
                    && self.st.tables.is_tuple_type(target)
                    && !self.st.is_generic_tuple_type(target)
                {
                    // 66439-66443: generic tuple sources relate through
                    // their base constraint.
                    let constraint = self.st.get_base_constraint_or_type(source)?;
                    if constraint != source {
                        return self.is_related_to(
                            constraint,
                            target,
                            RecursionFlags::SOURCE,
                            report_errors,
                            IntersectionState::NONE,
                        );
                    }
                }
                // Subtype fresh-empty-target arm (66444-66446): the LAST
                // link of the 66420 else-if chain — an entered-but-fallen-
                // through reference or generic-tuple arm skips it, exactly
                // like tsc. Subtype activates in 4.8; the guard is ported
                // faithfully.
                else if (self.relation == RelationKind::Subtype
                    || self.relation == RelationKind::StrictSubtype)
                    && self
                        .st
                        .tables
                        .object_flags_of(target)
                        .intersects(ObjectFlags::FRESH_LITERAL)
                    && self.st.is_empty_object_type(target)?
                    && !self.st.is_empty_object_type(source)?
                {
                    return Ok(Ternary::FALSE);
                }
            }
            if source_flags.intersects(TypeFlags::from_bits(
                TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
            )) && target_flags.intersects(TypeFlags::OBJECT)
            {
                let mut result = self.properties_related_to(
                    source,
                    target,
                    /*excluded_properties*/ None,
                    /*optionals_only*/ false,
                    intersection_state,
                )?;
                if is_true(result) {
                    result = ternary_and(
                        result,
                        self.signatures_related_to(
                            source,
                            target,
                            SignatureKind::Call,
                            intersection_state,
                        )?,
                    );
                    if is_true(result) {
                        result = ternary_and(
                            result,
                            self.signatures_related_to(
                                source,
                                target,
                                SignatureKind::Construct,
                                intersection_state,
                            )?,
                        );
                        if is_true(result) {
                            result = ternary_and(
                                result,
                                self.index_signatures_related_to(
                                    source,
                                    target,
                                    source_is_primitive,
                                    intersection_state,
                                )?,
                            );
                        }
                    }
                }
                if is_true(result) {
                    return Ok(result);
                }
            }
            if source_flags.intersects(TypeFlags::from_bits(
                TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
            )) && target_flags.intersects(TypeFlags::UNION)
            {
                let object_only_target = self.st.tables.filter_type(target, |tables, t| {
                    tables.flags_of(t).intersects(TypeFlags::from_bits(
                        TypeFlags::OBJECT.bits()
                            | TypeFlags::INTERSECTION.bits()
                            | TypeFlags::SUBSTITUTION.bits(),
                    ))
                });
                if self.flags(object_only_target).intersects(TypeFlags::UNION) {
                    let result =
                        self.type_related_to_discriminated_type(source, object_only_target)?;
                    if is_true(result) {
                        return Ok(result);
                    }
                }
            }
        }
        Ok(Ternary::FALSE)
    }

    fn template_parts(&self, ty: TypeId) -> (Vec<TemplateText>, Vec<TypeId>) {
        match &self.st.tables.type_of(ty).data {
            TypeData::TemplateLiteral { texts, types } => (texts.to_vec(), types.to_vec()),
            _ => unreachable!("template flag implies template data"),
        }
    }

    pub(crate) fn is_generic_mapped_type(&self, ty: TypeId) -> bool {
        // Conservative until M8 can inspect mapped constraints/name
        // types: mapped relation arms must become live fail-closed as
        // soon as ObjectFlags::Mapped is constructible.
        self.st
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
    }

    /// tsc-port: typeRelatedToDiscriminatedType @6.0.3
    /// tsc-hash: 85fa9f5931952adc6f02d25a30c7b507db162b8d7b669b1689978f19e7bebfc5
    /// tsc-span: _tsc.js:66523-66626
    fn type_related_to_discriminated_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<Ternary> {
        let source_properties = self.st.get_properties_of_type(source)?;
        let Some(source_properties_filtered) = self
            .st
            .find_discriminant_properties(&source_properties, target)?
        else {
            return Ok(Ternary::FALSE);
        };
        let mut num_combinations = 1usize;
        for &source_property in &source_properties_filtered {
            let prop_type = self.st.get_non_missing_type_of_symbol(source_property)?;
            num_combinations = num_combinations.saturating_mul(self.count_types(prop_type));
            if num_combinations > 25 {
                return Ok(Ternary::FALSE);
            }
        }
        let mut source_discriminant_types: Vec<Vec<TypeId>> =
            Vec::with_capacity(source_properties_filtered.len());
        let mut excluded_properties: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for &source_property in &source_properties_filtered {
            let source_property_type = self.st.get_non_missing_type_of_symbol(source_property)?;
            source_discriminant_types.push(
                if self
                    .flags(source_property_type)
                    .intersects(TypeFlags::UNION)
                {
                    self.union_members(source_property_type)
                } else {
                    vec![source_property_type]
                },
            );
            excluded_properties.insert(self.st.binder.symbol(source_property).escaped_name.clone());
        }
        let discriminant_combinations = cartesian_product(&source_discriminant_types);
        let mut matching_types: Vec<TypeId> = Vec::new();
        let target_types = self.union_members(target);
        for combination in &discriminant_combinations {
            let mut has_match = false;
            'outer: for &ty in &target_types {
                for (i, &source_property) in source_properties_filtered.iter().enumerate() {
                    let name = self.st.binder.symbol(source_property).escaped_name.clone();
                    let Some(target_property) = self.st.get_property_of_type_full(ty, &name)?
                    else {
                        continue 'outer;
                    };
                    if source_property == target_property {
                        continue;
                    }
                    let combination_type = combination[i];
                    let skip_optional = self.st.tables.strict_null_checks
                        || self.relation == RelationKind::Comparable;
                    let related = self.property_related_to(
                        source,
                        target,
                        source_property,
                        target_property,
                        |_, _| Ok(combination_type),
                        IntersectionState::NONE,
                        skip_optional,
                    )?;
                    if !is_true(related) {
                        continue 'outer;
                    }
                }
                if !matching_types.contains(&ty) {
                    matching_types.push(ty);
                }
                has_match = true;
            }
            if !has_match {
                return Ok(Ternary::FALSE);
            }
        }
        let mut result = Ternary::TRUE;
        for ty in matching_types {
            let props = self.properties_related_to(
                source,
                ty,
                Some(&excluded_properties),
                /*optionals_only*/ false,
                IntersectionState::NONE,
            )?;
            result = ternary_and(result, props);
            if is_true(result) {
                result = ternary_and(
                    result,
                    self.signatures_related_to(
                        source,
                        ty,
                        SignatureKind::Call,
                        IntersectionState::NONE,
                    )?,
                );
                if is_true(result) {
                    result = ternary_and(
                        result,
                        self.signatures_related_to(
                            source,
                            ty,
                            SignatureKind::Construct,
                            IntersectionState::NONE,
                        )?,
                    );
                    if is_true(result)
                        && !(self.st.tables.is_tuple_type(source)
                            && self.st.tables.is_tuple_type(ty))
                    {
                        result = ternary_and(
                            result,
                            self.index_signatures_related_to(
                                source,
                                ty,
                                /*source_is_primitive*/ false,
                                IntersectionState::NONE,
                            )?,
                        );
                    }
                }
            }
            if !is_true(result) {
                return Ok(result);
            }
        }
        Ok(result)
    }

    fn count_types(&self, ty: TypeId) -> usize {
        if self.flags(ty).intersects(TypeFlags::UNION) {
            self.union_members(ty).len()
        } else {
            1
        }
    }

    /// tsc-port: discriminateTypeByDiscriminableItems @6.0.3
    /// tsc-hash: 1290115be563c6a5dbeaf88f7facf3ed9a8fed47250275f155ab82f834058cbd
    /// tsc-span: _tsc.js:67259-67284
    ///
    /// M3 slice: the discriminators are the filtered source properties
    /// (findMatchingDiscriminantType builds `[() => getTypeOfSymbol(p),
    /// p.escapedName]` pairs, 90526-90528); M6's contextual-typing
    /// callers pass other thunks and generalize the parameter then.
    pub(crate) fn discriminate_type_by_discriminable_items(
        &mut self,
        target: TypeId,
        discriminators: &[SymbolId],
    ) -> CheckResult2<TypeId> {
        let types = self.union_members(target);
        let mut include: Vec<Ternary> = Vec::with_capacity(types.len());
        for &t in &types {
            let excluded = self.flags(t).intersects(TypeFlags::PRIMITIVE) || {
                let reduced = self.st.get_reduced_type(t)?;
                self.flags(reduced).intersects(TypeFlags::NEVER)
            };
            include.push(if excluded {
                Ternary::FALSE
            } else {
                Ternary::TRUE
            });
        }
        for &prop in discriminators {
            let property_name = self.st.binder.symbol(prop).escaped_name.clone();
            let discriminating_type = self.st.get_type_of_symbol(prop)?;
            let mut matched = false;
            for i in 0..types.len() {
                if is_true(include[i]) {
                    if let Some(target_type) = self
                        .st
                        .get_type_of_property_or_index_signature_of_type(types[i], &property_name)?
                    {
                        if self.some_type_related_for_discrimination(
                            discriminating_type,
                            target_type,
                        )? {
                            matched = true;
                        } else {
                            include[i] = Ternary::MAYBE;
                        }
                    }
                }
            }
            for slot in include.iter_mut() {
                if *slot == Ternary::MAYBE {
                    *slot = if matched {
                        Ternary::FALSE
                    } else {
                        Ternary::TRUE
                    };
                }
            }
        }
        let filtered = if include.contains(&Ternary::FALSE) {
            let kept: Vec<TypeId> = types
                .iter()
                .zip(include.iter())
                .filter(|&(_, &inc)| is_true(inc))
                .map(|(&t, _)| t)
                .collect();
            self.st.get_union_type_ex(&kept, UnionReduction::None)?
        } else {
            target
        };
        Ok(if self.flags(filtered).intersects(TypeFlags::NEVER) {
            target
        } else {
            filtered
        })
    }

    /// The `someType(getDiscriminatingType(), t => !!related(t,
    /// targetType))` slice of 67268 — someType's general port is M5
    /// 6.1; `related` is the closure's isRelatedTo with its tsc
    /// default arguments.
    fn some_type_related_for_discrimination(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        if self.flags(source).intersects(TypeFlags::UNION) {
            for t in self.union_members(source) {
                let related = self.is_related_to(
                    t,
                    target,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if is_true(related) {
                    return Ok(true);
                }
            }
            Ok(false)
        } else {
            let related = self.is_related_to(
                source,
                target,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                IntersectionState::NONE,
            )?;
            Ok(is_true(related))
        }
    }

    /// tsc-port: isPropertySymbolTypeRelated @6.0.3
    /// tsc-hash: 7fd07b9a0e96f5465c47515c67349cd76363821b9d0633c65248662ae558d4b5
    /// tsc-span: _tsc.js:66641-66662
    fn is_property_symbol_type_related(
        &mut self,
        source_prop: SymbolId,
        target_prop: SymbolId,
        get_type_of_source_property: impl Fn(&mut Self, SymbolId) -> CheckResult2<TypeId>,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let target_is_optional = self.st.tables.strict_null_checks
            && self
                .st
                .get_check_flags(target_prop)
                .intersects(CheckFlags::PARTIAL);
        let non_missing = self.st.get_non_missing_type_of_symbol(target_prop)?;
        let effective_target = self.st.tables.add_optionality(
            non_missing,
            /*is_property*/ false,
            target_is_optional,
        );
        let any_mask = if self.relation == RelationKind::StrictSubtype {
            TypeFlags::ANY
        } else {
            TypeFlags::ANY_OR_UNKNOWN
        };
        if self.flags(effective_target).intersects(any_mask) {
            return Ok(Ternary::TRUE);
        }
        let effective_source = get_type_of_source_property(self, source_prop)?;
        self.is_related_to(
            effective_source,
            effective_target,
            RecursionFlags::BOTH,
            /*report_errors*/ false,
            intersection_state,
        )
    }

    /// tsc-port: propertyRelatedTo @6.0.3
    /// tsc-hash: 19d5c6a584328bf185f97857552fb6e2d018cee9a73a975a40b8decf71d676da
    /// tsc-span: _tsc.js:66663-66707
    ///
    /// Private/protected arms key on class members (M4); M3 members
    /// carry no accessibility modifiers, so those flags are zero for
    /// declared symbols and read from Contains* for synthetics.
    #[allow(clippy::too_many_arguments)]
    fn property_related_to(
        &mut self,
        _source: TypeId,
        _target: TypeId,
        source_prop: SymbolId,
        target_prop: SymbolId,
        get_type_of_source_property: impl Fn(&mut Self, SymbolId) -> CheckResult2<TypeId>,
        intersection_state: IntersectionState,
        skip_optional: bool,
    ) -> CheckResult2<Ternary> {
        let source_prop_flags = self
            .st
            .get_declaration_modifier_flags_from_symbol(source_prop);
        let target_prop_flags = self
            .st
            .get_declaration_modifier_flags_from_symbol(target_prop);
        if source_prop_flags.intersects(ModifierFlags::PRIVATE)
            || target_prop_flags.intersects(ModifierFlags::PRIVATE)
        {
            let source_declaration = self.st.binder.symbol(source_prop).value_declaration;
            let target_declaration = self.st.binder.symbol(target_prop).value_declaration;
            if source_declaration != target_declaration {
                return Ok(Ternary::FALSE);
            }
        } else if target_prop_flags.intersects(ModifierFlags::PROTECTED) {
            if !self.st.is_valid_override_of(source_prop, target_prop)? {
                return Ok(Ternary::FALSE);
            }
        } else if source_prop_flags.intersects(ModifierFlags::PROTECTED) {
            // 66686-66692: protected source vs public target.
            return Ok(Ternary::FALSE);
        }
        if self.relation == RelationKind::StrictSubtype
            && self.st.is_readonly_symbol(source_prop)
            && !self.st.is_readonly_symbol(target_prop)
        {
            return Ok(Ternary::FALSE);
        }
        let related = self.is_property_symbol_type_related(
            source_prop,
            target_prop,
            get_type_of_source_property,
            intersection_state,
        )?;
        if !is_true(related) {
            return Ok(Ternary::FALSE);
        }
        let source_optional = self
            .st
            .symbol_flags(source_prop)
            .intersects(SymbolFlags::OPTIONAL);
        let target_class_member = self
            .st
            .symbol_flags(target_prop)
            .intersects(SymbolFlags::CLASS_MEMBER);
        let target_optional = self
            .st
            .symbol_flags(target_prop)
            .intersects(SymbolFlags::OPTIONAL);
        if !skip_optional && source_optional && target_class_member && !target_optional {
            return Ok(Ternary::FALSE);
        }
        Ok(related)
    }

    /// tsc-port: excludeProperties @6.0.3
    /// tsc-hash: 0b506adbd35ab7f6f207cb5a37868efc66b905adb9971f319f1299a3f74ef4f3
    /// tsc-span: _tsc.js:66627-66640
    fn exclude_properties(
        &self,
        properties: Vec<SymbolId>,
        excluded_properties: Option<&std::collections::HashSet<String>>,
    ) -> Vec<SymbolId> {
        let Some(excluded) = excluded_properties else {
            return properties;
        };
        properties
            .into_iter()
            .filter(|&p| !excluded.contains(&self.st.binder.symbol(p).escaped_name))
            .collect()
    }

    /// tsc-port: propertiesRelatedTo @6.0.3
    /// tsc-hash: 7ab473d979727978a6b21387e28e6ee505aa87b65b6b3e3ecba42fcc6eacb70a
    /// tsc-span: _tsc.js:66766-66910
    ///
    /// Includes the TUPLE target arm (66771+). The variadic-vs-rest
    /// createArrayType branch and generic-variadic flags need M4
    /// (generic tuples/global Array); M3 tuples carry no Variadic
    /// elements after normalization, so those branches report
    /// Unsupported if ever reached.
    #[allow(clippy::needless_range_loop)] // positional dual-array walk, ported as tsc wrote it
    pub(crate) fn properties_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        excluded_properties: Option<&std::collections::HashSet<String>>,
        optionals_only: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if self.relation == RelationKind::Identity {
            return self.properties_identical_to(source, target, excluded_properties);
        }
        let mut result = Ternary::TRUE;
        if self.st.tables.is_tuple_type(target) {
            let source_is_tuple = self.st.tables.is_tuple_type(source);
            // isArrayOrTupleType(source) (66772): array sources walk
            // the same element loop with arity 1, a Rest element and
            // minLength 0.
            if source_is_tuple || self.st.is_array_type(source)? {
                let source_data = if source_is_tuple {
                    let source_target = self.st.tables.reference_target(source);
                    let TypeData::TupleTarget(data) =
                        self.st.tables.type_of(source_target).data.clone()
                    else {
                        unreachable!("tuple type targets a tuple target");
                    };
                    Some(data)
                } else {
                    None
                };
                let target_target = self.st.tables.reference_target(target);
                let TypeData::TupleTarget(target_data) =
                    self.st.tables.type_of(target_target).data.clone()
                else {
                    unreachable!("tuple type targets a tuple target");
                };
                let source_readonly = match &source_data {
                    Some(data) => data.readonly,
                    None => self.st.is_readonly_array_type(source)?,
                };
                if !target_data.readonly && source_readonly {
                    return Ok(Ternary::FALSE);
                }
                let source_arity = match &source_data {
                    Some(data) => data.type_parameters.len(),
                    None => 1,
                };
                let target_arity = target_data.type_parameters.len();
                let source_rest_flag = match &source_data {
                    Some(data) => data.combined_flags.intersects(ElementFlags::REST),
                    None => true,
                };
                let target_has_rest_element = target_data
                    .combined_flags
                    .intersects(ElementFlags::VARIABLE);
                let source_min_length = source_data.as_ref().map_or(0, |data| data.min_length);
                let target_min_length = target_data.min_length;
                if !source_rest_flag && source_arity < target_min_length {
                    return Ok(Ternary::FALSE);
                }
                if !target_has_rest_element && target_arity < source_min_length {
                    return Ok(Ternary::FALSE);
                }
                if !target_has_rest_element && (source_rest_flag || target_arity < source_arity) {
                    return Ok(Ternary::FALSE);
                }
                // getTypeArguments (66804-66805): deferred tuple
                // references force their arguments lazily here.
                let source_type_arguments = self.st.get_type_arguments(source)?;
                let target_type_arguments = self.st.get_type_arguments(target)?;
                let target_start_count =
                    start_element_count(&target_data.element_flags, ElementFlags::NON_REST);
                let target_end_count =
                    end_element_count(&target_data.element_flags, ElementFlags::NON_REST);
                let mut can_exclude_discriminants = excluded_properties.is_some();
                for source_position in 0..source_arity {
                    // 66809: array sources read as Rest at every
                    // position.
                    let source_flags = match &source_data {
                        Some(data) => data.element_flags[source_position],
                        None => ElementFlags::REST,
                    };
                    let source_position_from_end = source_arity - 1 - source_position;
                    let target_position =
                        if target_has_rest_element && source_position >= target_start_count {
                            target_arity - 1 - source_position_from_end.min(target_end_count)
                        } else {
                            source_position
                        };
                    let target_flags = target_data.element_flags[target_position];
                    if target_flags.intersects(ElementFlags::VARIADIC)
                        && !source_flags.intersects(ElementFlags::VARIADIC)
                    {
                        return Ok(Ternary::FALSE);
                    }
                    if source_flags.intersects(ElementFlags::VARIADIC)
                        && !target_flags.intersects(ElementFlags::VARIABLE)
                    {
                        return Ok(Ternary::FALSE);
                    }
                    if target_flags.intersects(ElementFlags::REQUIRED)
                        && !source_flags.intersects(ElementFlags::REQUIRED)
                    {
                        return Ok(Ternary::FALSE);
                    }
                    if can_exclude_discriminants {
                        if source_flags.intersects(ElementFlags::VARIABLE)
                            || target_flags.intersects(ElementFlags::VARIABLE)
                        {
                            can_exclude_discriminants = false;
                        }
                        if can_exclude_discriminants
                            && excluded_properties
                                .is_some_and(|e| e.contains(&source_position.to_string()))
                        {
                            continue;
                        }
                    }
                    let source_type = self.st.remove_missing_type(
                        source_type_arguments[source_position],
                        source_flags.intersects(ElementFlags::OPTIONAL)
                            && target_flags.intersects(ElementFlags::OPTIONAL),
                    );
                    let target_type = target_type_arguments[target_position];
                    // 66841: a variadic source element against a rest
                    // target element compares to the rest ARRAY.
                    let target_check_type = if source_flags.intersects(ElementFlags::VARIADIC)
                        && target_flags.intersects(ElementFlags::REST)
                    {
                        self.st.create_array_type(target_type, /*readonly*/ false)?
                    } else {
                        self.st.remove_missing_type(
                            target_type,
                            target_flags.intersects(ElementFlags::OPTIONAL),
                        )
                    };
                    let related = self.is_related_to(
                        source_type,
                        target_check_type,
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        intersection_state,
                    )?;
                    if !is_true(related) {
                        return Ok(Ternary::FALSE);
                    }
                    result = ternary_and(result, related);
                }
                return Ok(result);
            }
            // tsc 66849-66851 `else if`: non-array/tuple sources
            // (references included — the M3-era escape here was stale)
            // fail fast against variable-arity targets, else fall
            // through to the property machinery over the synthesized
            // tuple index members.
            let target_target = self.st.tables.reference_target(target);
            let TypeData::TupleTarget(target_data) =
                self.st.tables.type_of(target_target).data.clone()
            else {
                unreachable!("tuple type targets a tuple target");
            };
            if target_data
                .combined_flags
                .intersects(ElementFlags::VARIABLE)
            {
                return Ok(Ternary::FALSE);
            }
        }
        let require_optional_properties = (self.relation == RelationKind::Subtype
            || self.relation == RelationKind::StrictSubtype)
            && !self.st.is_object_literal_type(source)
            && !self.st.tables.is_tuple_type(source);
        if let Some(_unmatched) =
            self.get_unmatched_property(source, target, require_optional_properties)?
        {
            return Ok(Ternary::FALSE);
        }
        if self.st.is_object_literal_type(target) {
            let source_props = self.st.get_properties_of_type(source)?;
            for source_prop in self.exclude_properties(source_props, excluded_properties) {
                let name = self.st.binder.symbol(source_prop).escaped_name.clone();
                if self
                    .st
                    .get_property_of_object_type(target, &name)?
                    .is_none()
                {
                    return Ok(Ternary::FALSE);
                }
            }
        }
        let properties = self.st.get_properties_of_type(target)?;
        let numeric_names_only =
            self.st.tables.is_tuple_type(source) && self.st.tables.is_tuple_type(target);
        for target_prop in self.exclude_properties(properties, excluded_properties) {
            let name = self.st.binder.symbol(target_prop).escaped_name.clone();
            let target_symbol_flags = self.st.symbol_flags(target_prop);
            if !target_symbol_flags.intersects(SymbolFlags::PROTOTYPE)
                && (!numeric_names_only || is_numeric_name(&name) || name == "length")
                && (!optionals_only || target_symbol_flags.intersects(SymbolFlags::OPTIONAL))
            {
                let source_prop = self.st.get_property_of_type_full(source, &name)?;
                if let Some(source_prop) = source_prop {
                    if source_prop != target_prop {
                        let skip_optional = self.relation == RelationKind::Comparable;
                        let related = self.property_related_to(
                            source,
                            target,
                            source_prop,
                            target_prop,
                            |checker, prop| checker.st.get_non_missing_type_of_symbol(prop),
                            intersection_state,
                            skip_optional,
                        )?;
                        if !is_true(related) {
                            return Ok(Ternary::FALSE);
                        }
                        result = ternary_and(result, related);
                    }
                }
            }
        }
        Ok(result)
    }

    /// tsc-port: getUnmatchedProperties @6.0.3
    /// tsc-hash: efa5f0c2bd4b56a53bdd6d612446a57e68108c2f7a11fce54777c60e7c37e908
    /// tsc-span: _tsc.js:68461-68482
    ///
    /// tsrs-native: the relation path always passes
    /// matchDiscriminantProperties=false — body on CheckerState since
    /// 7.2d (typesDefinitelyUnrelated needs the discriminant arm).
    fn get_unmatched_property(
        &mut self,
        source: TypeId,
        target: TypeId,
        require_optional_properties: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        self.st
            .get_unmatched_property(source, target, require_optional_properties, false)
    }

    /// tsc-port: propertiesIdenticalTo @6.0.3
    /// tsc-hash: f07d39e00b556f065fa869aefc6adce7135168be9f62c8a51afbe7b48351e137
    /// tsc-span: _tsc.js:66911-66933
    fn properties_identical_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        excluded_properties: Option<&std::collections::HashSet<String>>,
    ) -> CheckResult2<Ternary> {
        if !(self.flags(source).intersects(TypeFlags::OBJECT)
            && self.flags(target).intersects(TypeFlags::OBJECT))
        {
            return Ok(Ternary::FALSE);
        }
        let source_props = self.st.get_properties_of_object_type_owned(source)?;
        let source_properties = self.exclude_properties(source_props, excluded_properties);
        let target_props = self.st.get_properties_of_object_type_owned(target)?;
        let target_properties = self.exclude_properties(target_props, excluded_properties);
        if source_properties.len() != target_properties.len() {
            return Ok(Ternary::FALSE);
        }
        let mut result = Ternary::TRUE;
        for source_prop in source_properties {
            let name = self.st.binder.symbol(source_prop).escaped_name.clone();
            let Some(target_prop) = self.st.get_property_of_object_type(target, &name)? else {
                return Ok(Ternary::FALSE);
            };
            let related = self.compare_properties(source_prop, target_prop)?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: compareProperties @6.0.3
    /// tsc-hash: 42f04303574ccb64448bdeda07e716852dbca284333cf3f172159a566c991bb9
    /// tsc-span: _tsc.js:67536-67558
    fn compare_properties(
        &mut self,
        source_prop: SymbolId,
        target_prop: SymbolId,
    ) -> CheckResult2<Ternary> {
        if source_prop == target_prop {
            return Ok(Ternary::TRUE);
        }
        let source_accessibility = self
            .st
            .get_declaration_modifier_flags_from_symbol(source_prop)
            .bits()
            & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits();
        let target_accessibility = self
            .st
            .get_declaration_modifier_flags_from_symbol(target_prop)
            .bits()
            & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits();
        if source_accessibility != target_accessibility {
            return Ok(Ternary::FALSE);
        }
        if source_accessibility != 0 {
            // 67546: nominal identity through getTargetSymbol.
            if self.st.get_target_symbol(source_prop) != self.st.get_target_symbol(target_prop) {
                return Ok(Ternary::FALSE);
            }
        }
        if self
            .st
            .symbol_flags(source_prop)
            .intersects(SymbolFlags::OPTIONAL)
            != self
                .st
                .symbol_flags(target_prop)
                .intersects(SymbolFlags::OPTIONAL)
        {
            return Ok(Ternary::FALSE);
        }
        if self.st.is_readonly_symbol(source_prop) != self.st.is_readonly_symbol(target_prop) {
            return Ok(Ternary::FALSE);
        }
        let source_type = self.st.get_non_missing_type_of_symbol(source_prop)?;
        let target_type = self.st.get_non_missing_type_of_symbol(target_prop)?;
        self.is_related_to(
            source_type,
            target_type,
            RecursionFlags::BOTH,
            /*report_errors*/ false,
            IntersectionState::NONE,
        )
    }

    /// tsc-port: signaturesRelatedTo @6.0.3
    /// tsc-hash: dd60f4d6921e768de3c170755c2db13ad88525f85d0536f59177f8e469c0debb
    /// tsc-span: _tsc.js:66934-67042
    ///
    /// KNOWN-GAP (checkJs band): the JS-constructor Construct→Call
    /// kind swap (66945-66950, isJSConstructor requires isInJSFile)
    /// is unported — dead for TS inputs, must land with the checkJs
    /// band (M7/M8). The anyFunctionType wildcard arms and the
    /// instantiated/same-reference pairwise arm are LIVE (M4 stubs
    /// lapsed at 5.7 / 7.5b respectively).
    fn signatures_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        kind: SignatureKind,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if self.relation == RelationKind::Identity {
            return self.signatures_identical_to(source, target, kind);
        }
        // 66939-66944: the anyFunctionType wildcard arms — a
        // SkipContextSensitive function-expression stand-in (M4 5.7
        // argument selection is the first producer) relates to every
        // signature list; nothing relates TO it.
        if source == self.st.any_function_type {
            return Ok(Ternary::TRUE);
        }
        if target == self.st.any_function_type {
            return Ok(Ternary::FALSE);
        }
        let source_signatures = self.st.get_signatures_of_type(source, kind)?;
        let target_signatures = self.st.get_signatures_of_type(target, kind)?;
        if kind == SignatureKind::Construct
            && !source_signatures.is_empty()
            && !target_signatures.is_empty()
        {
            let source_is_abstract = self
                .st
                .signature_of(source_signatures[0])
                .flags
                .intersects(tsrs2_types::SignatureFlags::ABSTRACT);
            let target_is_abstract = self
                .st
                .signature_of(target_signatures[0])
                .flags
                .intersects(tsrs2_types::SignatureFlags::ABSTRACT);
            if source_is_abstract && !target_is_abstract {
                return Ok(Ternary::FALSE);
            }
            // KNOWN-GAP since M4 (m4-review B4):
            // constructorVisibilitiesAreCompatible is skipped —
            // private/protected constructors are constructible class
            // members since M4 (the "always public" claim is false),
            // so a private-ctor class assigned to a construct
            // signature misses its 2322 (probed).
        }
        let mut result = Ternary::TRUE;
        let source_object_flags = self.st.tables.object_flags_of(source);
        let target_object_flags = self.st.tables.object_flags_of(target);
        // 66952-66966 (m4-review B8): instantiations of one symbol —
        // or references to one target — compare their signature lists
        // PAIRWISE (index i to index i), never N×M; tsc asserts the
        // lists line up. tsc's `source.symbol === target.symbol`
        // treats two symbol-less instantiated types as a pair
        // (undefined === undefined) — None == None mirrors that.
        if source_object_flags.intersects(ObjectFlags::INSTANTIATED)
            && target_object_flags.intersects(ObjectFlags::INSTANTIATED)
            && self.st.tables.type_of(source).symbol == self.st.tables.type_of(target).symbol
            || source_object_flags.intersects(ObjectFlags::REFERENCE)
                && target_object_flags.intersects(ObjectFlags::REFERENCE)
                && self.st.tables.reference_target(source)
                    == self.st.tables.reference_target(target)
        {
            // Hard assert (7.5d review): tsc's Debug.assertEqual
            // throws in the SHIPPED compiler too — a debug_assert
            // would let a release build silently prefix-compare when
            // source is longer.
            assert_eq!(
                source_signatures.len(),
                target_signatures.len(),
                "same-target signature lists line up (tsc Debug.assertEqual 66957)"
            );
            for i in 0..target_signatures.len() {
                let related = self.signature_related_to(
                    source_signatures[i],
                    target_signatures[i],
                    /*erase*/ true,
                    intersection_state,
                )?;
                if !is_true(related) {
                    return Ok(Ternary::FALSE);
                }
                result = ternary_and(result, related);
            }
        } else if source_signatures.len() == 1 && target_signatures.len() == 1 {
            let erase_generics = self.relation == RelationKind::Comparable;
            let related = self.signature_related_to(
                source_signatures[0],
                target_signatures[0],
                erase_generics,
                intersection_state,
            )?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = related;
        } else {
            'outer: for &t in &target_signatures {
                for &s in &source_signatures {
                    let related =
                        self.signature_related_to(s, t, /*erase*/ true, intersection_state)?;
                    if is_true(related) {
                        result = ternary_and(result, related);
                        continue 'outer;
                    }
                }
                return Ok(Ternary::FALSE);
            }
        }
        Ok(result)
    }

    /// tsc-port: signatureRelatedTo @6.0.3
    /// tsc-hash: 077b917f6c7e74357aafbb0c7e5f24c8de49ad1498046f4c86951905aacae66f
    /// tsc-span: _tsc.js:67067-67081
    ///
    /// erase applies getErasedSignature to each side (67069-67071;
    /// the M4-era gap is closed — m4-review B8). compareTypes = the
    /// isRelatedToWorker closure over THIS frame with the captured
    /// intersectionState (the RelationFrame variant).
    fn signature_related_to(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        erase: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let source = if erase {
            self.st.get_erased_signature(source)?
        } else {
            source
        };
        let target = if erase {
            self.st.get_erased_signature(target)?
        } else {
            target
        };
        let check_mode = match self.relation {
            RelationKind::Subtype => check_mode::STRICT_TOP_SIGNATURE,
            RelationKind::StrictSubtype => {
                check_mode::STRICT_TOP_SIGNATURE | check_mode::STRICT_ARITY
            }
            _ => check_mode::NONE,
        };
        // 67069: signatureRelatedTo passes reportUnreliableMapper.
        let report_unreliable_markers = Some(self.st.report_unreliable_mapper);
        self.compare_signatures_related(
            source,
            target,
            check_mode,
            intersection_state,
            report_unreliable_markers,
            crate::inference::CompareTypesFn::RelationFrame,
        )
    }

    /// tsc-port: signaturesIdenticalTo @6.0.3
    /// tsc-hash: 8483eef69aa1e121bb7fc48638dab0cc409cab18a79e9acbca9f8a2698be49d5
    /// tsc-span: _tsc.js:67082-67107
    fn signatures_identical_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        kind: SignatureKind,
    ) -> CheckResult2<Ternary> {
        let source_signatures = self.st.get_signatures_of_type(source, kind)?;
        let target_signatures = self.st.get_signatures_of_type(target, kind)?;
        if source_signatures.len() != target_signatures.len() {
            return Ok(Ternary::FALSE);
        }
        let mut result = Ternary::TRUE;
        for i in 0..source_signatures.len() {
            let related =
                self.compare_signatures_identical(source_signatures[i], target_signatures[i])?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: isMatchingSignature @6.0.3
    /// tsc-hash: 964a6e7fc177bdf2129511936257ac777ad3265d51e6fb91770ff9e7b9f46486
    /// tsc-span: _tsc.js:67559-67573
    ///
    /// partialMatch accepts arity-compatible sources (58057-path).
    fn is_matching_signature(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        partial_match: bool,
    ) -> CheckResult2<bool> {
        let source_parameter_count = self.st.get_parameter_count(source)?;
        let target_parameter_count = self.st.get_parameter_count(target)?;
        let source_min = self.st.get_min_argument_count(source)?;
        let target_min = self.st.get_min_argument_count(target)?;
        let source_rest = self.st.has_effective_rest_parameter(source)?;
        let target_rest = self.st.has_effective_rest_parameter(target)?;
        if source_parameter_count == target_parameter_count
            && source_min == target_min
            && source_rest == target_rest
        {
            return Ok(true);
        }
        // 67570-67571: a partial match accepts a source that requires
        // no more than the target.
        Ok(partial_match && source_min <= target_min)
    }

    /// tsc-port: compareSignaturesIdentical @6.0.3
    /// tsc-hash: ff64ccff2dd2fde3efc5b70fe05834b924d9044f53833479bf00443877912805
    /// tsc-span: _tsc.js:67574-67630
    ///
    /// Type parameters and this-types are M4 rows; type predicates
    /// report Unsupported via getTypePredicateOfSignature.
    fn compare_signatures_identical(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> CheckResult2<Ternary> {
        self.compare_signatures_identical_ex(source, target, false, false, false)
    }

    /// The parametrized face (partialMatch / ignoreThisTypes /
    /// ignoreReturnTypes) used by the union-signature machinery; the
    /// ambient relation supplies the compareTypes callback.
    pub(crate) fn compare_signatures_identical_ex(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        partial_match: bool,
        ignore_this_types: bool,
        ignore_return_types: bool,
    ) -> CheckResult2<Ternary> {
        let mut source = source;
        if source == target {
            return Ok(Ternary::TRUE);
        }
        if !self.is_matching_signature(source, target, partial_match)? {
            return Ok(Ternary::FALSE);
        }
        // 67581-67595: pairwise type-parameter identity — constraints
        // and defaults compare through the source→target mapper.
        let source_type_parameters = self
            .st
            .signature_of(source)
            .type_parameters
            .clone()
            .unwrap_or_default();
        let target_type_parameters = self
            .st
            .signature_of(target)
            .type_parameters
            .clone()
            .unwrap_or_default();
        if source_type_parameters.len() != target_type_parameters.len() {
            return Ok(Ternary::FALSE);
        }
        if !target_type_parameters.is_empty() {
            let mapper = self.st.create_type_mapper(
                source_type_parameters.clone(),
                Some(target_type_parameters.clone()),
            );
            for (i, &t) in target_type_parameters.iter().enumerate() {
                let s = source_type_parameters[i];
                if s == t {
                    continue;
                }
                let unknown = self.st.tables.intrinsics.unknown;
                let source_constraint = self.st.get_constraint_from_type_parameter(s)?;
                let source_constraint = match source_constraint {
                    Some(constraint) => self.st.instantiate_type(constraint, Some(mapper))?,
                    None => unknown,
                };
                let target_constraint = self
                    .st
                    .get_constraint_from_type_parameter(t)?
                    .unwrap_or(unknown);
                let related = self.is_related_to(
                    source_constraint,
                    target_constraint,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if !is_true(related) {
                    return Ok(Ternary::FALSE);
                }
                let source_default = self.st.get_default_from_type_parameter(s)?;
                let source_default = match source_default {
                    Some(default) => self.st.instantiate_type(default, Some(mapper))?,
                    None => unknown,
                };
                let target_default = self
                    .st
                    .get_default_from_type_parameter(t)?
                    .unwrap_or(unknown);
                let related = self.is_related_to(
                    source_default,
                    target_default,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if !is_true(related) {
                    return Ok(Ternary::FALSE);
                }
            }
            // 67593-67598: the remaining comparison runs on the source
            // instantiated into the target's parameter space.
            source = self
                .st
                .instantiate_signature(source, mapper, /*erase_type_parameters*/ true)?;
        }
        let mut result = Ternary::TRUE;
        let source_this = if ignore_this_types {
            None
        } else {
            self.st.get_this_type_of_signature(source)?
        };
        if let Some(source_this) = source_this {
            if let Some(target_this) = self.st.get_this_type_of_signature(target)? {
                let related = self.is_related_to(
                    source_this,
                    target_this,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
                )?;
                if !is_true(related) {
                    return Ok(Ternary::FALSE);
                }
                result = ternary_and(result, related);
            }
        }
        let target_len = self.st.get_parameter_count(target)?;
        for i in 0..target_len {
            let s = self.st.get_type_at_position(source, i)?;
            let t = self.st.get_type_at_position(target, i)?;
            let related = self.is_related_to(
                t,
                s,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                IntersectionState::NONE,
            )?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        if ignore_return_types {
            return Ok(result);
        }
        // 67624-67628: the predicate consult REPLACES the return-type
        // comparison when either side carries one, and only inside
        // !ignoreReturnTypes — the ignoreReturnTypes cells (union
        // signature matching via findMatchingSignature) consult no
        // predicate machinery at all (m4-review B7 restored the
        // decision table; the old gate over-contained them).
        // KNOWN-GAP (7.5d review): this tail deliberately keeps the
        // RAW consult — the body-inferred-candidate guard the
        // related arm and callback cell ride
        // (relation_type_predicate_of_signature) would Err here on
        // every union/intersection signature-list assembly over
        // unannotated boolean members, killing their calls' REAL
        // rows (tsc resolves same-refinement members fine). Residual:
        // a ONE-sided body-inference in tsc (pred vs None → False)
        // can over-match here (both None → plain return compare) —
        // list-shape divergence only, no proven fabrication; rewrite
        // with getTypePredicateFromBody (the M6 escape's owner).
        let source_type_predicate = self.st.get_type_predicate_of_signature(source)?;
        let target_type_predicate = self.st.get_type_predicate_of_signature(target)?;
        let related = if source_type_predicate.is_some() || target_type_predicate.is_some() {
            self.compare_type_predicates_identical(
                source_type_predicate.as_ref(),
                target_type_predicate.as_ref(),
            )?
        } else {
            let source_return = self.st.get_return_type_of_signature(source)?;
            let target_return = self.st.get_return_type_of_signature(target)?;
            self.is_related_to(
                source_return,
                target_return,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                IntersectionState::NONE,
            )?
        };
        Ok(ternary_and(result, related))
    }

    /// tsc-port: compareTypePredicatesIdentical @6.0.3
    /// tsc-hash: 5315184a82be50f8baa530ea5ef8f83a9be9e5a183949ebaaa2afb97ee192428
    /// tsc-span: _tsc.js:67631-67633
    ///
    /// typePredicateKindsMatch (61610-61612) inlined: kind AND
    /// parameterIndex equality. `source.type === target.type` covers
    /// the both-None cell (asserts-form pairs); a one-sided type is
    /// False. compareTypes = the ambient relation's worker, exactly
    /// like the sibling identical-path calls.
    fn compare_type_predicates_identical(
        &mut self,
        source: Option<&crate::narrow::TypePredicate>,
        target: Option<&crate::narrow::TypePredicate>,
    ) -> CheckResult2<Ternary> {
        let (Some(source), Some(target)) = (source, target) else {
            return Ok(Ternary::FALSE);
        };
        if source.kind != target.kind || source.parameter_index != target.parameter_index {
            return Ok(Ternary::FALSE);
        }
        if source.ty == target.ty {
            return Ok(Ternary::TRUE);
        }
        if let (Some(source_type), Some(target_type)) = (source.ty, target.ty) {
            return self.is_related_to(
                source_type,
                target_type,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                IntersectionState::NONE,
            );
        }
        Ok(Ternary::FALSE)
    }

    /// tsc-port: compareSignaturesRelated @6.0.3
    /// tsc-hash: f0bf35ef85d54ae89a84377951424fb5b87b8ab55c8fc6ea30099c669d861e3b
    /// tsc-span: _tsc.js:64487-64605
    ///
    /// M3 dispositions: rest-parameter positions never construct
    /// (array rest annotations are M4), so getNonArrayRestType is
    /// None and the rest-index machinery is dead; the
    /// unreliable-marker instantiation is variance measurement
    /// (M4 5.3b). strictVariance keys on the target DECLARATION kind
    /// (method bivariance, core-interfaces §4 from_method).
    /// `compare_types` is tsc's compareTypes parameter: the type
    /// comparisons below run through the walker's own is_related_to
    /// with the threaded intersectionState — exactly signatureRelated-
    /// To's isRelatedToWorker closure (67070-67080) for the
    /// RelationFrame producer, and the fresh-walker construction at
    /// isImplementationCompatibleWithOverload models
    /// compareTypesAssignable; the enum value additionally rides the
    /// generic-source arm into iSICO's constraint clamp.
    fn compare_signatures_related(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        check_mode: i32,
        intersection_state: IntersectionState,
        report_unreliable_markers: Option<crate::instantiate::MapperId>,
        compare_types: crate::inference::CompareTypesFn,
    ) -> CheckResult2<Ternary> {
        let mut source = source;
        let mut target = target;
        if source == target {
            return Ok(Ternary::TRUE);
        }
        if !(check_mode & check_mode::STRICT_TOP_SIGNATURE != 0
            && self.st.is_top_signature(source)?)
            && self.st.is_top_signature(target)?
        {
            return Ok(Ternary::TRUE);
        }
        if check_mode & check_mode::STRICT_TOP_SIGNATURE != 0
            && self.st.is_top_signature(source)?
            && !self.st.is_top_signature(target)?
        {
            return Ok(Ternary::FALSE);
        }
        let target_count = self.st.get_parameter_count(target)?;
        let source_has_more_parameters = !self.st.has_effective_rest_parameter(target)?
            && (if check_mode & check_mode::STRICT_ARITY != 0 {
                self.st.has_effective_rest_parameter(source)?
                    || self.st.get_parameter_count(source)? > target_count
            } else {
                self.st.get_min_argument_count(source)? > target_count
            });
        if source_has_more_parameters {
            return Ok(Ternary::FALSE);
        }
        // 64505-64514 in tsc order (m4-review B8 rebuilt the head —
        // the old early gate contained cells the top-signature/arity
        // checks above decide without inference): a generic source
        // instantiates in the context of the CANONICAL target with the
        // frame loan PARKED on the state so the constraint clamp —
        // including re-entrant forward-slot resolutions through the
        // non-fixing mapper's deferred thunks (7.5d review fix) —
        // compares under this live frame. tsc's `source.typeParameters
        // !== target.typeParameters` is array identity; value-equal
        // lists arise only where tsc also shares or alpha-degenerates
        // (interned same-signature at entry, cloneSignature's shared
        // array, fresh-array lifts naming the same parameter TypeIds —
        // the 5.2e argument, amended 7.5d).
        if self.st.signature_of(source).type_parameters.is_some()
            && self.st.signature_of(source).type_parameters
                != self.st.signature_of(target).type_parameters
        {
            target = self.st.get_canonical_signature(target)?;
            let frame = self.loan_frame(intersection_state);
            // Nested arms save/restore the outer slot value (an outer
            // clamp's InFlight marker included) so arbitrary depths
            // stay balanced.
            let saved = std::mem::replace(
                &mut self.st.relation_frame_loan,
                crate::engine::RelationFrameLoan::Available(frame),
            );
            let instantiated = self.st.instantiate_signature_in_context_of(
                source,
                target,
                /*inference_context*/ None,
                Some(compare_types),
            );
            let parked = std::mem::replace(&mut self.st.relation_frame_loan, saved);
            let crate::engine::RelationFrameLoan::Available(frame) = parked else {
                panic!(
                    "the parked RelationFrame loan must come back Available — every clamp \
                     compare puts it back before returning (Err included)"
                );
            };
            self.restore_frame(frame);
            source = instantiated?;
        }
        let source_count = self.st.get_parameter_count(source)?;
        let source_rest_type = self.st.get_non_array_rest_type(source)?;
        let target_rest_type = self.st.get_non_array_rest_type(target)?;
        if let Some(probe) = source_rest_type.or(target_rest_type) {
            // 64518-64520: `void instantiateType(sourceRestType ||
            // targetRestType, reportUnreliableMarkers)`; a None mapper
            // is tsc's undefined — instantiateType is the identity and
            // no marker fires.
            if let Some(mapper) = report_unreliable_markers {
                self.st.instantiate_type(probe, Some(mapper))?;
            }
        }
        let target_kind = self
            .st
            .signature_of(target)
            .declaration
            .map(|declaration| self.st.kind_of(declaration));
        let strict_variance = check_mode & check_mode::CALLBACK == 0
            && self.st.strict_function_types
            && target_kind != Some(SyntaxKind::MethodDeclaration)
            && target_kind != Some(SyntaxKind::MethodSignature)
            && target_kind != Some(SyntaxKind::Constructor);
        let mut result = Ternary::TRUE;
        let source_this_type = self.st.get_this_type_of_signature(source)?;
        if let Some(source_this_type) = source_this_type {
            if source_this_type != self.st.tables.intrinsics.void {
                if let Some(target_this_type) = self.st.get_this_type_of_signature(target)? {
                    let bivariant = if !strict_variance {
                        self.is_related_to(
                            source_this_type,
                            target_this_type,
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            intersection_state,
                        )?
                    } else {
                        Ternary::FALSE
                    };
                    let related = if is_true(bivariant) {
                        bivariant
                    } else {
                        self.is_related_to(
                            target_this_type,
                            source_this_type,
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            intersection_state,
                        )?
                    };
                    if !is_true(related) {
                        return Ok(Ternary::FALSE);
                    }
                    result = ternary_and(result, related);
                }
            }
        }
        let param_count = if source_rest_type.is_some() || target_rest_type.is_some() {
            source_count.min(target_count)
        } else {
            source_count.max(target_count)
        };
        let rest_index: isize = if source_rest_type.is_some() || target_rest_type.is_some() {
            param_count as isize - 1
        } else {
            -1
        };
        for i in 0..param_count {
            // 64546-64547: the rest position reads through
            // getRestOrAnyTypeAtPosition on both sides.
            let source_type = if i as isize == rest_index {
                Some(self.st.get_rest_or_any_type_at_position(source, i)?)
            } else {
                self.st.try_get_type_at_position(source, i)?
            };
            let target_type = if i as isize == rest_index {
                Some(self.st.get_rest_or_any_type_at_position(target, i)?)
            } else {
                self.st.try_get_type_at_position(target, i)?
            };
            let (Some(source_type), Some(target_type)) = (source_type, target_type) else {
                continue;
            };
            if source_type != target_type || check_mode & check_mode::STRICT_ARITY != 0 {
                // 64549-64550: callback treatment is suppressed both
                // in callback checkMode AND for positions that were
                // generic pre-instantiation (isInstantiatedGeneric-
                // Parameter — 7.5d review closed the missing
                // disjunct).
                let source_sig = if check_mode & check_mode::CALLBACK != 0
                    || self.st.is_instantiated_generic_parameter(source, i)?
                {
                    None
                } else {
                    let non_nullable = self.st.remove_nullable_for_callback_gate(source_type);
                    self.st.get_single_call_signature(non_nullable)?
                };
                let target_sig = if check_mode & check_mode::CALLBACK != 0
                    || self.st.is_instantiated_generic_parameter(target, i)?
                {
                    None
                } else {
                    let non_nullable = self.st.remove_nullable_for_callback_gate(target_type);
                    self.st.get_single_call_signature(non_nullable)?
                };
                let callbacks = match (source_sig, target_sig) {
                    (Some(source_sig), Some(target_sig)) => {
                        // 64551: the callback cell requires BOTH
                        // signatures predicate-free — a predicate on
                        // either side just falls to the plain
                        // bivariant compare below (m4-review B7
                        // retired the over-contained pre-gate; the
                        // relation consult Errs on body-inferred
                        // candidates, 7.5d review). Consult order
                        // (short-circuit): source predicate, target
                        // predicate, then facts.
                        self.st
                            .relation_type_predicate_of_signature(source_sig)?
                            .is_none()
                            && self
                                .st
                                .relation_type_predicate_of_signature(target_sig)?
                                .is_none()
                            && self.st.undefined_null_facts(source_type)
                                == self.st.undefined_null_facts(target_type)
                    }
                    _ => false,
                };
                let mut related = if callbacks {
                    self.compare_signatures_related(
                        target_sig.expect("callbacks implies both signatures"),
                        source_sig.expect("callbacks implies both signatures"),
                        check_mode & check_mode::STRICT_ARITY
                            | if strict_variance {
                                check_mode::STRICT_CALLBACK
                            } else {
                                check_mode::BIVARIANT_CALLBACK
                            },
                        intersection_state,
                        report_unreliable_markers,
                        compare_types,
                    )?
                } else {
                    let bivariant = if check_mode & check_mode::CALLBACK == 0 && !strict_variance {
                        self.is_related_to(
                            source_type,
                            target_type,
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            intersection_state,
                        )?
                    } else {
                        Ternary::FALSE
                    };
                    if is_true(bivariant) {
                        bivariant
                    } else {
                        self.is_related_to(
                            target_type,
                            source_type,
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            intersection_state,
                        )?
                    }
                };
                if is_true(related)
                    && check_mode & check_mode::STRICT_ARITY != 0
                    && i >= self.st.get_min_argument_count(source)?
                    && i < self.st.get_min_argument_count(target)?
                    && is_true(self.is_related_to(
                        source_type,
                        target_type,
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        intersection_state,
                    )?)
                {
                    related = Ternary::FALSE;
                }
                if !is_true(related) {
                    return Ok(Ternary::FALSE);
                }
                result = ternary_and(result, related);
            }
        }
        if check_mode & check_mode::IGNORE_RETURN_TYPES == 0 {
            // KNOWN-GAP (checkJs band): tsc's isJSConstructor arms
            // (64577/64581 — a JS constructor's "return type" is its
            // declared class/interface type) are unported on both
            // sides; isJSConstructor requires isInJSFile, so the arms
            // are dead for TS inputs and land with checkJs (M7/M8).
            let target_resolving = self
                .st
                .signature_of(target)
                .resolved_return_type
                .is_resolving();
            let target_return_type = if target_resolving {
                self.st.tables.intrinsics.any
            } else {
                self.st.get_return_type_of_signature(target)?
            };
            if target_return_type == self.st.tables.intrinsics.void
                || target_return_type == self.st.tables.intrinsics.any
            {
                return Ok(result);
            }
            let source_resolving = self
                .st
                .signature_of(source)
                .resolved_return_type
                .is_resolving();
            let source_return_type = if source_resolving {
                self.st.tables.intrinsics.any
            } else {
                self.st.get_return_type_of_signature(source)?
            };
            // 64577-64592: the type-predicate arm (m4-review B7
            // restored tsc's decision table). The machinery
            // (compareTypePredicateRelatedTo) runs only when BOTH
            // sides carry predicates; a target-only identifier/this
            // predicate is a hard False (tsc's Signature_0_must_be_a_
            // type_predicate chain rides the T2 elaboration
            // containment); an asserts-form target alone falls
            // through with NO return-type comparison (in practice
            // the void-target early return above already caught it —
            // probed b7_target_only_asserts); a predicate-free
            // target takes the plain return comparison whatever the
            // source carries (a source-only is-predicate compares as
            // boolean, an asserts-source as void — probed
            // b7_source_only / b7_source_only_asserts). Both consults
            // ride the relation face, which Errs on body-inferred
            // candidates instead of mis-deciding the source-None
            // cells (7.5d review — the overload-fabrication FP face).
            let target_type_predicate = self.st.relation_type_predicate_of_signature(target)?;
            if let Some(target_type_predicate) = target_type_predicate {
                let source_type_predicate = self.st.relation_type_predicate_of_signature(source)?;
                if let Some(source_type_predicate) = source_type_predicate {
                    // 64580: result &= — a False verdict falls to the
                    // tail return, not an early exit (tsc has none
                    // here).
                    let related = self.compare_type_predicate_related_to(
                        &source_type_predicate,
                        &target_type_predicate,
                        intersection_state,
                    )?;
                    result = ternary_and(result, related);
                } else if matches!(
                    target_type_predicate.kind,
                    crate::narrow::TypePredicateKind::Identifier
                        | crate::narrow::TypePredicateKind::This
                ) {
                    return Ok(Ternary::FALSE);
                }
                return Ok(result);
            }
            let bivariant = if check_mode & check_mode::BIVARIANT_CALLBACK != 0 {
                self.is_related_to(
                    target_return_type,
                    source_return_type,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    intersection_state,
                )?
            } else {
                Ternary::FALSE
            };
            let related = if is_true(bivariant) {
                bivariant
            } else {
                self.is_related_to(
                    source_return_type,
                    target_return_type,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    intersection_state,
                )?
            };
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: compareTypePredicateRelatedTo @6.0.3
    /// tsc-hash: 6eebe74bc78f1d45a4471365a7ce2ec3ef940b92c4d5dd79542dd1a14280f72b
    /// tsc-span: _tsc.js:64606-64628
    ///
    /// Verdicts only — every reporting cell (the this-based-guard
    /// 2518 + 1226 pair, the 1227 + 1226 parameter-position pair, and
    /// the bare 1226 wrap) feeds tsc's elaboration chain under the
    /// outer head and rides the port's T2 containment. compareTypes =
    /// the frame worker
    /// (isRelatedToWorker2 captures the caller's intersectionState;
    /// the clamp-style two-arg call means reportErrors=false).
    /// `source.type === target.type` covers the both-None cell
    /// (asserts-form pairs — probed b7_asserts_both reaches the void
    /// early return first, but identifier pairs with equal types land
    /// here); a one-sided type is False.
    fn compare_type_predicate_related_to(
        &mut self,
        source: &crate::narrow::TypePredicate,
        target: &crate::narrow::TypePredicate,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if source.kind != target.kind {
            return Ok(Ternary::FALSE);
        }
        if matches!(
            source.kind,
            crate::narrow::TypePredicateKind::Identifier
                | crate::narrow::TypePredicateKind::AssertsIdentifier
        ) && source.parameter_index != target.parameter_index
        {
            return Ok(Ternary::FALSE);
        }
        if source.ty == target.ty {
            return Ok(Ternary::TRUE);
        }
        if let (Some(source_type), Some(target_type)) = (source.ty, target.ty) {
            return self.is_related_to(
                source_type,
                target_type,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                intersection_state,
            );
        }
        Ok(Ternary::FALSE)
    }

    /// tsc-port: membersRelatedToIndexInfo @6.0.3
    /// tsc-hash: 668b966db5f3fe04e1daa3833ea2bb542fbee71a34c087ba9d62ea7ca93ab2b6
    /// tsc-span: _tsc.js:67108-67147
    ///
    /// The optional-property NEUndefined narrowing uses the M3
    /// type-facts slice (full getTypeWithFacts is M5).
    fn members_related_to_index_info(
        &mut self,
        source: TypeId,
        target_info: &IndexInfo,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let mut result = Ternary::TRUE;
        let key_type = target_info.key_type;
        let props = if self.flags(source).intersects(TypeFlags::INTERSECTION) {
            self.st
                .get_properties_of_union_or_intersection_type(source)?
        } else {
            self.st.get_properties_of_object_type_owned(source)?
        };
        for prop in props {
            // 66968: include StringOrNumberLiteralOrUnique, non-public
            // members included.
            let name_type = self.st.get_literal_type_from_property(
                prop,
                TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
                /*include_non_public*/ true,
            )?;
            if !self.st.is_applicable_index_type(name_type, key_type)? {
                continue;
            }
            let prop_type = self.st.get_non_missing_type_of_symbol(prop)?;
            let ty = if self.st.tables.exact_optional_property_types
                || self.flags(prop_type).intersects(TypeFlags::UNDEFINED)
                || key_type == self.st.tables.intrinsics.number
                || !self.st.symbol_flags(prop).intersects(SymbolFlags::OPTIONAL)
            {
                prop_type
            } else {
                // getTypeWithFacts(propType, NEUndefined): the M3
                // slice removes undefined from unions.
                self.st.tables.filter_type(prop_type, |tables, t| {
                    !tables.flags_of(t).intersects(TypeFlags::UNDEFINED)
                })
            };
            let related = self.is_related_to(
                ty,
                target_info.value_type,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                intersection_state,
            )?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        for info in self.st.get_index_infos_of_type(source)? {
            if self.st.is_applicable_index_type(info.key_type, key_type)? {
                let related = self.index_info_related_to(&info, target_info, intersection_state)?;
                if !is_true(related) {
                    return Ok(Ternary::FALSE);
                }
                result = ternary_and(result, related);
            }
        }
        Ok(result)
    }

    /// tsc-port: indexInfoRelatedTo @6.0.3
    /// tsc-hash: f57f27c9133e11aad3954307992e0f6b751c67c451e8b4e6dbd45a69380254b6
    /// tsc-span: _tsc.js:67148-67166
    fn index_info_related_to(
        &mut self,
        source_info: &IndexInfo,
        target_info: &IndexInfo,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        self.is_related_to(
            source_info.value_type,
            target_info.value_type,
            RecursionFlags::BOTH,
            /*report_errors*/ false,
            intersection_state,
        )
    }

    /// tsc-port: indexSignaturesRelatedTo @6.0.3
    /// tsc-hash: 5ac6b8323f9bba8f6375b23effca9a1c9e5655a0270b0a7170ae19cea1b544df
    /// tsc-span: _tsc.js:67167-67182
    fn index_signatures_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        source_is_primitive: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if self.relation == RelationKind::Identity {
            return self.index_signatures_identical_to(source, target);
        }
        let index_infos = self.st.get_index_infos_of_type(target)?;
        let string = self.st.tables.intrinsics.string;
        let target_has_string_index = index_infos.iter().any(|info| info.key_type == string);
        let mut result = Ternary::TRUE;
        for target_info in &index_infos {
            let related = if self.relation != RelationKind::StrictSubtype
                && !source_is_primitive
                && target_has_string_index
                && self
                    .flags(target_info.value_type)
                    .intersects(TypeFlags::ANY)
            {
                Ternary::TRUE
            } else {
                // Generic mapped sources are M4.
                self.type_related_to_index_info(source, target_info, intersection_state)?
            };
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: typeRelatedToIndexInfo @6.0.3
    /// tsc-hash: e48488b753c4b6180855ca0982c10ee94ea5d1c158e53745dae74a99201094c0
    /// tsc-span: _tsc.js:67183-67195
    fn type_related_to_index_info(
        &mut self,
        source: TypeId,
        target_info: &IndexInfo,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if let Some(source_info) = self
            .st
            .get_applicable_index_info(source, target_info.key_type)?
        {
            return self.index_info_related_to(&source_info, target_info, intersection_state);
        }
        if !intersection_state.intersects(IntersectionState::SOURCE)
            && (self.relation != RelationKind::StrictSubtype
                || self
                    .st
                    .tables
                    .object_flags_of(source)
                    .intersects(ObjectFlags::FRESH_LITERAL))
            && self.st.is_object_type_with_inferable_index(source)?
        {
            return self.members_related_to_index_info(source, target_info, intersection_state);
        }
        Ok(Ternary::FALSE)
    }

    /// tsc-port: indexSignaturesIdenticalTo @6.0.3
    /// tsc-hash: fa902ee78d68974dfd60e8a14880bdc86dfe5f39bf40ae57bbd9b18ee19c92f5
    /// tsc-span: _tsc.js:67196-67209
    fn index_signatures_identical_to(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<Ternary> {
        let source_infos = self.st.get_index_infos_of_type(source)?;
        let target_infos = self.st.get_index_infos_of_type(target)?;
        if source_infos.len() != target_infos.len() {
            return Ok(Ternary::FALSE);
        }
        for target_info in &target_infos {
            let source_info = source_infos
                .iter()
                .find(|info| info.key_type == target_info.key_type);
            let Some(source_info) = source_info else {
                return Ok(Ternary::FALSE);
            };
            let related = self.is_related_to(
                source_info.value_type,
                target_info.value_type,
                RecursionFlags::BOTH,
                /*report_errors*/ false,
                IntersectionState::NONE,
            )?;
            if !(is_true(related) && source_info.is_readonly == target_info.is_readonly) {
                return Ok(Ternary::FALSE);
            }
        }
        Ok(Ternary::TRUE)
    }
}

/// tsc-port: getStartElementCount @6.0.3
/// tsc-hash: fcf9827ec361f2dac8727ef3c403ac9bbacc3a60a084a0c80509595292a20dc3
/// tsc-span: _tsc.js:61302-61305
fn start_element_count(element_flags: &[ElementFlags], flags: ElementFlags) -> usize {
    element_flags
        .iter()
        .position(|f| !f.intersects(flags))
        .unwrap_or(element_flags.len())
}

/// tsc-port: getEndElementCount @6.0.3
/// tsc-hash: c3739123ad58c1758730f324c93058d5e469afa256e6e8c2c66ce2684192b5d1
/// tsc-span: _tsc.js:61306-61308
pub(crate) fn end_element_count(element_flags: &[ElementFlags], flags: ElementFlags) -> usize {
    let last = element_flags.iter().rposition(|f| !f.intersects(flags));
    match last {
        Some(index) => element_flags.len() - index - 1,
        None => element_flags.len(),
    }
}

/// tsc isNumericLiteralName as used by propertiesRelatedTo tuple-name
/// filtering (canonical non-negative integers).
fn is_numeric_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_digit())
        && (name == "0" || !name.starts_with('0'))
}

/// tsc cartesianProduct (1145).
fn cartesian_product(arrays: &[Vec<TypeId>]) -> Vec<Vec<TypeId>> {
    let mut result: Vec<Vec<TypeId>> = vec![Vec::new()];
    for array in arrays {
        let mut next = Vec::with_capacity(result.len() * array.len());
        for prefix in &result {
            for &item in array {
                let mut combination = prefix.clone();
                combination.push(item);
                next.push(combination);
            }
        }
        result = next;
    }
    result
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getApparentType @6.0.3
    /// tsc-hash: 619ac2a1ef46eed57fbe781a4e1aaf381e2d5e02401d1f26609ce218ae2beedb
    /// tsc-span: _tsc.js:59093-59097
    ///
    /// tsc-port: getApparentType @6.0.3
    /// tsc-hash: 619ac2a1ef46eed57fbe781a4e1aaf381e2d5e02401d1f26609ce218ae2beedb
    /// tsc-span: _tsc.js:59093-59097
    ///
    /// getApparentTypeOfMappedType is M8 (Mapped is unconstructible).
    /// Wrapper globals resolve through the lazy 5.0 accessors — in the
    /// noLib world getGlobalType's failure fallback is emptyObjectType
    /// with the file-less one-shot 2318, invisible to fixture files.
    pub fn get_apparent_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let t = if self.tables.flags_of(ty).intersects(TypeFlags::INSTANTIABLE) {
            self.get_base_constraint_of_type(ty)?
                .unwrap_or(self.tables.intrinsics.unknown)
        } else {
            ty
        };
        let object_flags = self.tables.object_flags_of(t);
        if object_flags.intersects(ObjectFlags::MAPPED) {
            return Err(Unsupported::new("mapped type apparent types (M8)"));
        }
        if object_flags.intersects(ObjectFlags::REFERENCE) && t != ty {
            return self.get_type_with_this_argument(
                t,
                Some(ty),
                /*need_apparent_type*/ false,
            );
        }
        let flags = self.tables.flags_of(t);
        if flags.intersects(TypeFlags::INTERSECTION) {
            return self.get_apparent_type_of_intersection_type(t, ty);
        }
        if flags.intersects(TypeFlags::STRING_LIKE) {
            return self.global_string_type();
        }
        if flags.intersects(TypeFlags::NUMBER_LIKE) {
            return self.global_number_type();
        }
        if flags.intersects(TypeFlags::BIG_INT_LIKE) {
            return self.global_big_int_type();
        }
        if flags.intersects(TypeFlags::BOOLEAN_LIKE) {
            return self.global_boolean_type();
        }
        if flags.intersects(TypeFlags::ES_SYMBOL_LIKE) {
            return self.global_es_symbol_type();
        }
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return Ok(self.empty_object_type);
        }
        if flags.intersects(TypeFlags::INDEX) {
            return Ok(self.tables.intrinsics.string_number_symbol);
        }
        if flags.intersects(TypeFlags::UNKNOWN) && !self.tables.strict_null_checks {
            return Ok(self.empty_object_type);
        }
        Ok(t)
    }

    /// tsc-port: getApparentTypeOfIntersectionType @6.0.3
    /// tsc-hash: b1bd165210931aa78c769674782d148e3fe51b01f793974e32e98f590b4f7def
    /// tsc-span: _tsc.js:59026-59043
    ///
    /// tsc's resolvedApparentType slot and the `I{id},{id}` cachedTypes
    /// entry are pure caches over the deterministic, interning
    /// getTypeWithThisArgument — elided (recomputation returns the
    /// identical type ids).
    fn get_apparent_type_of_intersection_type(
        &mut self,
        ty: TypeId,
        this_argument: TypeId,
    ) -> CheckResult2<TypeId> {
        self.get_type_with_this_argument(ty, Some(this_argument), /*need_apparent_type*/ true)
    }

    /// tsc-port: getReducedApparentType @6.0.3
    /// tsc-hash: b1cf0cc54d00b7b1594d9b40a29bcf06e62f81238c47afb524d6e6f82a8f9ec3
    /// tsc-span: _tsc.js:59098-59100
    pub fn get_reduced_apparent_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let reduced = self.get_reduced_type(ty)?;
        let apparent = self.get_apparent_type(reduced)?;
        self.get_reduced_type(apparent)
    }

    /// tsc-port: getPropertiesOfType @6.0.3
    /// tsc-hash: 24909f78d7ea360522b5188e5af3c7b09613e4dc2e455ea321c4ec054b4d7576
    /// tsc-span: _tsc.js:58745-58748
    pub fn get_properties_of_type_full(&mut self, ty: TypeId) -> CheckResult2<Vec<SymbolId>> {
        let reduced = self.get_reduced_apparent_type(ty)?;
        if self
            .tables
            .flags_of(reduced)
            .intersects(TypeFlags::UNION_OR_INTERSECTION)
        {
            self.get_properties_of_union_or_intersection_type(reduced)
        } else {
            self.get_properties_of_object_type_owned(reduced)
        }
    }

    /// tsc-port: getPropertiesOfObjectType @6.0.3
    /// tsc-hash: 0f05e506ca30063507136680c157e7c4a6dd4ee239ed17e4f2e5b351133e393f
    /// tsc-span: _tsc.js:58705-58710
    pub fn get_properties_of_object_type_owned(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Vec<SymbolId>> {
        if self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            let members = self.resolve_structured_type_members(ty)?;
            return Ok(self.members_of(members).properties.clone());
        }
        Ok(Vec::new())
    }

    /// tsc-port: getPropertyOfObjectType @6.0.3
    /// tsc-hash: 8bd506ce7021670c0037b7ad4db75e2324fd5de9f14eb24b8c4233cf095e369e
    /// tsc-span: _tsc.js:58711-58719
    pub fn get_property_of_object_type(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<SymbolId>> {
        self.get_property_of_object_type_with_include_type_only_members(
            ty, name, /*include_type_only_members*/ false,
        )
    }

    fn get_property_of_object_type_with_include_type_only_members(
        &mut self,
        ty: TypeId,
        name: &str,
        include_type_only_members: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(None);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let Some(symbol) = self.members_of(members).members.get(name).copied() else {
            return Ok(None);
        };
        let hidden_type_only_export = !include_type_only_members
            && self.tables.type_of(ty).symbol.is_some_and(|module_symbol| {
                self.symbol_flags(module_symbol)
                    .intersects(SymbolFlags::VALUE_MODULE)
                    && self
                        .links
                        .symbol(module_symbol)
                        .type_only_export_star_map
                        .as_ref()
                        .is_some_and(|map| map.contains_key(name))
            });
        if !hidden_type_only_export && self.symbol_is_value(symbol, include_type_only_members)? {
            Ok(Some(symbol))
        } else {
            Ok(None)
        }
    }

    /// tsc-port: symbolIsValue @6.0.3
    /// tsc-hash: 99627f0ab0d15959cbc9fb63863a3370f651da6ac4b4a023c76a4bf90342a9b6
    /// tsc-span: _tsc.js:59433-59437
    pub(crate) fn symbol_is_value(
        &mut self,
        symbol: SymbolId,
        include_type_only_members: bool,
    ) -> CheckResult2<bool> {
        let flags = self.symbol_flags(symbol);
        Ok(flags.intersects(SymbolFlags::VALUE)
            || flags.intersects(SymbolFlags::ALIAS)
                && self
                    .get_symbol_flags_full(
                        symbol,
                        /*exclude_type_only_meanings*/ !include_type_only_members,
                        /*exclude_local_meanings*/ false,
                    )?
                    .intersects(SymbolFlags::VALUE))
    }

    /// tsc-port: getPropertiesOfUnionOrIntersectionType @6.0.3
    /// tsc-hash: cb4345ee23c44cc45e806ec68f48df4f2149eb4fe9bb8be10afeae9bda21e4ab
    /// tsc-span: _tsc.js:58720-58744
    ///
    /// Note the union quirk ported verbatim: after a constituent with
    /// NO index infos, the loop breaks (58737-58739).
    pub fn get_properties_of_union_or_intersection_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Vec<SymbolId>> {
        if let Some(cached) = self.links.ty(ty).resolved_properties.resolved() {
            return Ok(cached.to_vec());
        }
        let is_union = self.tables.flags_of(ty).intersects(TypeFlags::UNION);
        let members = match &self.tables.type_of(ty).data {
            TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("union/intersection flag implies member data"),
        };
        let mut seen: Vec<String> = Vec::new();
        let mut result: Vec<SymbolId> = Vec::new();
        for current in members {
            for prop in self.get_properties_of_type_full(current)? {
                let name = self.binder.symbol(prop).escaped_name.clone();
                if !seen.contains(&name) {
                    seen.push(name.clone());
                    if let Some(combined) = self.get_property_of_union_or_intersection_type(
                        ty, &name, /*skip_object_function_property_augment*/ !is_union,
                    )? {
                        result.push(combined);
                    }
                }
            }
            if is_union && self.get_index_infos_of_type(current)?.is_empty() {
                break;
            }
        }
        self.links.set_type_resolved_properties(
            self.speculation_depth,
            ty,
            result.clone().into_boxed_slice(),
        );
        Ok(result)
    }

    /// tsc-port: getPropertyOfType @6.0.3
    /// tsc-hash: 39a7221f835629e1b6b6c3d3e53d7aec1032999299e682d96846922fa299498a
    /// tsc-span: _tsc.js:59348-59389
    ///
    /// M3 slice: the function/object global augmentation fallbacks are
    /// empty in the noLib world; late-bound and type-only members are
    /// M4.
    pub fn get_property_of_type_full(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<SymbolId>> {
        self.get_property_of_type_ex_with_include_type_only_members(
            ty, name, /*skip_object_function_property_augment*/ false,
            /*include_type_only_members*/ false,
        )
    }

    /// The full tsc shape (59348-59389), object/function augment
    /// included: a member-less object type still reaches
    /// Object.prototype members through globalObjectType (and callable
    /// shapes through global(Callable|Newable)FunctionType) — in noLib
    /// the lazy global getters fall back to empty types so the augment
    /// is inert, while lib-loaded programs resolve `x.toString` etc.
    /// like tsc (5.5d conformance FP find: nonPrimitiveStrictNull).
    /// symbolIsValue follows aliases, and the
    /// includeTypeOnlyMembers/typeOnlyExportStarMap gate matches the
    /// value/type-query distinction.
    pub fn get_property_of_type_ex(
        &mut self,
        ty: TypeId,
        name: &str,
        skip_object_function_property_augment: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        self.get_property_of_type_ex_with_include_type_only_members(
            ty,
            name,
            skip_object_function_property_augment,
            /*include_type_only_members*/ false,
        )
    }

    /// tsrs-native: include-carrying body behind the getPropertyOfType
    /// compatibility wrappers.
    pub(crate) fn get_property_of_type_ex_with_include_type_only_members(
        &mut self,
        ty: TypeId,
        name: &str,
        skip_object_function_property_augment: bool,
        include_type_only_members: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let reduced = self.get_reduced_apparent_type(ty)?;
        let flags = self.tables.flags_of(reduced);
        if flags.intersects(TypeFlags::OBJECT) {
            if let Some(symbol) = self.get_property_of_object_type_with_include_type_only_members(
                reduced,
                name,
                include_type_only_members,
            )? {
                return Ok(Some(symbol));
            }
            if skip_object_function_property_augment {
                return Ok(None);
            }
            let members = self.resolve_structured_type_members(reduced)?;
            let (has_call, has_construct) = {
                let resolved = self.members_of(members);
                (
                    !resolved.call_signatures.is_empty(),
                    !resolved.construct_signatures.is_empty(),
                )
            };
            let function_type = if reduced == self.any_function_type {
                Some(self.global_function_type()?)
            } else if has_call {
                Some(self.global_callable_function_type()?)
            } else if has_construct {
                Some(self.global_newable_function_type()?)
            } else {
                None
            };
            if let Some(function_type) = function_type {
                if let Some(symbol) = self.get_property_of_object_type(function_type, name)? {
                    return Ok(Some(symbol));
                }
            }
            let global_object = self.global_object_type()?;
            return self.get_property_of_object_type(global_object, name);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            if let Some(property) = self.get_property_of_union_or_intersection_type(
                reduced, name, /*skip_object_function_property_augment*/ true,
            )? {
                return Ok(Some(property));
            }
            if !skip_object_function_property_augment {
                return self.get_property_of_union_or_intersection_type(
                    reduced,
                    name,
                    skip_object_function_property_augment,
                );
            }
            return Ok(None);
        }
        if flags.intersects(TypeFlags::UNION) {
            return self.get_property_of_union_or_intersection_type(
                reduced,
                name,
                skip_object_function_property_augment,
            );
        }
        Ok(None)
    }

    /// tsc-port: getTypeOfPropertyOrIndexSignatureOfType @6.0.3
    /// tsc-hash: ae41aa69b4517daebd8ee32fa4aa6db6ef15902492947777622dd0cabd315099
    /// tsc-span: _tsc.js:55807-55817
    pub fn get_type_of_property_or_index_signature_of_type(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(prop) = self.get_property_of_type_full(ty, name)? {
            return Ok(Some(self.get_type_of_symbol(prop)?));
        }
        let Some(prop_type) = self.get_applicable_index_info_for_name(ty, name)? else {
            return Ok(None);
        };
        Ok(Some(self.tables.add_optionality(
            prop_type, /*is_property*/ true, /*is_optional*/ true,
        )))
    }

    /// tsc-port: getPropertyOfUnionOrIntersectionType @6.0.3
    /// tsc-hash: b4f449a45ce4346e6e458eb4962ea77e413761aeff9603015c0c64c55bcb87da
    /// tsc-span: _tsc.js:59283-59286
    pub fn get_property_of_union_or_intersection_type(
        &mut self,
        ty: TypeId,
        name: &str,
        skip_object_function_property_augment: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let Some(property) = self.get_union_or_intersection_property(
            ty,
            name,
            skip_object_function_property_augment,
        )?
        else {
            return Ok(None);
        };
        if self
            .get_check_flags(property)
            .intersects(CheckFlags::READ_PARTIAL)
        {
            Ok(None)
        } else {
            Ok(Some(property))
        }
    }

    /// tsc-port: isImplementationCompatibleWithOverload @6.0.3
    /// tsc-hash: 625a585f0579346256510f52327b11c5bd5aee2a2e5c655aef93f2cfb767599b
    /// tsc-span: _tsc.js:64629-64643
    /// (covers isSignatureAssignableTo 64463-64477 — the
    /// ignoreReturnTypes compareSignaturesRelated probe)
    pub(crate) fn is_implementation_compatible_with_overload(
        &mut self,
        implementation: SignatureId,
        overload: SignatureId,
    ) -> CheckResult2<bool> {
        let erased_source = self.get_erased_signature(implementation)?;
        let erased_target = self.get_erased_signature(overload)?;
        let source_return_type = self.get_return_type_of_signature(erased_source)?;
        let target_return_type = self.get_return_type_of_signature(erased_target)?;
        let return_ok = target_return_type == self.tables.intrinsics.void
            || self.is_type_related_to(
                target_return_type,
                source_return_type,
                RelationKind::Assignable,
            )?
            || self.is_type_related_to(
                source_return_type,
                target_return_type,
                RelationKind::Assignable,
            )?;
        if !return_ok {
            return Ok(false);
        }
        let relation = RelationKind::Assignable;
        let relation_count = (16_000_000 - self.relations.cache(relation).len() as i64) >> 3;
        // One walker carries ALL of compareSignaturesRelated's
        // internal compares — tsc's compareTypesAssignable enters a
        // FRESH checkTypeRelatedTo per compare, so the complexity
        // budget/overflow bookkeeping is shared here where tsc
        // re-seeds it (verdict-visible only under pathological budget
        // exhaustion; maybe stacks are empty between top-level
        // compares either way).
        let mut checker = crate::engine::RelationChecker {
            st: self,
            relation,
            maybe_keys: Vec::new(),
            maybe_keys_set: std::collections::HashSet::new(),
            source_stack: Vec::new(),
            target_stack: Vec::new(),
            maybe_count: 0,
            source_depth: 0,
            target_depth: 0,
            expanding_flags: tsrs2_types::ExpandingFlags::NONE,
            overflow: false,
            relation_count,
        };
        let verdict = checker.compare_signatures_related(
            erased_source,
            erased_target,
            check_mode::IGNORE_RETURN_TYPES,
            IntersectionState::NONE,
            /*report_unreliable_markers*/ None,
            // isSignatureAssignableTo passes compareTypesAssignable
            // (64475); the erased sides keep the generic-source arm
            // dead here either way.
            crate::inference::CompareTypesFn::Assignable,
        )?;
        Ok(verdict != Ternary::FALSE)
    }

    /// tsc-port: isReferenceToType @6.0.3
    /// tsc-hash: 84871de8faa8d88bbb9a6ac7c91c4f8b117eb0e41b0fac6b844236f5c61c5451
    /// tsc-span: _tsc.js:56990-56992
    ///
    /// `type.target` covers plain References AND GenericType/tuple
    /// targets (tsc `type.target = type`) via tables.reference_target.
    /// The tsc `target !== undefined` guard maps to callers passing a
    /// real TypeId (the emptyGenericType fallback compares unequal).
    pub(crate) fn is_reference_to_type(&self, ty: TypeId, target: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
            && self.tables.reference_target(ty) == target
    }

    /// tsc-port: isReferenceToSomeType @6.0.3
    /// tsc-hash: 339c275fb77a043a6f21ed038a1309980614105afcb362ed1386ab2e8e3675a7
    /// tsc-span: _tsc.js:56979-56989
    pub(crate) fn is_reference_to_some_type(&self, ty: TypeId, targets: &[TypeId]) -> bool {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return false;
        }
        let target = self.tables.reference_target(ty);
        targets.contains(&target)
    }

    /// tsc-port: getReducedType @6.0.3
    /// tsc-hash: abdfab6ced2592e580352b92374d1ca078ca38b3ca65ba80961e5f8c83ee32f7
    /// tsc-span: _tsc.js:59287-59297
    ///
    /// The IsNeverIntersection pair is a monotone objectFlags cache —
    /// tsc mutates the interned type in place and so does the arena.
    pub fn get_reduced_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION)
            && self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::CONTAINS_INTERSECTIONS)
        {
            if let Some(cached) = self.links.ty(ty).resolved_reduced_type.resolved() {
                return Ok(cached);
            }
            let reduced = self.get_reduced_union_type(ty)?;
            self.links
                .set_type_resolved_reduced_type(self.speculation_depth, ty, reduced);
            return Ok(reduced);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            if !self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::IS_NEVER_INTERSECTION_COMPUTED)
            {
                let properties = self.get_properties_of_union_or_intersection_type(ty)?;
                let mut is_never = false;
                for prop in properties {
                    if self.is_never_reduced_property(prop)? {
                        is_never = true;
                        break;
                    }
                }
                let mut bits = ObjectFlags::IS_NEVER_INTERSECTION_COMPUTED.bits();
                if is_never {
                    bits |= ObjectFlags::IS_NEVER_INTERSECTION.bits();
                }
                let object_flags = self.tables.object_flags_of(ty).bits() | bits;
                self.tables.type_mut(ty).object_flags = ObjectFlags::from_bits(object_flags);
            }
            return Ok(
                if self
                    .tables
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::IS_NEVER_INTERSECTION)
                {
                    self.tables.intrinsics.never
                } else {
                    ty
                },
            );
        }
        Ok(ty)
    }

    /// tsc-port: getReducedUnionType @6.0.3
    /// tsc-hash: b3c08b496383a35f8652196b191738f0a7d6d25a3857ed0a0f7e25cdc41340e1
    /// tsc-span: _tsc.js:59298-59308
    fn get_reduced_union_type(&mut self, union: TypeId) -> CheckResult2<TypeId> {
        let TypeData::Union { types, .. } = self.tables.type_of(union).data.clone() else {
            unreachable!("union flag implies union data");
        };
        let mut reduced_types = Vec::with_capacity(types.len());
        let mut changed = false;
        for &t in types.iter() {
            let reduced = self.get_reduced_type(t)?;
            changed |= reduced != t;
            reduced_types.push(reduced);
        }
        if !changed {
            return Ok(union);
        }
        let reduced = self.get_union_type_ex(&reduced_types, UnionReduction::Literal)?;
        if self.tables.flags_of(reduced).intersects(TypeFlags::UNION)
            && self
                .links
                .ty(reduced)
                .resolved_reduced_type
                .resolved()
                .is_none()
        {
            self.links
                .set_type_resolved_reduced_type(self.speculation_depth, reduced, reduced);
        }
        Ok(reduced)
    }

    /// tsc-port: isNeverReducedProperty @6.0.3
    /// tsc-hash: 6239810100fef7b16f70be793af343c3fe86bd83f6aab87b19c43d34405a50fd
    /// tsc-span: _tsc.js:59309-59311
    fn is_never_reduced_property(&mut self, prop: SymbolId) -> CheckResult2<bool> {
        if self.is_discriminant_with_never_type(prop)? {
            return Ok(true);
        }
        Ok(self.is_conflicting_private_property(prop))
    }

    /// tsc-port: isDiscriminantWithNeverType @6.0.3
    /// tsc-hash: 1fc8f6f43e8013bfb1ee1898a7b5059256701d01c190b703d2362a0853b11aa0
    /// tsc-span: _tsc.js:59312-59314
    fn is_discriminant_with_never_type(&mut self, prop: SymbolId) -> CheckResult2<bool> {
        if self.symbol_flags(prop).intersects(SymbolFlags::OPTIONAL) {
            return Ok(false);
        }
        let check_flags = self.get_check_flags(prop);
        if check_flags.bits()
            & (CheckFlags::DISCRIMINANT.bits() | CheckFlags::HAS_NEVER_TYPE.bits())
            != CheckFlags::DISCRIMINANT.bits()
        {
            return Ok(false);
        }
        let ty = self.get_type_of_symbol(prop)?;
        Ok(self.tables.flags_of(ty).intersects(TypeFlags::NEVER))
    }

    /// tsc-port: isConflictingPrivateProperty @6.0.3
    /// tsc-hash: b85fdb88b67c0b34d28d407511d88581db1c95deddc8b1ff1251ebca97fe2704
    /// tsc-span: _tsc.js:59315-59317
    /// tsc-port: elaborateNeverIntersection @6.0.3 (row builder — the
    /// caller nests it under its own head like chainDiagnosticMessages)
    /// tsc-hash: b46805cd4aa6778115885da73d54ce07116c79daa94b08ca60717787e53f011e
    /// tsc-span: _tsc.js:59325-59347
    pub(crate) fn elaborate_never_intersection_row(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<tsrs2_diags::MessageChain>> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION)
            || !self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::IS_NEVER_INTERSECTION)
        {
            return Ok(None);
        }
        let properties = self.get_properties_of_union_or_intersection_type(ty)?;
        for prop in properties.iter().copied() {
            if self.is_discriminant_with_never_type(prop)? {
                let type_name = self.type_to_string_slice(ty)?;
                let prop_name = self.symbol_display_name(prop);
                return Ok(Some(tsrs2_diags::MessageChain::new(
                    &tsrs2_diags::gen::The_intersection_0_was_reduced_to_never_because_property_1_has_conflicting_types_in_some_constituents,
                    &[type_name, prop_name],
                )));
            }
        }
        for prop in properties.iter().copied() {
            if self.is_conflicting_private_property(prop) {
                let type_name = self.type_to_string_slice(ty)?;
                let prop_name = self.symbol_display_name(prop);
                return Ok(Some(tsrs2_diags::MessageChain::new(
                    &tsrs2_diags::gen::The_intersection_0_was_reduced_to_never_because_property_1_exists_in_multiple_constituents_and_is_private_in_some,
                    &[type_name, prop_name],
                )));
            }
        }
        Ok(None)
    }

    fn is_conflicting_private_property(&self, prop: SymbolId) -> bool {
        self.binder.symbol(prop).value_declaration.is_none()
            && self
                .get_check_flags(prop)
                .intersects(CheckFlags::CONTAINS_PRIVATE)
    }

    /// tsc-port: getUnionOrIntersectionProperty @6.0.3
    /// tsc-hash: 490f85816c9419feb271b4eda40fdda59ef8b7868f80e46af09245eef7e95ab8
    /// tsc-span: _tsc.js:59246-59261
    #[allow(clippy::only_used_in_recursion)] // the skip flag is tsc's cache-key parameter
    fn get_union_or_intersection_property(
        &mut self,
        ty: TypeId,
        name: &str,
        skip: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let key = (ty, name.to_owned(), skip);
        if let Some(cached) = self.links.union_property(&key) {
            return Ok(Some(cached));
        }
        let property = self.create_union_or_intersection_property(ty, name, skip)?;
        if let Some(property) = property {
            self.links
                .set_union_property(self.speculation_depth, key, property);
        }
        Ok(property)
    }

    /// tsc-port: createUnionOrIntersectionProperty @6.0.3
    /// tsc-hash: 21791f74b0558b599db3de8950d26bd93152bbb62c3e250727503334167bf713
    /// tsc-span: _tsc.js:59101-59245
    ///
    /// Full port (A5 closed the M3-slice residue: per-member modifier
    /// folding into ContainsPublic/Protected/Private/Static, the
    /// identical-instantiation clone, accessor propFlags tracking,
    /// writeTypes and nameType propagation). One documented
    /// divergence stands: the DeferredType branch (over two
    /// constituents, 59231-59235) computes eagerly — semantics
    /// identical, the deferral is a perf cache — so
    /// deferralConstituents/deferralWriteConstituents have no port
    /// fields.
    fn create_union_or_intersection_property(
        &mut self,
        containing_type: TypeId,
        name: &str,
        skip: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let is_union = self
            .tables
            .flags_of(containing_type)
            .intersects(TypeFlags::UNION);
        let members = match &self.tables.type_of(containing_type).data {
            TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("union/intersection flag implies member data"),
        };
        let mut prop_flags = SymbolFlags::from_bits(0);
        let mut single_prop: Option<SymbolId> = None;
        let mut prop_set: Vec<SymbolId> = Vec::new();
        let mut index_types: Vec<TypeId> = Vec::new();
        let mut optional_flag: Option<SymbolFlags> = None;
        let mut check_flags = if is_union {
            0
        } else {
            CheckFlags::READONLY.bits()
        };
        let mut syntactic_flag = CheckFlags::SYNTHETIC_METHOD;
        let mut merged_instantiations = false;
        for current in members {
            let ty = self.get_apparent_type(current)?;
            if self.tables.is_error_type(ty)
                || self.tables.flags_of(ty).intersects(TypeFlags::NEVER)
            {
                continue;
            }
            let prop = if self
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::UNION_OR_INTERSECTION)
            {
                self.get_union_or_intersection_property(ty, name, skip)?
            } else {
                // tsc 59109: getPropertyOfType WITH the skip flag —
                // the augment-allowing second pass reaches
                // Object.prototype members on intersection
                // constituents (intersectionIncludingPropFromGlobal
                // Augmentation pins `x.hasOwnProperty`).
                self.get_property_of_type_ex(ty, name, skip)?
            };
            if let Some(prop) = prop {
                let modifiers = self.get_declaration_modifier_flags_from_symbol(prop);
                let prop_symbol_flags = self.symbol_flags(prop);
                if prop_symbol_flags.intersects(SymbolFlags::CLASS_MEMBER) {
                    let base = optional_flag.unwrap_or(if is_union {
                        SymbolFlags::from_bits(0)
                    } else {
                        SymbolFlags::OPTIONAL
                    });
                    optional_flag = Some(if is_union {
                        SymbolFlags::from_bits(
                            base.bits() | (prop_symbol_flags & SymbolFlags::OPTIONAL).bits(),
                        )
                    } else {
                        SymbolFlags::from_bits(base.bits() & prop_symbol_flags.bits())
                    });
                }
                match single_prop {
                    None => {
                        single_prop = Some(prop);
                        // 59124: `prop.flags & Accessor || Property`.
                        let accessor = prop_symbol_flags.bits() & SymbolFlags::ACCESSOR.bits();
                        prop_flags = SymbolFlags::from_bits(if accessor != 0 {
                            accessor
                        } else {
                            SymbolFlags::PROPERTY.bits()
                        });
                    }
                    Some(existing) if existing != prop => {
                        // 59126-59136: identical instantiations of one
                        // generic parent merge into a clone instead of
                        // a propSet.
                        let is_instantiation =
                            self.get_target_symbol(prop) == self.get_target_symbol(existing);
                        if is_instantiation && self.compare_properties_identical(existing, prop)? {
                            merged_instantiations = match self.binder.symbol(existing).parent {
                                Some(parent) => !self
                                    .get_local_type_parameters_of_class_or_interface_or_type_alias(
                                        parent,
                                    )
                                    .is_empty(),
                                None => false,
                            };
                        } else {
                            if prop_set.is_empty() {
                                prop_set.push(existing);
                            }
                            if !prop_set.contains(&prop) {
                                prop_set.push(prop);
                            }
                        }
                        // 59137-59139: mixed accessor/non-accessor
                        // members downgrade to a plain property.
                        let flags_accessor = prop_flags.bits() & SymbolFlags::ACCESSOR.bits();
                        if flags_accessor != 0
                            && prop_symbol_flags.bits() & SymbolFlags::ACCESSOR.bits()
                                != flags_accessor
                        {
                            prop_flags = SymbolFlags::from_bits(
                                (prop_flags.bits() & !SymbolFlags::ACCESSOR.bits())
                                    | SymbolFlags::PROPERTY.bits(),
                            );
                        }
                    }
                    _ => {}
                }
                if is_union && self.is_readonly_symbol(prop) {
                    check_flags |= CheckFlags::READONLY.bits();
                } else if !is_union && !self.is_readonly_symbol(prop) {
                    check_flags &= !CheckFlags::READONLY.bits();
                }
                // 59148-59152: fold the member's declared modifiers.
                check_flags |=
                    if !modifiers.intersects(ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER) {
                        CheckFlags::CONTAINS_PUBLIC.bits()
                    } else {
                        0
                    } | if modifiers.intersects(ModifierFlags::PROTECTED) {
                        CheckFlags::CONTAINS_PROTECTED.bits()
                    } else {
                        0
                    } | if modifiers.intersects(ModifierFlags::PRIVATE) {
                        CheckFlags::CONTAINS_PRIVATE.bits()
                    } else {
                        0
                    } | if modifiers.intersects(ModifierFlags::STATIC) {
                        CheckFlags::CONTAINS_STATIC.bits()
                    } else {
                        0
                    };
                if !self.is_prototype_property(prop) {
                    syntactic_flag = CheckFlags::SYNTHETIC_PROPERTY;
                }
            } else if is_union {
                let index_info = self.get_applicable_index_info_for_name_info(ty, name)?;
                if let Some(index_info) = index_info {
                    // 59156: an index substitute is a plain property.
                    prop_flags = SymbolFlags::from_bits(
                        (prop_flags.bits() & !SymbolFlags::ACCESSOR.bits())
                            | SymbolFlags::PROPERTY.bits(),
                    );
                    check_flags |= CheckFlags::WRITE_PARTIAL.bits()
                        | if index_info.is_readonly {
                            CheckFlags::READONLY.bits()
                        } else {
                            0
                        };
                    // 59161: tuple members contribute their rest type
                    // (or undefined for fixed-length tuples), not the
                    // index-info type.
                    index_types.push(if self.tables.is_tuple_type(ty) {
                        self.get_rest_type_of_tuple_type(ty)?
                            .unwrap_or(self.tables.intrinsics.undefined)
                    } else {
                        index_info.value_type
                    });
                } else if self.is_object_literal_type(ty) {
                    check_flags |= CheckFlags::WRITE_PARTIAL.bits();
                    index_types.push(self.tables.intrinsics.undefined);
                } else {
                    check_flags |= CheckFlags::READ_PARTIAL.bits();
                }
            }
        }
        let Some(single_prop) = single_prop else {
            return Ok(None);
        };
        if is_union
            && (!prop_set.is_empty() || check_flags & CheckFlags::PARTIAL.bits() != 0)
            && check_flags
                & (CheckFlags::CONTAINS_PRIVATE.bits() | CheckFlags::CONTAINS_PROTECTED.bits())
                != 0
            && (prop_set.is_empty() || !self.common_declarations_of_symbols(&prop_set))
        {
            return Ok(None);
        }
        if prop_set.is_empty()
            && check_flags & CheckFlags::READ_PARTIAL.bits() == 0
            && index_types.is_empty()
        {
            if merged_instantiations {
                // 59176-59186: identical instantiations of one generic
                // parent answer a fresh clone carrying containingType,
                // the transient source's resolved type/mapper, and the
                // write type. The fallible write-type read fires
                // before any clone write.
                let transient = self
                    .symbol_flags(single_prop)
                    .intersects(SymbolFlags::TRANSIENT);
                let links_type = if transient {
                    self.links.symbol(single_prop).type_of_symbol.resolved()
                } else {
                    None
                };
                let links_mapper = if transient {
                    self.links.symbol(single_prop).mapper
                } else {
                    None
                };
                let write_type = self.get_write_type_of_symbol(single_prop)?;
                let clone = self.create_symbol_with_type(single_prop, links_type);
                // 59180: parent comes from the VALUE declaration's
                // symbol (overwriting createSymbolWithType's copy).
                let parent = self
                    .binder
                    .symbol(single_prop)
                    .value_declaration
                    .and_then(|declaration| self.binder.node_symbol(declaration))
                    .and_then(|symbol| self.binder.symbol(symbol).parent);
                self.binder.symbol_mut(clone).parent = parent;
                self.links.set_symbol_union_clone_links(
                    self.speculation_depth,
                    clone,
                    containing_type,
                    links_mapper,
                );
                self.links
                    .set_symbol_write_type(self.speculation_depth, clone, write_type);
                return Ok(Some(clone));
            }
            return Ok(Some(single_prop));
        }
        let props = if prop_set.is_empty() {
            vec![single_prop]
        } else {
            prop_set
        };
        let mut declarations: Vec<tsrs2_syntax::NodeId> = Vec::new();
        let mut first_type: Option<TypeId> = None;
        let mut name_type: Option<TypeId> = None;
        let mut prop_types: Vec<TypeId> = Vec::new();
        let mut write_types: Option<Vec<TypeId>> = None;
        let mut first_value_declaration: Option<tsrs2_syntax::NodeId> = None;
        let mut has_non_uniform_value_declaration = false;
        for &prop in &props {
            let value_declaration = self.binder.symbol(prop).value_declaration;
            match (first_value_declaration, value_declaration) {
                (None, Some(declaration)) => first_value_declaration = Some(declaration),
                (Some(first), Some(declaration)) if declaration != first => {
                    has_non_uniform_value_declaration = true;
                }
                _ => {}
            }
            declarations.extend(self.binder.symbol(prop).declarations.iter().copied());
            let ty = self.get_type_of_symbol(prop)?;
            if first_type.is_none() {
                first_type = Some(ty);
                // 59206: nameType rides the FIRST member.
                name_type = self.links.symbol(prop).name_type;
            }
            // 59209-59213: writeTypes materializes (seeded with the
            // read types so far) as soon as any member's write type
            // differs.
            let write_type = self.get_write_type_of_symbol(prop)?;
            if write_types.is_some() || write_type != ty {
                write_types
                    .get_or_insert_with(|| prop_types.clone())
                    .push(write_type);
            }
            if first_type != Some(ty) {
                check_flags |= CheckFlags::HAS_NON_UNIFORM_TYPE.bits();
            }
            if self.is_literal_type_public(ty) || self.tables.is_pattern_literal_type(ty) {
                check_flags |= CheckFlags::HAS_LITERAL_TYPE.bits();
            }
            if self.tables.flags_of(ty).intersects(TypeFlags::NEVER)
                && ty != self.tables.intrinsics.unique_literal
            {
                check_flags |= CheckFlags::HAS_NEVER_TYPE.bits();
            }
            prop_types.push(ty);
        }
        prop_types.extend(index_types);
        let flags = SymbolFlags::from_bits(
            prop_flags.bits() | optional_flag.map(|f| f.bits()).unwrap_or(0),
        );
        let result = self.binder.create_symbol(flags, name.to_owned());
        // 59224-59227: value declaration + its symbol's parent when
        // uniform.
        let parent = match (has_non_uniform_value_declaration, first_value_declaration) {
            (false, Some(declaration)) => self
                .binder
                .node_symbol(declaration)
                .and_then(|symbol| self.binder.symbol(symbol).parent),
            _ => None,
        };
        {
            let symbol = self.binder.symbol_mut(result);
            symbol.declarations = declarations;
            if !has_non_uniform_value_declaration {
                symbol.value_declaration = first_value_declaration;
                if parent.is_some() {
                    symbol.parent = parent;
                }
            }
        }
        // Eager equivalent of the DeferredType branch (59228-59239):
        // both combined types compute before any links write.
        let combined = if is_union {
            self.get_union_type_ex(&prop_types, UnionReduction::Literal)?
        } else {
            self.get_intersection_type(&prop_types, tsrs2_types::IntersectionFlags::NONE)?
        };
        let combined_write = match &write_types {
            Some(write_types) => Some(if is_union {
                self.get_union_type_ex(write_types, UnionReduction::Literal)?
            } else {
                self.get_intersection_type(write_types, tsrs2_types::IntersectionFlags::NONE)?
            }),
            None => None,
        };
        self.links.set_symbol_synthetic(
            self.speculation_depth,
            result,
            CheckFlags::from_bits(syntactic_flag.bits() | check_flags),
            containing_type,
            combined,
        );
        self.links
            .set_symbol_name_type(self.speculation_depth, result, name_type);
        if let Some(write_type) = combined_write {
            self.links
                .set_symbol_write_type(self.speculation_depth, result, write_type);
        }
        Ok(Some(result))
    }

    /// tsc-port: compareProperties @6.0.3
    /// tsc-hash: 42f04303574ccb64448bdeda07e716852dbca284333cf3f172159a566c991bb9
    /// tsc-span: _tsc.js:67536-67558
    ///
    /// The identity-comparator instantiation
    /// (createUnionOrIntersectionProperty 59127 passes
    /// `(a, b) => a === b ? True : False`); the relation-engine
    /// instantiation lives on RelationChecker.
    fn compare_properties_identical(
        &mut self,
        source_prop: SymbolId,
        target_prop: SymbolId,
    ) -> CheckResult2<bool> {
        if source_prop == target_prop {
            return Ok(true);
        }
        let source_accessibility = self
            .get_declaration_modifier_flags_from_symbol(source_prop)
            .bits()
            & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits();
        let target_accessibility = self
            .get_declaration_modifier_flags_from_symbol(target_prop)
            .bits()
            & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits();
        if source_accessibility != target_accessibility {
            return Ok(false);
        }
        if source_accessibility != 0 {
            if self.get_target_symbol(source_prop) != self.get_target_symbol(target_prop) {
                return Ok(false);
            }
        } else if self
            .symbol_flags(source_prop)
            .intersects(SymbolFlags::OPTIONAL)
            != self
                .symbol_flags(target_prop)
                .intersects(SymbolFlags::OPTIONAL)
        {
            return Ok(false);
        }
        if self.is_readonly_symbol(source_prop) != self.is_readonly_symbol(target_prop) {
            return Ok(false);
        }
        let source_type = self.get_non_missing_type_of_symbol(source_prop)?;
        let target_type = self.get_non_missing_type_of_symbol(target_prop)?;
        Ok(source_type == target_type)
    }

    /// tsc-port: getCommonDeclarationsOfSymbols @6.0.3
    /// tsc-hash: 2519f1e0ae6b4266cd11c98f1ae3204d467c94db7dd960c17fc4e7ca1a20781a
    /// tsc-span: _tsc.js:59262-59281
    ///
    /// Truthiness only — the caller (59172) tests the set, never
    /// reads it. A declaration-less symbol answers false (tsc's
    /// undefined `symbol.declarations`; the port models both that and
    /// the empty array as an empty Vec).
    fn common_declarations_of_symbols(&self, symbols: &[SymbolId]) -> bool {
        let mut common: Option<Vec<tsrs2_syntax::NodeId>> = None;
        for &symbol in symbols {
            let declarations = &self.binder.symbol(symbol).declarations;
            if declarations.is_empty() {
                return false;
            }
            match &mut common {
                None => common = Some(declarations.clone()),
                Some(common) => {
                    common.retain(|declaration| declarations.contains(declaration));
                    if common.is_empty() {
                        return false;
                    }
                }
            }
        }
        common.is_some()
    }

    /// tsc-port: isPrototypeProperty @6.0.3
    /// tsc-hash: a0150259e3d1514eb8ec0975ce264af887321e6d865c941d8a3dace7eefa0a93
    /// tsc-span: _tsc.js:74862-74864
    ///
    /// The JS valueDeclaration arm is dead in TS files.
    pub(crate) fn is_prototype_property(&self, prop: SymbolId) -> bool {
        self.symbol_flags(prop).intersects(SymbolFlags::METHOD)
            || self
                .get_check_flags(prop)
                .intersects(CheckFlags::SYNTHETIC_METHOD)
    }

    fn is_literal_type_public(&self, ty: TypeId) -> bool {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::BOOLEAN) {
            return true;
        }
        if flags.intersects(TypeFlags::UNION) {
            if flags.intersects(TypeFlags::ENUM_LITERAL) {
                return true;
            }
            let TypeData::Union { types, .. } = &self.tables.type_of(ty).data else {
                return false;
            };
            return types
                .iter()
                .all(|&t| self.tables.flags_of(t).intersects(TypeFlags::UNIT));
        }
        flags.intersects(TypeFlags::UNIT)
    }

    pub fn get_check_flags(&self, symbol: SymbolId) -> CheckFlags {
        self.links.symbol(symbol).check_flags
    }

    /// tsc-port: isReadonlySymbol @6.0.3
    /// tsc-hash: f4bb3512724bb23e8f837910378f78347824481f39847034aec8d8fdf8cf6f3b
    /// tsc-span: _tsc.js:79253-79255
    ///
    /// Full port (the M3 property-modifier slice widened at 5.5a with
    /// its checkIdentifier/delete consumers): readonly check flags,
    /// readonly properties (through the 5.3e modifier-flags reader),
    /// const/using variables, get-only accessors, enum members.
    /// isReadonlyAssignmentDeclaration (Object.defineProperty shapes)
    /// is the JS band — constant-false in TS files.
    pub fn is_readonly_symbol(&self, symbol: SymbolId) -> bool {
        if self
            .get_check_flags(symbol)
            .intersects(CheckFlags::READONLY)
        {
            return true;
        }
        let flags = self.symbol_flags(symbol);
        if flags.intersects(SymbolFlags::PROPERTY)
            && self
                .get_declaration_modifier_flags_from_symbol(symbol)
                .intersects(ModifierFlags::READONLY)
        {
            return true;
        }
        if flags.intersects(SymbolFlags::VARIABLE)
            && self.get_declaration_node_flags_from_symbol(symbol)
                & (tsrs2_types::NodeFlags::CONST.bits() | tsrs2_types::NodeFlags::USING.bits())
                != 0
        {
            return true;
        }
        if flags.intersects(SymbolFlags::ACCESSOR) && !flags.intersects(SymbolFlags::SET_ACCESSOR) {
            return true;
        }
        flags.intersects(SymbolFlags::ENUM_MEMBER)
    }

    /// tsc getDeclarationNodeFlagsFromSymbol (13712): combined node
    /// flags of the value declaration.
    fn get_declaration_node_flags_from_symbol(&self, symbol: SymbolId) -> i32 {
        match self.binder.symbol(symbol).value_declaration {
            Some(declaration) => tsrs2_binder::node_util::get_combined_node_flags(
                self.binder.source_of_node(declaration),
                declaration,
            )
            .bits(),
            None => 0,
        }
    }

    /// getDeclarationModifierFlagsFromSymbol (17436), M3 slice: type
    /// members carry no accessibility/static modifiers; synthetics
    /// read the Contains* check flags.
    /// tsc getTargetSymbol (85309-85311): instantiated symbols compare
    /// by their target.
    pub(crate) fn get_target_symbol(&self, symbol: SymbolId) -> SymbolId {
        if self
            .get_check_flags(symbol)
            .intersects(CheckFlags::INSTANTIATED)
        {
            self.links
                .symbol(symbol)
                .target
                .expect("Instantiated check flag implies links.target")
        } else {
            symbol
        }
    }

    pub fn get_declaration_modifier_flags_from_symbol(&self, symbol: SymbolId) -> ModifierFlags {
        self.get_declaration_modifier_flags_from_symbol_write(symbol, /*is_write*/ false)
    }

    /// The full tsc signature (17436): isWrite selects the SETTER
    /// declaration first — live from 5.5d's write-position accessibility.
    pub fn get_declaration_modifier_flags_from_symbol_write(
        &self,
        symbol: SymbolId,
        is_write: bool,
    ) -> ModifierFlags {
        if let Some(value_declaration) = self.binder.symbol(symbol).value_declaration {
            // 17438-17441: `isWrite && find(setter) || GetAccessor &&
            // find(getter) || valueDeclaration`.
            let find_accessor = |kind: tsrs2_syntax::SyntaxKind| {
                self.binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .copied()
                    .find(|&declaration| self.kind_of(declaration) == kind)
            };
            let declaration = is_write
                .then(|| find_accessor(tsrs2_syntax::SyntaxKind::SetAccessor))
                .flatten()
                .or_else(|| {
                    self.symbol_flags(symbol)
                        .intersects(SymbolFlags::GET_ACCESSOR)
                        .then(|| find_accessor(tsrs2_syntax::SyntaxKind::GetAccessor))
                        .flatten()
                })
                .unwrap_or(value_declaration);
            let flags = tsrs2_binder::node_util::get_combined_modifier_flags(
                self.binder.source_of_node(declaration),
                declaration,
            );
            let parent_is_class = self.binder.symbol(symbol).parent.is_some_and(|parent| {
                self.binder
                    .symbol(parent)
                    .flags
                    .intersects(SymbolFlags::CLASS)
            });
            return if parent_is_class {
                flags
            } else {
                ModifierFlags::from_bits(
                    flags.bits() & !ModifierFlags::ACCESSIBILITY_MODIFIER.bits(),
                )
            };
        }
        let check_flags = self.get_check_flags(symbol);
        if check_flags.intersects(CheckFlags::SYNTHETIC) {
            // 17445-17447: accessModifier | staticModifier — the
            // STATIC OR-in is load-bearing for synthesized protected
            // statics (a mixin `typeof A & typeof B` static otherwise
            // walks the INSTANCE-protected path and fabricates 2446
            // inside its own class — the mixinAccessModifiers FP).
            let access_modifier = if check_flags.intersects(CheckFlags::CONTAINS_PRIVATE) {
                ModifierFlags::PRIVATE
            } else if check_flags.intersects(CheckFlags::CONTAINS_PUBLIC) {
                ModifierFlags::PUBLIC
            } else {
                ModifierFlags::PROTECTED
            };
            let static_modifier = if check_flags.intersects(CheckFlags::CONTAINS_STATIC) {
                ModifierFlags::STATIC
            } else {
                ModifierFlags::from_bits(0)
            };
            return ModifierFlags::from_bits(access_modifier.bits() | static_modifier.bits());
        }
        // 17449-17451: prototype properties are public statics.
        if self.symbol_flags(symbol).intersects(SymbolFlags::PROTOTYPE) {
            return ModifierFlags::from_bits(
                ModifierFlags::PUBLIC.bits() | ModifierFlags::STATIC.bits(),
            );
        }
        ModifierFlags::from_bits(0)
    }

    /// tsc-port: isDiscriminantProperty @6.0.3
    /// tsc-hash: 1b3d6f14be2183682f24b21ec0f57e84975ced1cf03ab31db92b2b62388d6a8a
    /// tsc-span: _tsc.js:69562-69573
    pub(crate) fn is_discriminant_property(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<bool> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            return Ok(false);
        }
        let Some(prop) = self.get_union_or_intersection_property(ty, name, false)? else {
            return Ok(false);
        };
        if !self
            .get_check_flags(prop)
            .intersects(CheckFlags::SYNTHETIC_PROPERTY)
        {
            return Ok(false);
        }
        if let Some(cached) = self.links.symbol(prop).is_discriminant_property {
            return Ok(cached);
        }
        let check_flags = self.get_check_flags(prop);
        let is_discriminant = if check_flags.contains(CheckFlags::DISCRIMINANT) {
            let prop_type = self.get_type_of_symbol(prop)?;
            !self.tables.is_generic_type(prop_type)
        } else {
            false
        };
        self.links
            .set_symbol_is_discriminant(self.speculation_depth, prop, is_discriminant);
        Ok(is_discriminant)
    }

    /// tsc-port: findDiscriminantProperties @6.0.3
    /// tsc-hash: 46d0c2343386366e455335ae878e9ae9cad1fe74ce3d862456208e588201ac1d
    /// tsc-span: _tsc.js:69574-69586
    pub fn find_discriminant_properties(
        &mut self,
        source_properties: &[SymbolId],
        target: TypeId,
    ) -> CheckResult2<Option<Vec<SymbolId>>> {
        let mut result: Option<Vec<SymbolId>> = None;
        for &source_property in source_properties {
            let name = self.binder.symbol(source_property).escaped_name.clone();
            if self.is_discriminant_property(target, &name)? {
                result.get_or_insert_with(Vec::new).push(source_property);
            }
        }
        Ok(result)
    }

    /// tsc-port: getNonMissingTypeOfSymbol @6.0.3
    /// tsc-hash: 1aa20c8980faf03585f0d843ee17d09615e5b1b2bb8cdca142f78edd822982d0
    /// tsc-span: _tsc.js:56976-56978
    pub fn get_non_missing_type_of_symbol(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        let ty = self.get_type_of_symbol(symbol)?;
        let optional = self.symbol_flags(symbol).intersects(SymbolFlags::OPTIONAL);
        Ok(self.remove_missing_type(ty, optional))
    }

    /// tsc-port: removeMissingType @6.0.3
    /// tsc-hash: 84f12e953d3cae2fbd118aeedd88f90504bbd65eff6a2b343bfef5a8bc2693c1
    /// tsc-span: _tsc.js:67883-67885
    pub fn remove_missing_type(&mut self, ty: TypeId, is_optional: bool) -> TypeId {
        if self.tables.exact_optional_property_types && is_optional {
            let missing = self.tables.intrinsics.missing;
            self.tables.filter_type(ty, |_, t| t != missing)
        } else {
            ty
        }
    }

    /// tsc-port: isObjectTypeWithInferableIndex @6.0.3
    /// tsc-hash: fcdbe6c1dadf0af5c346ddc29a02ffa65512065b6bc19855678ad12c7585ea04
    /// tsc-span: _tsc.js:67895-67898
    ///
    /// KNOWN-GAP since M4 (m4-review B6): tsc's symbol-flag mask also
    /// accepts Enum | ValueModule — enums and namespaces are
    /// constructible since M4 (the old "arms are M4" framing lapsed),
    /// so a namespace value assigned against an index-signature
    /// target misses the inferable-index path (probed 2322 FP class).
    /// The ObjectRestType / ReverseMapped disjuncts stay out with
    /// their unconstructed producers.
    pub(crate) fn is_object_type_with_inferable_index(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            for t in types.iter() {
                if !self.is_object_type_with_inferable_index(*t)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return Ok(false);
        };
        let flags = self.symbol_flags(symbol);
        Ok(flags.intersects(SymbolFlags::from_bits(
            SymbolFlags::OBJECT_LITERAL.bits() | SymbolFlags::TYPE_LITERAL.bits(),
        )) && !flags.intersects(SymbolFlags::CLASS)
            && !self.type_has_call_or_construct_signatures(ty)?)
    }

    /// tsc-port: getUnmatchedProperty @6.0.3
    /// tsc-hash: 488e841fefc40d75aa7fa0d3f82f6cd689fd1b4098e12d1f9ce2ba7b050c1df3
    /// tsc-span: _tsc.js:68483-68485
    ///
    /// tsc-port: getUnmatchedProperties @6.0.3
    /// tsc-hash: fbca79444245eedebe6170eec4706a5840746ffdd9d4a9d4e75c1b4fad4e323e
    /// tsc-span: _tsc.js:68464-68482
    ///
    /// The generator collapses to first-match (every caller takes the
    /// first). Static private identifier properties never participate
    /// — `typeof Derived` relates to `typeof Base` regardless of the
    /// base's `static #x`. matchDiscriminantProperties (true only from
    /// typesDefinitelyUnrelated's source→target direction) adds the
    /// present-but-mismatched unit-discriminant arm (68470-68479).
    pub(crate) fn get_unmatched_property(
        &mut self,
        source: TypeId,
        target: TypeId,
        require_optional_properties: bool,
        match_discriminant_properties: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let properties = self.get_properties_of_type(target)?;
        for target_prop in properties {
            if self.is_static_private_identifier_property(target_prop) {
                continue;
            }
            let flags = self.symbol_flags(target_prop);
            if require_optional_properties
                || !(flags.intersects(SymbolFlags::OPTIONAL)
                    || self
                        .get_check_flags(target_prop)
                        .intersects(CheckFlags::PARTIAL))
            {
                let name = self.binder.symbol(target_prop).escaped_name.clone();
                match self.get_property_of_type_full(source, &name)? {
                    None => return Ok(Some(target_prop)),
                    Some(source_prop) if match_discriminant_properties => {
                        let target_type = self.get_type_of_symbol(target_prop)?;
                        if self
                            .tables
                            .flags_of(target_type)
                            .intersects(TypeFlags::UNIT)
                        {
                            let source_type = self.get_type_of_symbol(source_prop)?;
                            if !(self.tables.flags_of(source_type).intersects(TypeFlags::ANY)
                                || self.tables.get_regular_type_of_literal_type(source_type)
                                    == self.tables.get_regular_type_of_literal_type(target_type))
                            {
                                return Ok(Some(target_prop));
                            }
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: tupleTypesDefinitelyUnrelated @6.0.3
    /// tsc-hash: 637314e9511f05762c221182289781dceebd4b2608387222b87f34c7490dbdc9
    /// tsc-span: _tsc.js:68486-68488
    fn tuple_types_definitely_unrelated(&self, source: TypeId, target: TypeId) -> bool {
        let source_target = self.tables.reference_target(source);
        let target_target = self.tables.reference_target(target);
        let (TypeData::TupleTarget(source_data), TypeData::TupleTarget(target_data)) = (
            &self.tables.type_of(source_target).data,
            &self.tables.type_of(target_target).data,
        ) else {
            unreachable!("tuple types target tuple targets");
        };
        (!target_data
            .combined_flags
            .intersects(ElementFlags::VARIADIC)
            && target_data.min_length > source_data.min_length)
            || (!target_data
                .combined_flags
                .intersects(ElementFlags::VARIABLE)
                && (source_data
                    .combined_flags
                    .intersects(ElementFlags::VARIABLE)
                    || target_data.fixed_length < source_data.fixed_length))
    }

    /// tsc-port: typesDefinitelyUnrelated @6.0.3
    /// tsc-hash: 7d40855c5fcc34c4fde7b7f229139fa505df379f6359eadd4b8c2b59768afa53
    /// tsc-span: _tsc.js:68489-68498
    pub(crate) fn types_definitely_unrelated(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        if self.tables.is_tuple_type(source) && self.tables.is_tuple_type(target) {
            return Ok(self.tuple_types_definitely_unrelated(source, target));
        }
        Ok(self
            .get_unmatched_property(
                source, target, /*require_optional_properties*/ false,
                /*match_discriminant_properties*/ true,
            )?
            .is_some()
            && self
                .get_unmatched_property(
                    target, source, /*require_optional_properties*/ false,
                    /*match_discriminant_properties*/ false,
                )?
                .is_some())
    }

    /// tsc-port: isTupleTypeStructureMatching @6.0.3
    /// tsc-hash: b92ae770de9fe4e7613a1f8d90309db616bea7b342aa8b0cd62367d9900cfdde
    /// tsc-span: _tsc.js:67833-67835
    pub(crate) fn is_tuple_type_structure_matching(&self, t1: TypeId, t2: TypeId) -> bool {
        if self.get_type_reference_arity(t1) != self.get_type_reference_arity(t2) {
            return false;
        }
        let (TypeData::TupleTarget(d1), TypeData::TupleTarget(d2)) = (
            &self.tables.type_of(self.tables.reference_target(t1)).data,
            &self.tables.type_of(self.tables.reference_target(t2)).data,
        ) else {
            unreachable!("tuple types target tuple targets");
        };
        d1.element_flags
            .iter()
            .zip(d2.element_flags.iter())
            .all(|(f1, f2)| {
                (f1.bits() & ElementFlags::VARIABLE.bits())
                    == (f2.bits() & ElementFlags::VARIABLE.bits())
            })
    }

    /// tsc-port: getTypeReferenceArity @6.0.3
    /// tsc-hash: 27899ed0c1ce76ece5e5d45bca01208dab7f039178ed0d9a749984065f40a151
    /// tsc-span: _tsc.js:60223-60225
    ///
    /// `length(type.target.typeParameters)`: element count for tuple
    /// targets; class/interface targets read the declaring symbol's
    /// parameter list (Array/ReadonlyArray → 1 on the
    /// isArrayOrTupleType consumers).
    pub(crate) fn get_type_reference_arity(&self, ty: TypeId) -> usize {
        let target = self.tables.reference_target(ty);
        if let TypeData::TupleTarget(data) = &self.tables.type_of(target).data {
            return data.type_parameters.len();
        }
        let Some(symbol) = self.tables.type_of(target).symbol else {
            return 0;
        };
        self.links
            .symbol(symbol)
            .type_parameters
            .as_deref()
            .map_or(0, <[TypeId]>::len)
    }

    // ---- protected-member override checks (5.3e) ----

    /// tsc-port: forEachProperty @6.0.3
    /// tsc-hash: d79d83ae9df34acfafd28e2c8d2a868eab9c57fcb6c534180868cd553d452b16
    /// tsc-span: _tsc.js:67432-67444
    ///
    /// Rust face: collects the leaf (non-synthetic) property fan-out
    /// instead of threading a callback.
    fn for_each_property_leaf(
        &mut self,
        prop: SymbolId,
        out: &mut Vec<SymbolId>,
    ) -> CheckResult2<()> {
        if self.get_check_flags(prop).intersects(CheckFlags::SYNTHETIC) {
            let containing = self
                .links
                .symbol(prop)
                .containing_type
                .expect("synthetic properties carry their containing type");
            let name = self.binder.symbol(prop).escaped_name.clone();
            let types = match &self.tables.type_of(containing).data {
                TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("containing types are unions or intersections"),
            };
            for t in types {
                if let Some(p) = self.get_property_of_type_full(t, &name)? {
                    self.for_each_property_leaf(p, out)?;
                }
            }
            return Ok(());
        }
        out.push(prop);
        Ok(())
    }

    /// tsc-port: getDeclaringClass @6.0.3
    /// tsc-hash: 049f21cf11685ae294ad5a29441b2addc02e7c766ab94220928627f0c3d993da
    /// tsc-span: _tsc.js:67445-67447
    pub(crate) fn get_declaring_class(&mut self, prop: SymbolId) -> CheckResult2<Option<TypeId>> {
        let Some(parent) = self.binder.symbol(prop).parent else {
            return Ok(None);
        };
        if !self
            .binder
            .symbol(parent)
            .flags
            .intersects(SymbolFlags::CLASS)
        {
            return Ok(None);
        }
        Ok(Some(self.get_declared_type_of_class_or_interface(parent)?))
    }

    /// tsc-port: isPropertyInClassDerivedFrom @6.0.3
    /// tsc-hash: 5c9e6781d9e73cdfd2454e0d78e186892726bf5afa91b5df88bc5ae0568c6184
    /// tsc-span: _tsc.js:67453-67458
    fn is_property_in_class_derived_from(
        &mut self,
        prop: SymbolId,
        base_class: Option<TypeId>,
    ) -> CheckResult2<bool> {
        let mut leaves = Vec::new();
        self.for_each_property_leaf(prop, &mut leaves)?;
        for leaf in leaves {
            if let Some(source_class) = self.get_declaring_class(leaf)? {
                if let Some(base_class) = base_class {
                    if self.has_base_type(source_class, base_class)? {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: isValidOverrideOf @6.0.3
    /// tsc-hash: a8192507b3a538f6c9fb2f21d5b73f220e58da15b182b6e219e773674c485a60
    /// tsc-span: _tsc.js:67459-67464
    pub(crate) fn is_valid_override_of(
        &mut self,
        source_prop: SymbolId,
        target_prop: SymbolId,
    ) -> CheckResult2<bool> {
        let mut leaves = Vec::new();
        self.for_each_property_leaf(target_prop, &mut leaves)?;
        for tp in leaves {
            if self
                .get_declaration_modifier_flags_from_symbol(tp)
                .intersects(ModifierFlags::PROTECTED)
            {
                let declaring = self.get_declaring_class(tp)?;
                if !self.is_property_in_class_derived_from(source_prop, declaring)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    // ---- union/intersection member synthesis (5.3d) ----

    /// tsc-port: cloneSignature @6.0.3
    /// tsc-hash: 854d0bdd7888a4d1c0d177652aa9715c0f7f9c92a2dabd703825f16449fa6adf
    /// tsc-span: _tsc.js:57868-57886
    pub(crate) fn clone_signature(&mut self, signature: SignatureId) -> SignatureId {
        let source = self.signature_of(signature).clone();
        let result = crate::state::Signature {
            declaration: source.declaration,
            flags: tsrs2_types::SignatureFlags::from_bits(
                source.flags.bits() & tsrs2_types::SignatureFlags::PROPAGATING_FLAGS.bits(),
            ),
            type_parameters: source.type_parameters.clone(),
            parameters: source.parameters.clone(),
            this_parameter: source.this_parameter,
            min_argument_count: source.min_argument_count,
            resolved_return_type: crate::links::LinkSlot::Vacant,
            from_method: source.from_method,
            target: source.target,
            mapper: source.mapper,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            canonical_signature_cache: None,
            base_signature_cache: None,
            composite_kind: source.composite_kind,
            composite_signatures: source.composite_signatures.clone(),
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: source.isolated_signature_kind,
            isolated_signature_type: None,
            // Clones of the failure stub stay stub-derived.
        };
        self.alloc_signature(result)
    }

    /// tsc-port: createUnionSignature @6.0.3
    /// tsc-hash: f2066c8b50870ca26e149fa2dfbc9f6fafda42661ec859fe0d414384c23a04c0
    /// tsc-span: _tsc.js:57887-57894
    pub(crate) fn create_union_signature(
        &mut self,
        signature: SignatureId,
        union_signatures: Vec<SignatureId>,
    ) -> SignatureId {
        let result = self.clone_signature(signature);
        let data = self.signature_mut(result);
        data.composite_signatures = Some(union_signatures);
        data.composite_kind = Some(TypeFlags::UNION);
        data.target = None;
        data.mapper = None;
        result
    }

    /// tsc-port: createSymbolWithType @6.0.3
    /// tsc-hash: 6f9c4ebd31cbdba03af7db5474a8867a0d798e87df6e7dd8c7268f7acc6d7c0d
    /// tsc-span: _tsc.js:67899-67913
    ///
    /// `type` is optional (67901): the identical-instantiation clone
    /// passes a non-transient source's vacant slot through, and the
    /// clone then computes lazily from its copied declarations.
    pub(crate) fn create_symbol_with_type(
        &mut self,
        source: SymbolId,
        ty: Option<TypeId>,
    ) -> SymbolId {
        let source_flags = self.symbol_flags(source);
        let name = self.binder.symbol(source).escaped_name.clone();
        let symbol = self.binder.create_symbol(source_flags, name);
        let readonly = tsrs2_types::CheckFlags::from_bits(
            self.get_check_flags(source).bits() & tsrs2_types::CheckFlags::READONLY.bits(),
        );
        self.links
            .set_symbol_check_flags(self.speculation_depth, symbol, readonly);
        if let Some(ty) = ty {
            self.links.set_symbol_type(
                self.speculation_depth,
                symbol,
                crate::links::LinkSlot::Resolved(ty),
            );
        }
        self.links
            .set_symbol_target(self.speculation_depth, symbol, source);
        let declarations = self.binder.symbol(source).declarations.clone();
        let parent = self.binder.symbol(source).parent;
        let value_declaration = self.binder.symbol(source).value_declaration;
        {
            let clone = self.binder.symbol_mut(symbol);
            clone.declarations = declarations;
            clone.parent = parent;
            if value_declaration.is_some() {
                clone.value_declaration = value_declaration;
            }
        }
        if let Some(name_type) = self.links.symbol(source).name_type {
            self.links
                .set_symbol_name_type(self.speculation_depth, symbol, Some(name_type));
        }
        symbol
    }

    /// The compareTypes parameter of compareSignaturesIdentical:
    /// partialMatch selects compareTypesSubtypeOf, else
    /// compareTypesIdentical (findMatchingSignature 58001).
    pub(crate) fn compare_signatures_identical_at(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        partial_match: bool,
        ignore_this_types: bool,
        ignore_return_types: bool,
    ) -> CheckResult2<bool> {
        let relation = if partial_match {
            RelationKind::Subtype
        } else {
            RelationKind::Identity
        };
        let relation_count = (16_000_000 - self.relations.cache(relation).len() as i64) >> 3;
        let mut checker = RelationChecker {
            st: self,
            relation,
            maybe_keys: Vec::new(),
            maybe_keys_set: std::collections::HashSet::new(),
            source_stack: Vec::new(),
            target_stack: Vec::new(),
            maybe_count: 0,
            source_depth: 0,
            target_depth: 0,
            expanding_flags: tsrs2_types::ExpandingFlags::NONE,
            overflow: false,
            relation_count,
        };
        let related = checker.compare_signatures_identical_ex(
            source,
            target,
            partial_match,
            ignore_this_types,
            ignore_return_types,
        )?;
        Ok(is_true(related))
    }

    /// tsc-port: findMatchingSignature @6.0.3
    /// tsc-hash: 0e0e437a5d2be21392feb70dc62cce7bf2afecac3a0ac6acbe0cdad41627008c
    /// tsc-span: _tsc.js:57999-58005
    fn find_matching_signature(
        &mut self,
        signature_list: &[SignatureId],
        signature: SignatureId,
        partial_match: bool,
        ignore_this_types: bool,
        ignore_return_types: bool,
    ) -> CheckResult2<Option<SignatureId>> {
        for &s in signature_list {
            if self.compare_signatures_identical_at(
                s,
                signature,
                partial_match,
                ignore_this_types,
                ignore_return_types,
            )? {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }

    /// tsc-port: findMatchingSignatures @6.0.3
    /// tsc-hash: b691b1412bfb3d9ce37229a1d649d14ab90df1ff159ed4c1d0ac624630792ed6
    /// tsc-span: _tsc.js:58006-58054
    fn find_matching_signatures(
        &mut self,
        signature_lists: &[Vec<SignatureId>],
        signature: SignatureId,
        list_index: usize,
    ) -> CheckResult2<Option<Vec<SignatureId>>> {
        if self.signature_of(signature).type_parameters.is_some() {
            // 58008-58023: generic signatures match in the FIRST list
            // only, and only via exact matches everywhere.
            if list_index > 0 {
                return Ok(None);
            }
            for list in &signature_lists[1..] {
                if self
                    .find_matching_signature(list, signature, false, false, false)?
                    .is_none()
                {
                    return Ok(None);
                }
            }
            return Ok(Some(vec![signature]));
        }
        let mut result: Vec<SignatureId> = Vec::new();
        for (i, list) in signature_lists.iter().enumerate() {
            let matched = if i == list_index {
                Some(signature)
            } else {
                match self.find_matching_signature(list, signature, false, false, true)? {
                    Some(matched) => Some(matched),
                    None => self.find_matching_signature(list, signature, true, false, true)?,
                }
            };
            let Some(matched) = matched else {
                return Ok(None);
            };
            if !result.contains(&matched) {
                result.push(matched);
            }
        }
        Ok(Some(result))
    }

    /// tsc-port: getUnionSignatures @6.0.3
    /// tsc-hash: de365ba44bdfdca52811439ffebf59ebc19fb1b100954118cea2daaafb974edd
    /// tsc-span: _tsc.js:58055-58108
    pub(crate) fn get_union_signatures(
        &mut self,
        signature_lists: &[Vec<SignatureId>],
    ) -> CheckResult2<Vec<SignatureId>> {
        let mut result: Vec<SignatureId> = Vec::new();
        let mut index_with_length_over_one: Option<isize> = None;
        for (i, list) in signature_lists.iter().enumerate() {
            if list.is_empty() {
                return Ok(Vec::new());
            }
            if list.len() > 1 {
                index_with_length_over_one = match index_with_length_over_one {
                    None => Some(i as isize),
                    Some(_) => Some(-1),
                };
            }
            for &signature in list {
                if self
                    .find_matching_signature(&result, signature, false, false, true)?
                    .is_some()
                {
                    continue;
                }
                let Some(union_signatures) =
                    self.find_matching_signatures(signature_lists, signature, i)?
                else {
                    continue;
                };
                let mut s = signature;
                if union_signatures.len() > 1 {
                    let mut this_parameter = self.signature_of(signature).this_parameter;
                    let first_this = union_signatures
                        .iter()
                        .find_map(|&sig| self.signature_of(sig).this_parameter);
                    if let Some(first_this) = first_this {
                        let mut this_types = Vec::new();
                        for &sig in &union_signatures {
                            if let Some(this_param) = self.signature_of(sig).this_parameter {
                                this_types.push(self.get_type_of_symbol(this_param)?);
                            }
                        }
                        let this_type = self.get_intersection_type(
                            &this_types,
                            tsrs2_types::IntersectionFlags::NONE,
                        )?;
                        this_parameter =
                            Some(self.create_symbol_with_type(first_this, Some(this_type)));
                    }
                    s = self.create_union_signature(signature, union_signatures);
                    self.signature_mut(s).this_parameter = this_parameter;
                }
                result.push(s);
            }
        }
        if result.is_empty() && index_with_length_over_one != Some(-1) {
            // 58091-58106: no common signatures — combine the master
            // list pairwise across the union members.
            let master_index = index_with_length_over_one.unwrap_or(0) as usize;
            let master_list = &signature_lists[master_index];
            let mut results: Option<Vec<SignatureId>> = Some(master_list.clone());
            for list in signature_lists {
                if std::ptr::eq(list.as_slice(), master_list.as_slice()) {
                    continue;
                }
                let signature = list[0];
                let incompatible_generics = self.signature_of(signature).type_parameters.is_some()
                    && results.as_ref().is_some_and(|results| {
                        results.iter().any(|&s| {
                            self.signatures[s.0 as usize].type_parameters.is_some()
                                && !self.compare_type_parameters_identical_ok(signature, s)
                        })
                    });
                if incompatible_generics {
                    results = None;
                } else if let Some(current) = results {
                    let mut combined = Vec::with_capacity(current.len());
                    for sig in current {
                        combined.push(self.combine_signatures_of_union_members(sig, signature)?);
                    }
                    results = Some(combined);
                }
                if results.is_none() {
                    break;
                }
            }
            result = results.unwrap_or_default();
        }
        Ok(result)
    }

    /// compareTypeParametersIdentical's boolean face for the closure
    /// position above (Err collapses to "not identical" would be
    /// dishonest — so this helper is infallible-by-construction and the
    /// fallible body lives in compare_type_parameters_identical).
    fn compare_type_parameters_identical_ok(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> bool {
        matches!(
            self.compare_type_parameters_identical(source, target),
            Ok(true)
        )
    }

    /// tsc-port: compareTypeParametersIdentical @6.0.3
    /// tsc-hash: 065a593f6bc93d374499bf9cf35327921820333c8042b23a45c6208a4c59833a
    /// tsc-span: _tsc.js:58109-58124
    pub(crate) fn compare_type_parameters_identical(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> CheckResult2<bool> {
        let source_params = self
            .signature_of(source)
            .type_parameters
            .clone()
            .unwrap_or_default();
        let target_params = self
            .signature_of(target)
            .type_parameters
            .clone()
            .unwrap_or_default();
        if source_params.len() != target_params.len() {
            return Ok(false);
        }
        if source_params.is_empty() || target_params.is_empty() {
            return Ok(true);
        }
        let mapper = self.create_type_mapper(target_params.clone(), Some(source_params.clone()));
        let unknown = self.tables.intrinsics.unknown;
        for i in 0..source_params.len() {
            let s = source_params[i];
            let t = target_params[i];
            if s == t {
                continue;
            }
            let source_constraint = self
                .get_constraint_from_type_parameter(s)?
                .unwrap_or(unknown);
            let target_constraint = match self.get_constraint_from_type_parameter(t)? {
                Some(constraint) => self.instantiate_type(constraint, Some(mapper))?,
                None => unknown,
            };
            if !self.is_type_identical_to(source_constraint, target_constraint)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn is_type_identical_to(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        self.is_type_related_to(source, target, RelationKind::Identity)
    }

    /// tsc-port: isPropertyIdenticalTo @6.0.3
    /// tsc-hash: e5bc0670b1b176c446db71d2ecbd36ba6cdc4903be20e3bb2ac807ec25c89652
    /// tsc-span: _tsc.js:67533-67535
    ///
    /// compareProperties (67536-67558) under compareTypesIdentical,
    /// transcribed standalone: the optionality mismatch check runs
    /// ONLY in the public-accessibility branch (the RelationChecker
    /// twin above checks it unconditionally per its own span).
    pub(crate) fn is_property_identical_to(
        &mut self,
        source_prop: SymbolId,
        target_prop: SymbolId,
    ) -> CheckResult2<bool> {
        if source_prop == target_prop {
            return Ok(true);
        }
        let source_accessibility = self
            .get_declaration_modifier_flags_from_symbol(source_prop)
            .bits()
            & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits();
        let target_accessibility = self
            .get_declaration_modifier_flags_from_symbol(target_prop)
            .bits()
            & ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER.bits();
        if source_accessibility != target_accessibility {
            return Ok(false);
        }
        if source_accessibility != 0 {
            if self.get_target_symbol(source_prop) != self.get_target_symbol(target_prop) {
                return Ok(false);
            }
        } else if self
            .symbol_flags(source_prop)
            .intersects(SymbolFlags::OPTIONAL)
            != self
                .symbol_flags(target_prop)
                .intersects(SymbolFlags::OPTIONAL)
        {
            return Ok(false);
        }
        if self.is_readonly_symbol(source_prop) != self.is_readonly_symbol(target_prop) {
            return Ok(false);
        }
        let source_type = self.get_non_missing_type_of_symbol(source_prop)?;
        let target_type = self.get_non_missing_type_of_symbol(target_prop)?;
        self.is_type_identical_to(source_type, target_type)
    }

    /// tsc-port: getTypeWithoutSignatures @6.0.3
    /// tsc-hash: 961358bb0c7547ebd888d3e0232d508e0fc4b2a3754c5967d0e953f68ac30903
    /// tsc-span: _tsc.js:63884-63900
    pub(crate) fn get_type_without_signatures(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::OBJECT) {
            let members = self.resolve_structured_type_members(ty)?;
            let resolved = self.members_of(members).clone();
            if !resolved.construct_signatures.is_empty() || !resolved.call_signatures.is_empty() {
                let symbol = self.tables.type_of(ty).symbol;
                let result = self.create_resolved_empty_anonymous_type(symbol);
                let result_members = self
                    .links
                    .ty(result)
                    .resolved_members
                    .resolved()
                    .expect("freshly created anonymous types carry resolved members");
                let stripped = crate::state::ResolvedMembers {
                    members: resolved.members.clone(),
                    properties: resolved.properties.clone(),
                    call_signatures: Vec::new(),
                    construct_signatures: Vec::new(),
                    index_infos: Vec::new(),
                };
                *self.members_mut(result_members) = stripped;
                return Ok(result);
            }
        } else if flags.intersects(TypeFlags::INTERSECTION) {
            let constituents = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            let mut mapped = Vec::with_capacity(constituents.len());
            for constituent in constituents {
                mapped.push(self.get_type_without_signatures(constituent)?);
            }
            return self.get_intersection_type(&mapped, tsrs2_types::IntersectionFlags::NONE);
        }
        Ok(ty)
    }

    /// tsc-port: combineUnionThisParam @6.0.3
    /// tsc-hash: c9896f07d74bd49a7292d7a74d1fae3913a232660cf3ac8a9275ca76f9c118a0
    /// tsc-span: _tsc.js:58125-58131
    fn combine_union_this_param(
        &mut self,
        left: Option<SymbolId>,
        right: Option<SymbolId>,
        mapper: Option<crate::instantiate::MapperId>,
    ) -> CheckResult2<Option<SymbolId>> {
        let (left, right) = match (left, right) {
            (Some(left), Some(right)) => (left, right),
            (left, right) => return Ok(left.or(right)),
        };
        let left_type = self.get_type_of_symbol(left)?;
        let right_type = self.get_type_of_symbol(right)?;
        let right_type = self.instantiate_type(right_type, mapper)?;
        let this_type = self.get_intersection_type(
            &[left_type, right_type],
            tsrs2_types::IntersectionFlags::NONE,
        )?;
        Ok(Some(self.create_symbol_with_type(left, Some(this_type))))
    }

    /// tsc-port: combineUnionParameters @6.0.3
    /// tsc-hash: 2396ab002984b923e2a91738a38c9d36cad7626e1dd56b17c1c534df3f62c88f
    /// tsc-span: _tsc.js:58132-58173
    ///
    /// getParameterNameAtPosition's tuple-label arm reads the labeled
    /// declaration's name text; positions past both lists fall back to
    /// `arg{i}` exactly like tsc.
    fn combine_union_parameters(
        &mut self,
        left: SignatureId,
        right: SignatureId,
        mapper: Option<crate::instantiate::MapperId>,
    ) -> CheckResult2<Vec<SymbolId>> {
        let left_count = self.get_parameter_count(left)?;
        let right_count = self.get_parameter_count(right)?;
        let (longest, shorter) = if left_count >= right_count {
            (left, right)
        } else {
            (right, left)
        };
        let longest_count = if longest == left {
            left_count
        } else {
            right_count
        };
        let either_has_effective_rest =
            self.has_effective_rest_parameter(left)? || self.has_effective_rest_parameter(right)?;
        let needs_extra_rest_element =
            either_has_effective_rest && !self.has_effective_rest_parameter(longest)?;
        let mut params: Vec<SymbolId> =
            Vec::with_capacity(longest_count + usize::from(needs_extra_rest_element));
        let longest_min = self.get_min_argument_count(longest)?;
        let shorter_min = self.get_min_argument_count(shorter)?;
        for i in 0..longest_count {
            let mut longest_param_type = self
                .try_get_type_at_position(longest, i)?
                .expect("positions below the longest count have types");
            if longest == right {
                longest_param_type = self.instantiate_type(longest_param_type, mapper)?;
            }
            let mut shorter_param_type = self
                .try_get_type_at_position(shorter, i)?
                .unwrap_or(self.tables.intrinsics.unknown);
            if shorter == right {
                shorter_param_type = self.instantiate_type(shorter_param_type, mapper)?;
            }
            let union_param_type = self.get_intersection_type(
                &[longest_param_type, shorter_param_type],
                tsrs2_types::IntersectionFlags::NONE,
            )?;
            let is_rest_param =
                either_has_effective_rest && !needs_extra_rest_element && i == longest_count - 1;
            let is_optional = i >= longest_min && i >= shorter_min;
            let left_name = if i >= left_count {
                None
            } else {
                self.get_parameter_name_at_position(left, i)?
            };
            let right_name = if i >= right_count {
                None
            } else {
                self.get_parameter_name_at_position(right, i)?
            };
            let param_name = if left_name == right_name {
                left_name
            } else if left_name.is_none() {
                right_name
            } else if right_name.is_none() {
                left_name
            } else {
                None
            };
            let mut symbol_flags = SymbolFlags::FUNCTION_SCOPED_VARIABLE;
            if is_optional && !is_rest_param {
                symbol_flags |= SymbolFlags::OPTIONAL;
            }
            let param_symbol = self.binder.create_symbol(
                symbol_flags,
                param_name.unwrap_or_else(|| format!("arg{i}")),
            );
            let check_flags = if is_rest_param {
                CheckFlags::REST_PARAMETER
            } else if is_optional {
                CheckFlags::OPTIONAL_PARAMETER
            } else {
                CheckFlags::from_bits(0)
            };
            self.links
                .set_symbol_check_flags(self.speculation_depth, param_symbol, check_flags);
            let param_type = if is_rest_param {
                self.create_array_type(union_param_type, false)?
            } else {
                union_param_type
            };
            self.links.set_symbol_type(
                self.speculation_depth,
                param_symbol,
                crate::links::LinkSlot::Resolved(param_type),
            );
            params.push(param_symbol);
        }
        if needs_extra_rest_element {
            let rest_symbol = self
                .binder
                .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, "args".to_owned());
            self.links.set_symbol_check_flags(
                self.speculation_depth,
                rest_symbol,
                CheckFlags::REST_PARAMETER,
            );
            let shorter_at = self.get_type_at_position(shorter, longest_count)?;
            let mut rest_type = self.create_array_type(shorter_at, false)?;
            if shorter == right {
                rest_type = self.instantiate_type(rest_type, mapper)?;
            }
            self.links.set_symbol_type(
                self.speculation_depth,
                rest_symbol,
                crate::links::LinkSlot::Resolved(rest_type),
            );
            params.push(rest_symbol);
        }
        Ok(params)
    }

    /// getParameterNameAtPosition (78218-78232 slice): declared
    /// positions read the parameter symbol's name; tuple-rest expanded
    /// positions read the label declaration's name text when present.
    pub(crate) fn get_parameter_name_at_position(
        &mut self,
        signature: SignatureId,
        pos: usize,
    ) -> CheckResult2<Option<String>> {
        let signature_data = self.signature_of(signature);
        let has_rest = signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER);
        let param_count = signature_data.parameters.len() - usize::from(has_rest);
        if pos < param_count {
            return Ok(Some(
                self.binder
                    .symbol(signature_data.parameters[pos])
                    .escaped_name
                    .clone(),
            ));
        }
        let Some(&rest_parameter) = self.signature_of(signature).parameters.last() else {
            return Ok(None);
        };
        Ok(Some(
            self.binder.symbol(rest_parameter).escaped_name.clone(),
        ))
    }

    /// tsc-port: combineSignaturesOfUnionMembers @6.0.3
    /// tsc-hash: 9804b081625944a8195a29bc8b837167d0185d298eb8a128d453fbdf740cdfbf
    /// tsc-span: _tsc.js:58174-58209
    fn combine_signatures_of_union_members(
        &mut self,
        left: SignatureId,
        right: SignatureId,
    ) -> CheckResult2<SignatureId> {
        let left_data = self.signature_of(left).clone();
        let right_data = self.signature_of(right).clone();
        let type_params = left_data
            .type_parameters
            .clone()
            .or_else(|| right_data.type_parameters.clone());
        let param_mapper = match (&left_data.type_parameters, &right_data.type_parameters) {
            (Some(left_params), Some(right_params)) => {
                Some(self.create_type_mapper(right_params.clone(), Some(left_params.clone())))
            }
            _ => None,
        };
        let mut flags = tsrs2_types::SignatureFlags::from_bits(
            (left_data.flags.bits() | right_data.flags.bits())
                & (tsrs2_types::SignatureFlags::PROPAGATING_FLAGS.bits()
                    & !tsrs2_types::SignatureFlags::HAS_REST_PARAMETER.bits()),
        );
        let params = self.combine_union_parameters(left, right, param_mapper)?;
        if let Some(&last_param) = params.last() {
            if self
                .get_check_flags(last_param)
                .intersects(CheckFlags::REST_PARAMETER)
            {
                flags = tsrs2_types::SignatureFlags::from_bits(
                    flags.bits() | tsrs2_types::SignatureFlags::HAS_REST_PARAMETER.bits(),
                );
            }
        }
        let this_param = self.combine_union_this_param(
            left_data.this_parameter,
            right_data.this_parameter,
            param_mapper,
        )?;
        let min_arg_count = left_data
            .min_argument_count
            .max(right_data.min_argument_count);
        let mut composite_signatures = match (
            left_data.composite_kind,
            left_data.composite_signatures.clone(),
        ) {
            (Some(kind), Some(signatures)) if !kind.intersects(TypeFlags::INTERSECTION) => {
                signatures
            }
            _ => vec![left],
        };
        composite_signatures.push(right);
        let mapper = if let Some(param_mapper) = param_mapper {
            match (
                left_data.composite_kind,
                left_data.mapper,
                &left_data.composite_signatures,
            ) {
                (Some(kind), Some(left_mapper), Some(_))
                    if !kind.intersects(TypeFlags::INTERSECTION) =>
                {
                    Some(self.combine_type_mappers(Some(left_mapper), param_mapper))
                }
                _ => Some(param_mapper),
            }
        } else {
            match (
                left_data.composite_kind,
                left_data.mapper,
                &left_data.composite_signatures,
            ) {
                (Some(kind), Some(left_mapper), Some(_))
                    if !kind.intersects(TypeFlags::INTERSECTION) =>
                {
                    Some(left_mapper)
                }
                _ => None,
            }
        };
        let result = crate::state::Signature {
            declaration: left_data.declaration,
            flags,
            type_parameters: type_params,
            parameters: params,
            this_parameter: this_param,
            min_argument_count: min_arg_count,
            resolved_return_type: crate::links::LinkSlot::Vacant,
            from_method: left_data.from_method,
            target: None,
            mapper,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            canonical_signature_cache: None,
            base_signature_cache: None,
            composite_kind: Some(TypeFlags::UNION),
            composite_signatures: Some(composite_signatures),
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: left_data.isolated_signature_kind,
            isolated_signature_type: None,
        };
        Ok(self.alloc_signature(result))
    }

    /// tsc-port: getUnionIndexInfos @6.0.3
    /// tsc-hash: 722b15b0268f26a28505f37b9d187c7d568785aa74d9cbf3adf015693e35fc9f
    /// tsc-span: _tsc.js:58210-58223
    pub(crate) fn get_union_index_infos(
        &mut self,
        types: &[TypeId],
    ) -> CheckResult2<Vec<IndexInfo>> {
        let source_infos = self.get_index_infos_of_type(types[0])?;
        let mut result = Vec::new();
        'infos: for info in source_infos {
            let key_type = info.key_type;
            let mut value_types = Vec::with_capacity(types.len());
            let mut is_readonly = false;
            for &t in types {
                let candidate = self.get_index_info_of_type(t, key_type)?;
                let Some(candidate) = candidate else {
                    continue 'infos;
                };
                value_types.push(candidate.value_type);
                is_readonly |= candidate.is_readonly;
            }
            let value = self.get_union_type_ex(&value_types, UnionReduction::Literal)?;
            result.push(IndexInfo {
                key_type,
                value_type: value,
                is_readonly,
                declaration: None,
                components: None,
                is_enum_number_index_info: false,
            });
        }
        Ok(result)
    }

    /// tsc getIndexInfoOfType (59466-59468) — findIndexInfo over the
    /// type's infos.
    pub(crate) fn get_index_info_of_type(
        &mut self,
        ty: TypeId,
        key_type: TypeId,
    ) -> CheckResult2<Option<IndexInfo>> {
        let infos = self.get_index_infos_of_type(ty)?;
        Ok(infos.into_iter().find(|info| info.key_type == key_type))
    }

    /// tsc-port: resolveUnionTypeMembers @6.0.3
    /// tsc-hash: b79146b727edab18aa3474c4ab6d0ef5d15302b2264ec1ca853d395372867afb
    /// tsc-span: _tsc.js:58224-58229
    ///
    /// A globalFunctionType member contributes [unknownSignature] to
    /// the CALL list (58225 — the declaration-less singleton exists
    /// since 5.7a); its construct list resolves normally.
    pub(crate) fn resolve_union_type_members(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<crate::state::MembersId> {
        let TypeData::Union { types, .. } = self.tables.type_of(ty).data.clone() else {
            unreachable!("union flag implies union data");
        };
        let mut call_lists = Vec::with_capacity(types.len());
        let mut construct_lists = Vec::with_capacity(types.len());
        for &t in types.iter() {
            if self.is_global_function_type(t)? {
                call_lists.push(vec![self.unknown_signature]);
            } else {
                call_lists.push(self.get_signatures_of_type(t, SignatureKind::Call)?);
            }
            construct_lists.push(self.get_signatures_of_type(t, SignatureKind::Construct)?);
        }
        let call_signatures = self.get_union_signatures(&call_lists)?;
        let construct_signatures = self.get_union_signatures(&construct_lists)?;
        let index_infos = self.get_union_index_infos(&types)?;
        let id = self.alloc_members(crate::state::ResolvedMembers {
            call_signatures,
            construct_signatures,
            index_infos,
            ..crate::state::ResolvedMembers::default()
        });
        self.links.set_type_members(
            self.speculation_depth,
            ty,
            crate::links::LinkSlot::Resolved(id),
        );
        Ok(id)
    }

    fn is_global_function_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        // globalFunctionType is lazily bound; comparing against an
        // unbound global must not force a lookup that reports 2318 —
        // probe the memo path only when the type is a plain interface.
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::CLASS_OR_INTERFACE)
        {
            return Ok(false);
        }
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return Ok(false);
        };
        Ok(self.binder.symbol(symbol).escaped_name == "Function"
            && self.symbol_flags(symbol).intersects(SymbolFlags::INTERFACE)
            && self.global_function_type()? == ty)
    }

    /// tsc-port: resolveIntersectionTypeMembers @6.0.3
    /// tsc-hash: 9f5e810872d62327c70570c799ea9204577d8336dea93c08f47a50bd1b432d91
    /// tsc-span: _tsc.js:58256-58285
    pub(crate) fn resolve_intersection_type_members(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<crate::state::MembersId> {
        let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
            unreachable!("intersection flag implies intersection data");
        };
        let mixin_flags = self.find_mixins(&types)?;
        let mixin_count = mixin_flags.iter().filter(|&&b| b).count();
        let mut call_signatures: Vec<SignatureId> = Vec::new();
        let mut construct_signatures: Vec<SignatureId> = Vec::new();
        let mut index_infos: Vec<IndexInfo> = Vec::new();
        for (i, &t) in types.iter().enumerate() {
            if !mixin_flags[i] {
                let mut signatures = self.get_signatures_of_type(t, SignatureKind::Construct)?;
                if !signatures.is_empty() && mixin_count > 0 {
                    let mut mapped = Vec::with_capacity(signatures.len());
                    for &s in &signatures {
                        let clone = self.clone_signature(s);
                        let return_type = self.get_return_type_of_signature(s)?;
                        let mixed =
                            self.include_mixin_type(return_type, &types, &mixin_flags, i)?;
                        self.signature_mut(clone).resolved_return_type =
                            crate::links::LinkSlot::Resolved(mixed);
                        mapped.push(clone);
                    }
                    signatures = mapped;
                }
                self.append_signatures(&mut construct_signatures, &signatures)?;
            }
            let calls = self.get_signatures_of_type(t, SignatureKind::Call)?;
            self.append_signatures(&mut call_signatures, &calls)?;
            for info in self.get_index_infos_of_type(t)? {
                self.append_index_info(&mut index_infos, info, /*union*/ false)?;
            }
        }
        let id = self.alloc_members(crate::state::ResolvedMembers {
            call_signatures,
            construct_signatures,
            index_infos,
            ..crate::state::ResolvedMembers::default()
        });
        self.links.set_type_members(
            self.speculation_depth,
            ty,
            crate::links::LinkSlot::Resolved(id),
        );
        Ok(id)
    }

    /// tsc-port: findMixins @6.0.3
    /// tsc-hash: 842e131d8b6c0647002c0dc233b86d0e4caf1ccaa10126ad25c0881f10fab8a4
    /// tsc-span: _tsc.js:58233-58244
    pub(crate) fn find_mixins(&mut self, types: &[TypeId]) -> CheckResult2<Vec<bool>> {
        let mut constructor_type_count = 0usize;
        for &t in types {
            if !self
                .get_signatures_of_type(t, SignatureKind::Construct)?
                .is_empty()
            {
                constructor_type_count += 1;
            }
        }
        let mut mixin_flags = Vec::with_capacity(types.len());
        for &t in types {
            mixin_flags.push(self.is_mixin_constructor_type(t)?);
        }
        let mixin_true_count = mixin_flags.iter().filter(|&&b| b).count();
        if constructor_type_count > 0 && constructor_type_count == mixin_true_count {
            let first = mixin_flags
                .iter()
                .position(|&b| b)
                .expect("count > 0 implies a true entry");
            mixin_flags[first] = false;
        }
        Ok(mixin_flags)
    }

    /// tsc-port: isMixinConstructorType @6.0.3
    /// tsc-hash: 9c41ff6b53e42fcee67b664ff474e516746ab4eff3dfb1ab3a9eb8175a1f7fbf
    /// tsc-span: _tsc.js:57111-57121
    pub(crate) fn is_mixin_constructor_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let signatures = self.get_signatures_of_type(ty, SignatureKind::Construct)?;
        if signatures.len() != 1 {
            return Ok(false);
        }
        let s = self.signature_of(signatures[0]).clone();
        if s.type_parameters.is_none()
            && s.parameters.len() == 1
            && s.flags
                .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            let param_type = self.get_type_of_parameter(s.parameters[0])?;
            if self.tables.flags_of(param_type).intersects(TypeFlags::ANY) {
                return Ok(true);
            }
            let element = self.get_element_type_of_array_type(param_type)?;
            return Ok(element == Some(self.tables.intrinsics.any));
        }
        Ok(false)
    }

    /// tsc getElementTypeOfArrayType (67677-67679).
    pub(crate) fn get_element_type_of_array_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.is_array_type(ty)? {
            return Ok(Some(self.get_type_arguments(ty)?[0]));
        }
        Ok(None)
    }

    /// tsc-port: includeMixinType @6.0.3
    /// tsc-hash: ea32a308425c179ed929ea3ce58409f3d0b31991146c76fe6ac9539262ab93de
    /// tsc-span: _tsc.js:58245-58255
    fn include_mixin_type(
        &mut self,
        ty: TypeId,
        types: &[TypeId],
        mixin_flags: &[bool],
        index: usize,
    ) -> CheckResult2<TypeId> {
        let mut mixed_types = Vec::new();
        for (i, &t) in types.iter().enumerate() {
            if i == index {
                mixed_types.push(ty);
            } else if mixin_flags[i] {
                let construct = self.get_signatures_of_type(t, SignatureKind::Construct)?[0];
                mixed_types.push(self.get_return_type_of_signature(construct)?);
            }
        }
        self.get_intersection_type(&mixed_types, tsrs2_types::IntersectionFlags::NONE)
    }

    /// tsc-port: appendSignatures @6.0.3
    /// tsc-hash: ce220df79982e0892294026f9da6e75bc6dce320d3f578b6acebce0faf4830da
    /// tsc-span: _tsc.js:58286-58303
    fn append_signatures(
        &mut self,
        signatures: &mut Vec<SignatureId>,
        new_signatures: &[SignatureId],
    ) -> CheckResult2<()> {
        'outer: for &sig in new_signatures {
            if !signatures.is_empty() {
                for &s in signatures.iter() {
                    if self.compare_signatures_identical_at(s, sig, false, false, false)? {
                        continue 'outer;
                    }
                }
            }
            signatures.push(sig);
        }
        Ok(())
    }

    /// tsc-port: appendIndexInfo @6.0.3
    /// tsc-hash: 261bc2d3946da7a99f772ef4313e7e6aaaf97a8a814919c7272cc52947c0a5ef
    /// tsc-span: _tsc.js:58304-58317
    fn append_index_info(
        &mut self,
        index_infos: &mut Vec<IndexInfo>,
        new_info: IndexInfo,
        union: bool,
    ) -> CheckResult2<()> {
        for info in index_infos.iter_mut() {
            if info.key_type == new_info.key_type {
                let value = if union {
                    self.get_union_type_ex(
                        &[info.value_type, new_info.value_type],
                        UnionReduction::Literal,
                    )?
                } else {
                    self.get_intersection_type(
                        &[info.value_type, new_info.value_type],
                        tsrs2_types::IntersectionFlags::NONE,
                    )?
                };
                let is_readonly = if union {
                    info.is_readonly || new_info.is_readonly
                } else {
                    info.is_readonly && new_info.is_readonly
                };
                *info = IndexInfo {
                    key_type: info.key_type,
                    value_type: value,
                    is_readonly,
                    declaration: None,
                    components: None,
                    is_enum_number_index_info: false,
                };
                return Ok(());
            }
        }
        index_infos.push(new_info);
        Ok(())
    }

    // ---- signature access ----

    /// tsc-port: getSignaturesOfStructuredType @6.0.3
    /// tsc-hash: b8cacc74c4e68b268f4eab638a12931ba584c0aaa9abcf1425a93d75ad1989c4
    /// tsc-span: _tsc.js:59390-59396
    ///
    /// getSignaturesOfType's union call-signature fallback (59397+)
    /// needs union signature synthesis — M4; union sources with call
    /// signatures report Unsupported.
    pub fn get_signatures_of_type(
        &mut self,
        ty: TypeId,
        kind: SignatureKind,
    ) -> CheckResult2<Vec<SignatureId>> {
        let reduced = self.get_reduced_apparent_type(ty)?;
        // getSignaturesOfStructuredType: unions and intersections
        // resolve through their member synthesis (5.3d) like objects.
        if !self
            .tables
            .flags_of(reduced)
            .intersects(TypeFlags::STRUCTURED_TYPE)
        {
            return Ok(Vec::new());
        }
        let members = self.resolve_structured_type_members(reduced)?;
        let resolved = self.members_of(members);
        Ok(match kind {
            SignatureKind::Call => resolved.call_signatures.clone(),
            SignatureKind::Construct => resolved.construct_signatures.clone(),
        })
    }

    /// tsc-port: getThisTypeOfSignature @6.0.3
    /// tsc-hash: a92299092018fe8dd15442b0940d370735dd479570f377bcaddda83e871beab6
    /// tsc-span: _tsc.js:59760-59764
    pub fn get_this_type_of_signature(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(this_parameter) = self.signature_of(signature).this_parameter else {
            return Ok(None);
        };
        Ok(Some(self.get_type_of_symbol(this_parameter)?))
    }

    /// tsc-port: isTopSignature @6.0.3
    /// tsc-hash: 2deef363c847c3d8dd816c49efdf631572c8bd5cd017402e80809d986d917197
    /// tsc-span: _tsc.js:64479-64486
    ///
    pub fn is_top_signature(&mut self, signature: SignatureId) -> CheckResult2<bool> {
        let signature_data = self.signature_of(signature).clone();
        let this_is_any = match signature_data.this_parameter {
            None => true,
            Some(this_parameter) => {
                let this_type = self.get_type_of_parameter(this_parameter)?;
                self.tables.flags_of(this_type).intersects(TypeFlags::ANY)
            }
        };
        if signature_data.type_parameters.is_none()
            && this_is_any
            && signature_data.parameters.len() == 1
            && signature_data
                .flags
                .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            let parameter_type = self.get_type_of_parameter(signature_data.parameters[0])?;
            let rest_type = if self.is_array_type(parameter_type)? {
                self.get_type_arguments(parameter_type)?[0]
            } else {
                parameter_type
            };
            let return_type = self.get_return_type_of_signature(signature)?;
            return Ok(self
                .tables
                .flags_of(rest_type)
                .intersects(TypeFlags::ANY | TypeFlags::NEVER)
                && self
                    .tables
                    .flags_of(return_type)
                    .intersects(TypeFlags::ANY_OR_UNKNOWN));
        }
        Ok(false)
    }

    /// tsc-port: isArrayType @6.0.3
    /// tsc-hash: 880f484023ae500fd17675daebbc00e72462411283bf49135001973ca042cf9f
    /// tsc-span: _tsc.js:67665-67667
    pub(crate) fn is_array_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(false);
        }
        let target = self.tables.reference_target(ty);
        Ok(target == self.global_array_type()? || target == self.global_readonly_array_type()?)
    }

    /// tsc-port: isReadonlyArrayType @6.0.3
    /// tsc-hash: c05b7ec4ec075d5ce9de1fb736daea7e32901f0c533d10374b40b053b5f8e1a7
    /// tsc-span: _tsc.js:67668-67670
    pub(crate) fn is_readonly_array_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(false);
        }
        let target = self.tables.reference_target(ty);
        Ok(target == self.global_readonly_array_type()?)
    }

    /// tsc-port: isMutableArrayOrTuple @6.0.3
    /// tsc-hash: 1c01574a02619fe0324ab8bc6ea0624ded960d83d92345f690176ad88468bd76
    /// tsc-span: _tsc.js:67674-67676
    pub(crate) fn is_mutable_array_or_tuple(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self.is_array_type(ty)? && !self.is_readonly_array_type(ty)? {
            return Ok(true);
        }
        if self.tables.is_tuple_type(ty) {
            let target = self.tables.reference_target(ty);
            if let TypeData::TupleTarget(data) = &self.tables.type_of(target).data {
                return Ok(!data.readonly);
            }
        }
        Ok(false)
    }

    /// tsc-port: isSingleElementGenericTupleType @6.0.3
    /// tsc-hash: bf39e70f94342a30b638fb1d87052e8833f356e4fc252650dec22dbd34f9e960
    /// tsc-span: _tsc.js:67797-67799
    pub(crate) fn is_single_element_generic_tuple_type(&self, ty: TypeId) -> bool {
        if !self.is_generic_tuple_type(ty) {
            return false;
        }
        let target = self.tables.reference_target(ty);
        match &self.tables.type_of(target).data {
            TypeData::TupleTarget(data) => data.element_flags.len() == 1,
            _ => false,
        }
    }

    /// tsc-port: getIndexTypeOfType @6.0.3
    /// tsc-hash: dda9f758a99a41508806273859c03a0821806a963e7f12bc4ffae06e24f51af3
    /// tsc-span: _tsc.js:59469-59472
    pub(crate) fn get_index_type_of_type(
        &mut self,
        ty: TypeId,
        key_type: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        Ok(self
            .get_index_info_of_type(ty, key_type)?
            .map(|info| info.value_type))
    }

    /// tsc-port: isEmptyLiteralType @6.0.3
    /// tsc-hash: 458c64127035db67ab875dccd517872b294a42d0b24239a86045b48e14372723
    /// tsc-span: _tsc.js:67715-67717
    ///
    /// Both sentinels come from empty-array-literal widening (M6
    /// expression checking), so annotation-built types never match.
    pub(crate) fn is_empty_literal_type(&self, ty: TypeId) -> bool {
        if self.tables.strict_null_checks {
            ty == self.tables.intrinsics.implicit_never
        } else {
            ty == self.tables.intrinsics.undefined_widening
        }
    }

    /// tsc-port: isEmptyArrayLiteralType @6.0.3
    /// tsc-hash: 36ea5d535a8ac1fbf15562bd839a466e062940623a642f6ffee087f07b521744
    /// tsc-span: _tsc.js:67718-67721
    pub(crate) fn is_empty_array_literal_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let element = self.get_element_type_of_array_type(ty)?;
        Ok(element.is_some_and(|element| self.is_empty_literal_type(element)))
    }

    /// The rest-parameter tuple target, when the last parameter's type
    /// is a tuple reference.
    fn rest_tuple_target_data(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<(TypeId, tsrs2_types::TupleTargetData)>> {
        let signature_data = self.signature_of(signature);
        if !signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Ok(None);
        }
        let rest_parameter = *signature_data
            .parameters
            .last()
            .expect("rest-parameter signatures have parameters");
        let rest_type = self.get_type_of_symbol(rest_parameter)?;
        if !self.tables.is_tuple_type(rest_type) {
            return Ok(None);
        }
        let target = self.tables.reference_target(rest_type);
        let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
            unreachable!("tuple type targets a tuple target");
        };
        Ok(Some((rest_type, data)))
    }

    // ---- arity helpers (78233-78341) ----

    /// tsc-port: getParameterCount @6.0.3
    /// tsc-hash: 88e24efd3edb09e7c4c597f52541cb2cc8bb745ffdfb9ddd606c00c0e7ecb9b7
    /// tsc-span: _tsc.js:78277-78286
    pub fn get_parameter_count(&mut self, signature: SignatureId) -> CheckResult2<usize> {
        let length = self.signature_of(signature).parameters.len();
        if let Some((_, data)) = self.rest_tuple_target_data(signature)? {
            return Ok(length + data.fixed_length
                - usize::from(!data.combined_flags.intersects(ElementFlags::VARIABLE)));
        }
        Ok(length)
    }

    /// tsc-port: getMinArgumentCount @6.0.3
    /// tsc-hash: 7e615bfc72d73516124a8fd89a208b2ca2036519bbaf5b7cad73d2010b1ef3b8
    /// tsc-span: _tsc.js:78287-78321
    ///
    /// The (StrongArityForUntypedJS|VoidIsNonOptional) flags parameter
    /// is elided — every ported caller passes none, so the
    /// resolvedMinArgumentCount cache reduces to recomputation; the
    /// void-trimming loop lowers the syntactic count when trailing
    /// parameters accept void.
    pub fn get_min_argument_count(&mut self, signature: SignatureId) -> CheckResult2<usize> {
        let mut computed: Option<usize> = None;
        if let Some((_, data)) = self.rest_tuple_target_data(signature)? {
            let first_optional_index = data
                .element_flags
                .iter()
                .position(|flags| !flags.intersects(ElementFlags::REQUIRED));
            let required_count = first_optional_index.unwrap_or(data.fixed_length);
            if required_count > 0 {
                computed = Some(self.signature_of(signature).parameters.len() - 1 + required_count);
            }
        }
        let signature_data = self.signature_of(signature);
        let mut min_argument_count = computed.unwrap_or(signature_data.min_argument_count as usize);
        let mut i = min_argument_count;
        while i > 0 {
            i -= 1;
            let ty = self.get_type_at_position(signature, i)?;
            let filtered = self.tables.filter_type(ty, |tables, t| {
                tables.flags_of(t).intersects(TypeFlags::VOID)
            });
            if !self.tables.flags_of(filtered).intersects(TypeFlags::NEVER) {
                min_argument_count = i;
            } else {
                break;
            }
        }
        Ok(min_argument_count)
    }

    /// tsc-port: getMinArgumentCount @6.0.3 (VoidIsNonOptional face)
    /// tsc-hash: 7e615bfc72d73516124a8fd89a208b2ca2036519bbaf5b7cad73d2010b1ef3b8
    /// tsc-span: _tsc.js:78287-78321
    ///
    /// isOptionalParameter (59509-59527) reads the count under
    /// (StrongArityForUntypedJS | VoidIsNonOptional): the void-
    /// trimming loop is skipped (the 78313 early return) and the
    /// untyped-JS zero (78309-78311) is bypassed — leaving exactly
    /// the tuple-rest required count or the declared integer.
    pub(crate) fn min_argument_count_without_void_trimming(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<usize> {
        if let Some((_, data)) = self.rest_tuple_target_data(signature)? {
            let first_optional_index = data
                .element_flags
                .iter()
                .position(|flags| !flags.intersects(ElementFlags::REQUIRED));
            let required_count = first_optional_index.unwrap_or(data.fixed_length);
            if required_count > 0 {
                return Ok(self.signature_of(signature).parameters.len() - 1 + required_count);
            }
        }
        Ok(self.signature_of(signature).min_argument_count as usize)
    }

    /// tsc-port: hasEffectiveRestParameter @6.0.3
    /// tsc-hash: 4545af73ef96a3c83fa4e089d6a10737e822c04c947fb002fbfa5e24fe93959f
    /// tsc-span: _tsc.js:78322-78328
    pub fn has_effective_rest_parameter(&mut self, signature: SignatureId) -> CheckResult2<bool> {
        if !self
            .signature_of(signature)
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Ok(false);
        }
        match self.rest_tuple_target_data(signature)? {
            None => Ok(true),
            Some((_, data)) => Ok(data.combined_flags.intersects(ElementFlags::VARIABLE)),
        }
    }

    /// tsc-port: getEffectiveRestType @6.0.3
    /// tsc-hash: dc61427d195fae1b21b9d693d06a386da7077ed2559c33e243ae2b4cd472f37a
    /// tsc-span: _tsc.js:78329-78341
    /// tsc-port: getNonArrayRestType @6.0.3
    /// tsc-hash: 9b7b44144d29b9ab97451facaeed6458f2e907a6b8b5654bcb117b89940886a7
    /// tsc-span: _tsc.js:78341-78344
    pub(crate) fn get_non_array_rest_type(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(rest_type) = self.get_effective_rest_type(signature)? else {
            return Ok(None);
        };
        if self.is_array_type(rest_type)?
            || self.tables.flags_of(rest_type).intersects(TypeFlags::ANY)
        {
            return Ok(None);
        }
        Ok(Some(rest_type))
    }

    /// tsc-port: getRestTypeAtPosition @6.0.3
    /// tsc-hash: a5d5888b5bc281ebdd937e74c846f59b9dff97d96920cadc248e41b69bec1c8f
    /// tsc-span: _tsc.js:78250-78271
    pub(crate) fn get_rest_type_at_position(
        &mut self,
        source: SignatureId,
        pos: usize,
        readonly: bool,
    ) -> CheckResult2<TypeId> {
        let parameter_count = self.get_parameter_count(source)?;
        let min_argument_count = self.get_min_argument_count(source)?;
        let rest_type = self.get_effective_rest_type(source)?;
        if let Some(rest_type) = rest_type {
            if pos + 1 >= parameter_count {
                return if pos + 1 == parameter_count {
                    Ok(rest_type)
                } else {
                    let indexed = self.get_indexed_access_type(
                        rest_type,
                        self.tables.intrinsics.number,
                        tsrs2_types::AccessFlags::NONE,
                        /*access_node*/ None,
                        /*alias_symbol*/ None,
                        /*alias_type_arguments*/ None,
                    )?;
                    self.create_array_type(indexed, /*readonly*/ false)
                };
            }
        }
        let mut types = Vec::new();
        let mut flags = Vec::new();
        let mut names: Vec<Option<u32>> = Vec::new();
        for i in pos..parameter_count {
            match rest_type {
                Some(rest_type) if i + 1 >= parameter_count => {
                    types.push(rest_type);
                    flags.push(ElementFlags::VARIADIC);
                }
                _ => {
                    types.push(self.get_type_at_position(source, i)?);
                    flags.push(if i < min_argument_count {
                        ElementFlags::REQUIRED
                    } else {
                        ElementFlags::OPTIONAL
                    });
                }
            }
            names.push(self.get_nameable_declaration_at_position(source, i)?);
        }
        self.create_tuple_type_forced(&types, Some(&flags), readonly, Some(&names))
    }

    /// tsc-port: getRestOrAnyTypeAtPosition @6.0.3
    /// tsc-hash: 6b4afd149f3e9a4b43324695fef12c0eef9f0cc98aae4159ab7b9889317b5512
    /// tsc-span: _tsc.js:78272-78275
    pub(crate) fn get_rest_or_any_type_at_position(
        &mut self,
        source: SignatureId,
        pos: usize,
    ) -> CheckResult2<TypeId> {
        let rest_type = self.get_rest_type_at_position(source, pos, /*readonly*/ false)?;
        let element = self.get_element_type_of_array_type(rest_type)?;
        Ok(match element {
            Some(element) if self.tables.flags_of(element).intersects(TypeFlags::ANY) => {
                self.tables.intrinsics.any
            }
            _ => rest_type,
        })
    }

    /// tsc-port: getNameableDeclarationAtPosition @6.0.3
    /// tsc-hash: 7d2fc2e1055b65d5f61afe23ce96fa7aa716b28b4a336a0e23c7ebb3e60e3e28
    /// tsc-span: _tsc.js:78218-78233
    fn get_nameable_declaration_at_position(
        &mut self,
        signature: SignatureId,
        pos: usize,
    ) -> CheckResult2<Option<u32>> {
        let data = self.signature_of(signature);
        let has_rest = data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER);
        let parameters = data.parameters.clone();
        let param_count = parameters.len() - usize::from(has_rest);
        if pos < param_count {
            let declaration = self.binder.symbol(parameters[pos]).value_declaration;
            return Ok(declaration
                .filter(|&declaration| self.is_valid_declaration_for_tuple_label(declaration))
                .map(|declaration| declaration.0));
        }
        // tsc falls back to unknownSymbol when the rest slot is out of
        // range — no value declaration either way.
        let Some(&rest_parameter) = parameters.get(param_count) else {
            return Ok(None);
        };
        let rest_type = self.get_type_of_symbol(rest_parameter)?;
        if self.tables.is_tuple_type(rest_type) {
            let target = self.tables.reference_target(rest_type);
            if let TypeData::TupleTarget(data) = &self.tables.type_of(target).data {
                let index = pos - param_count;
                return Ok(data
                    .labeled_element_declarations
                    .as_ref()
                    .and_then(|names| names.get(index).copied())
                    .flatten());
            }
        }
        let declaration = self.binder.symbol(rest_parameter).value_declaration;
        Ok(declaration
            .filter(|&declaration| self.is_valid_declaration_for_tuple_label(declaration))
            .map(|declaration| declaration.0))
    }

    /// tsc-port: isValidDeclarationForTupleLabel @6.0.3
    /// tsc-hash: f6a5a8962e35ef3a5f93de0263059acbb1f767864d8efd2ea7da9d48d8a61dae
    /// tsc-span: _tsc.js:78215-78217
    fn is_valid_declaration_for_tuple_label(&self, declaration: NodeId) -> bool {
        if self.kind_of(declaration) == SyntaxKind::NamedTupleMember {
            return true;
        }
        matches!(
            self.data_of(declaration),
            NodeData::Parameter(data)
                if data.name.is_some_and(|name| self.kind_of(name) == SyntaxKind::Identifier)
        )
    }

    pub fn get_effective_rest_type(
        &mut self,
        signature: SignatureId,
    ) -> CheckResult2<Option<TypeId>> {
        let signature_data = self.signature_of(signature);
        if !signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Ok(None);
        }
        let rest_parameter = *signature_data
            .parameters
            .last()
            .expect("rest-parameter signatures have parameters");
        let rest_type = self.get_type_of_symbol(rest_parameter)?;
        if !self.tables.is_tuple_type(rest_type) {
            return Ok(Some(
                if self.tables.flags_of(rest_type).intersects(TypeFlags::ANY) {
                    self.any_array_type()?
                } else {
                    rest_type
                },
            ));
        }
        let target = self.tables.reference_target(rest_type);
        let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
            unreachable!("tuple type targets a tuple target");
        };
        if data.combined_flags.intersects(ElementFlags::VARIABLE) {
            return Ok(Some(self.slice_tuple_type(
                rest_type,
                data.fixed_length,
                0,
            )?));
        }
        Ok(None)
    }

    /// tsc-port: sliceTupleType @6.0.3
    /// tsc-hash: f3e74aeb0c72e2b0ddb886f625669eeb36d0c954405b64b394783b09cf1519b2
    /// tsc-span: _tsc.js:61288-61299
    pub(crate) fn slice_tuple_type(
        &mut self,
        ty: TypeId,
        index: usize,
        end_skip_count: usize,
    ) -> CheckResult2<TypeId> {
        let target = self.tables.reference_target(ty);
        let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
            unreachable!("tuple type targets a tuple target");
        };
        // 61290 slices with JS Array.prototype.slice semantics: an end
        // before the start yields the empty slice (reachable from
        // inferFromObjectTypes' middle-arm bounds when the source
        // tuple is shorter than the target's fixed parts — pinned),
        // and a NEGATIVE end argument (skip > arity) counts from the
        // END — `max(len - (skip - len), 0)`. The from-end window
        // became REACHABLE at 7.4: the both-variadic impliedArity arm
        // (69114) passes endLength + sourceArity - impliedArity, which
        // exceeds the source arity whenever impliedArity < endLength
        // (7.2d re-audit item, resolved — pinned below).
        let len = data.type_parameters.len();
        let end_index = if end_skip_count <= len {
            len - end_skip_count
        } else {
            len.saturating_sub(end_skip_count - len)
        }
        .max(index);
        if index > data.fixed_length {
            let rest_array = self.get_rest_array_type_of_tuple_type(ty)?;
            return match rest_array {
                Some(array) => Ok(array),
                None => Ok(self.tables.get_tuple_target_type(
                    TupleTargetFlags::new(&[]).expect("empty tuple is not single-rest"),
                    false,
                    None,
                )),
            };
        }
        let arguments = self.get_type_arguments(ty)?;
        let labels = data
            .labeled_element_declarations
            .as_ref()
            .map(|declarations| declarations[index..end_index].to_vec());
        self.create_tuple_type_forced(
            &arguments[index..end_index],
            Some(&data.element_flags[index..end_index]),
            /*readonly*/ false,
            labels.as_deref(),
        )
    }

    /// tsc-port: getRestArrayTypeOfTupleType @6.0.3
    /// tsc-hash: 0e78932a17539cec04a5456647111329efdaf8c6f3cfd7ed49ccb870d6431d6c
    /// tsc-span: _tsc.js:67816-67819
    pub(crate) fn get_rest_array_type_of_tuple_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let rest_type = self.get_rest_type_of_tuple_type(ty)?;
        match rest_type {
            Some(rest_type) => Ok(Some(self.create_array_type(rest_type, false)?)),
            None => Ok(None),
        }
    }

    /// createTupleType through the checker's variadic pre-force wrapper
    /// (tables cannot force deferred element arguments).
    pub(crate) fn create_tuple_type_forced(
        &mut self,
        element_types: &[TypeId],
        element_flags: Option<&[ElementFlags]>,
        readonly: bool,
        named_member_declarations: Option<&[Option<u32>]>,
    ) -> CheckResult2<TypeId> {
        let default_flags;
        let flags = match element_flags {
            Some(flags) => flags,
            None => {
                default_flags = vec![ElementFlags::REQUIRED; element_types.len()];
                &default_flags
            }
        };
        // getTupleTargetType 61146-61148: `[...E[]]` IS (readonly)
        // E[] — the checker owns the global array targets, so the
        // collapse lives here and TupleTargetFlags excludes that shape
        // from tables target construction (L-TWIN).
        if flags.len() == 1 && flags[0].intersects(ElementFlags::REST) {
            return self.create_array_type(element_types[0], readonly);
        }
        let flags = TupleTargetFlags::new(flags)
            .expect("single-rest tuples collapse before tuple-target construction");
        let target = self
            .tables
            .get_tuple_target_type(flags, readonly, named_member_declarations);
        if element_types.is_empty() {
            return Ok(target);
        }
        self.create_normalized_type_reference_forced(target, element_types)
    }

    /// tsc-port: getTypeAtPosition @6.0.3
    /// tsc-hash: 94dadfd82637724c09a0e07b9a85fc66909feab9a9b13a9ccf83267afb8a6bdd
    /// tsc-span: _tsc.js:78233-78235
    pub fn get_type_at_position(
        &mut self,
        signature: SignatureId,
        pos: usize,
    ) -> CheckResult2<TypeId> {
        Ok(self
            .try_get_type_at_position(signature, pos)?
            .unwrap_or(self.tables.intrinsics.any))
    }

    /// tsc-port: tryGetTypeAtPosition @6.0.3
    /// tsc-hash: 6a57eaee3ac44538561a3064d59ce10681d1142b37a15867827a1c6f6f6b666b
    /// tsc-span: _tsc.js:78236-78249
    pub fn try_get_type_at_position(
        &mut self,
        signature: SignatureId,
        pos: usize,
    ) -> CheckResult2<Option<TypeId>> {
        let signature_data = self.signature_of(signature);
        let parameters = signature_data.parameters.clone();
        let has_rest = signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER);
        let parameter_count = parameters.len() - usize::from(has_rest);
        if pos < parameter_count {
            return Ok(Some(self.get_type_of_parameter(parameters[pos])?));
        }
        if has_rest {
            let rest_type = self.get_type_of_symbol(parameters[parameter_count])?;
            let index = pos - parameter_count;
            let tuple_gate = if self.tables.is_tuple_type(rest_type) {
                let target = self.tables.reference_target(rest_type);
                match &self.tables.type_of(target).data {
                    TypeData::TupleTarget(data) => {
                        data.combined_flags.intersects(ElementFlags::VARIABLE)
                            || index < data.fixed_length
                    }
                    _ => unreachable!("tuple type targets a tuple target"),
                }
            } else {
                true
            };
            if tuple_gate {
                let literal = self.tables.get_number_literal_type(index as f64);
                return Ok(Some(self.get_indexed_access_type(
                    rest_type,
                    literal,
                    tsrs2_types::AccessFlags::NONE,
                    None,
                    None,
                    None,
                )?));
            }
        }
        Ok(None)
    }

    // ---- index info access ----

    /// tsc-port: getIndexInfosOfStructuredType @6.0.3
    /// tsc-hash: d73837152b89c733efde4142f174e9bec5fcb195dd267a7528ea2b9c6e7f9967
    /// tsc-span: _tsc.js:59456-59462
    ///
    /// Union/intersection index infos need member resolution for those
    /// kinds — M4 rows.
    pub fn get_index_infos_of_type(&mut self, ty: TypeId) -> CheckResult2<Vec<IndexInfo>> {
        let reduced = self.get_reduced_apparent_type(ty)?;
        // getIndexInfosOfStructuredType: unions and intersections
        // resolve through their member synthesis (5.3d) like objects.
        if !self
            .tables
            .flags_of(reduced)
            .intersects(TypeFlags::STRUCTURED_TYPE)
        {
            return Ok(Vec::new());
        }
        let members = self.resolve_structured_type_members(reduced)?;
        Ok(self.members_of(members).index_infos.clone())
    }

    /// tsc-port: findApplicableIndexInfo @6.0.3
    /// tsc-hash: 042abae5b36c857308f5bca056c8e1fd60454ec4b252b73cc54651436e668126
    /// tsc-span: _tsc.js:59431-59452
    ///
    /// tsc-port: getApplicableIndexInfo @6.0.3
    /// tsc-hash: abfecc791837715d037edd04734e85e7d190acff2b5f7bad2eaec4ec8b32f6ef
    /// tsc-span: _tsc.js:59476-59478
    ///
    /// The multi-applicable intersection branch builds the combined
    /// info through getIntersectionType.
    pub fn get_applicable_index_info(
        &mut self,
        ty: TypeId,
        key_type: TypeId,
    ) -> CheckResult2<Option<IndexInfo>> {
        let index_infos = self.get_index_infos_of_type(ty)?;
        let string = self.tables.intrinsics.string;
        let mut string_index_info: Option<IndexInfo> = None;
        let mut applicable: Vec<IndexInfo> = Vec::new();
        for info in index_infos {
            if info.key_type == string {
                string_index_info = Some(info);
            } else if self.is_applicable_index_type(key_type, info.key_type)? {
                applicable.push(info);
            }
        }
        if applicable.len() > 1 {
            let types: Vec<TypeId> = applicable.iter().map(|info| info.value_type).collect();
            let value = self.get_intersection_type(&types, tsrs2_types::IntersectionFlags::NONE)?;
            let is_readonly = applicable.iter().all(|info| info.is_readonly);
            let declaration = applicable[0].declaration;
            return Ok(Some(IndexInfo {
                key_type: self.tables.intrinsics.unknown,
                value_type: value,
                is_readonly,
                declaration,
                components: None,
                is_enum_number_index_info: false,
            }));
        }
        if let Some(info) = applicable.into_iter().next() {
            return Ok(Some(info));
        }
        if let Some(info) = string_index_info {
            if self.is_applicable_index_type(key_type, string)? {
                return Ok(Some(info));
            }
        }
        Ok(None)
    }

    /// tsc-port: isApplicableIndexType @6.0.3
    /// tsc-hash: 6271e2054ec8d55802a40f0113ff6e66276e4a0b9fb9217f044060e9c499fa34
    /// tsc-span: _tsc.js:59453-59455
    pub fn is_applicable_index_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        if self.is_type_assignable_to(source, target)? {
            return Ok(true);
        }
        if target == self.tables.intrinsics.string
            && self.is_type_assignable_to(source, self.tables.intrinsics.number)?
        {
            return Ok(true);
        }
        if target == self.tables.intrinsics.number {
            // `${number}` applies to number keys (59454's
            // numericStringType face — numericStringLiteralTypes pins
            // `a[x]` with x: `${number}` clean).
            if source == self.tables.intrinsics.numeric_string {
                return Ok(true);
            }
            if let TypeData::Literal {
                value: tsrs2_types::LiteralValue::String(value),
            } = &self.tables.type_of(source).data
            {
                return Ok(is_numeric_literal_name_js(value));
            }
        }
        Ok(false)
    }

    /// The applicable index info's shape for union-property synthesis
    /// (getApplicableIndexInfoForName over the property name).
    pub(crate) fn get_applicable_index_info_for_name_info(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<IndexInfo>> {
        // getApplicableIndexInfoForName probes LATE-BOUND names
        // (isLateBoundName — the `__@` unique-symbol spellings) with
        // esSymbolType, everything else with the name's literal type.
        let name_type = if name.starts_with("__@") {
            self.tables.intrinsics.es_symbol
        } else if is_numeric_name(name) {
            let value: f64 = name
                .parse()
                .expect("is_numeric_name admits only f64-parsable digit strings");
            self.tables.get_number_literal_type(value)
        } else {
            self.tables.get_string_literal_type(name)
        };
        self.get_applicable_index_info(ty, name_type)
    }

    // ---- callback-parameter helpers ----

    /// tsc-port: isInstantiatedGenericParameter @6.0.3
    /// tsc-hash: 1fdd3193cc61fda77748c9bc598ef0b914c0841c0b1b52dd36dfa0d7e818b632
    /// tsc-span: _tsc.js:75871-75874
    ///
    /// A parameter position that was a generic type BEFORE
    /// instantiation (signature.target's type at pos) — the callback
    /// cell suppresses single-signature treatment for those (64549-
    /// 64550; missing pre-7.5d, which mis-routed instantiated
    /// same-shape methods through the callback recursion).
    pub(crate) fn is_instantiated_generic_parameter(
        &mut self,
        signature: SignatureId,
        pos: usize,
    ) -> CheckResult2<bool> {
        let Some(target) = self.signature_of(signature).target else {
            return Ok(false);
        };
        let Some(ty) = self.try_get_type_at_position(target, pos)? else {
            return Ok(false);
        };
        self.is_generic_type(ty)
    }

    /// getSingleCallSignature (75875-75877): the one-line delegation
    /// to `getSingleSignature(type, Call, /*allowMembers*/ false)`,
    /// exactly as tsc cuts it.
    pub fn get_single_call_signature(&mut self, ty: TypeId) -> CheckResult2<Option<SignatureId>> {
        self.get_single_signature(ty, SignatureKind::Call, false)
    }

    /// tsc-port: getSingleCallOrConstructSignature @6.0.3
    /// tsc-hash: d7af02d36c16f7a647b4f19bc1eba0d59125afe42cfc70f7e290753a11ea2bf8
    /// tsc-span: _tsc.js:75883-75895
    pub(crate) fn get_single_call_or_construct_signature(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<SignatureId>> {
        if let Some(call) = self.get_single_signature(ty, SignatureKind::Call, false)? {
            return Ok(Some(call));
        }
        self.get_single_signature(ty, SignatureKind::Construct, false)
    }

    /// tsc-port: getSingleSignature @6.0.3
    /// tsc-hash: df7eb7f955e102594820d38ca174aad010b07e446f61fd554b44a0ae8afabf68
    /// tsc-span: _tsc.js:75896-75909
    pub(crate) fn get_single_signature(
        &mut self,
        ty: TypeId,
        kind: SignatureKind,
        allow_members: bool,
    ) -> CheckResult2<Option<SignatureId>> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(None);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        if allow_members || resolved.properties.is_empty() && resolved.index_infos.is_empty() {
            if kind == SignatureKind::Call
                && resolved.call_signatures.len() == 1
                && resolved.construct_signatures.is_empty()
            {
                return Ok(Some(resolved.call_signatures[0]));
            }
            if kind == SignatureKind::Construct
                && resolved.construct_signatures.len() == 1
                && resolved.call_signatures.is_empty()
            {
                return Ok(Some(resolved.construct_signatures[0]));
            }
        }
        Ok(None)
    }

    /// getNonNullableType's M3 slice for the callback gate: strip
    /// nullable constituents (the full type-facts machinery is M5).
    pub fn remove_nullable_for_callback_gate(&mut self, ty: TypeId) -> TypeId {
        if !self.tables.strict_null_checks {
            return ty;
        }
        self.tables.filter_type(ty, |tables, t| {
            !tables.flags_of(t).intersects(TypeFlags::NULLABLE)
        })
    }

    /// getTypeFacts(type, IsUndefinedOrNull) equality slice for the
    /// callback gate: whether the type includes undefined / null.
    pub fn undefined_null_facts(&self, ty: TypeId) -> (bool, bool) {
        let mut has_undefined = false;
        let mut has_null = false;
        let mut visit = |flags: TypeFlags| {
            has_undefined |= flags.intersects(TypeFlags::UNDEFINED);
            has_null |= flags.intersects(TypeFlags::NULL);
        };
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            if let TypeData::Union { types, .. } = &self.tables.type_of(ty).data {
                for &t in types.iter() {
                    visit(self.tables.flags_of(t));
                }
            }
        } else {
            visit(self.tables.flags_of(ty));
        }
        (has_undefined, has_null)
    }

    // ---- template literal matching (68515-68636) ----

    /// tsc-port: templateLiteralTypesDefinitelyUnrelated @6.0.3
    /// tsc-hash: 25d55d7febaa6846386f9d500ae59a2416a3aa80b8f7911034ea50f1a66a21c6
    /// tsc-span: _tsc.js:68515-68523
    pub fn template_literal_types_definitely_unrelated(
        &self,
        source: TypeId,
        target: TypeId,
    ) -> bool {
        // JS strings index by UTF-16 code unit — byte slicing panics
        // on multi-byte chars (the review's `é` pin) and would compare
        // different prefixes anyway.
        let (source_texts, _) = self.template_parts_of(source);
        let (target_texts, _) = self.template_parts_of(target);
        let source_start = source_texts[0].units();
        let target_start = target_texts[0].units();
        let source_end = source_texts[source_texts.len() - 1].units();
        let target_end = target_texts[target_texts.len() - 1].units();
        let start_len = source_start.len().min(target_start.len());
        let end_len = source_end.len().min(target_end.len());
        source_start[..start_len] != target_start[..start_len]
            || source_end[source_end.len() - end_len..] != target_end[target_end.len() - end_len..]
    }

    fn template_parts_of(&self, ty: TypeId) -> (Vec<TemplateText>, Vec<TypeId>) {
        match &self.tables.type_of(ty).data {
            TypeData::TemplateLiteral { texts, types } => (texts.to_vec(), types.to_vec()),
            _ => unreachable!("template flag implies template data"),
        }
    }

    /// tsc-port: isValidNumberString @6.0.3
    /// tsc-hash: 5cbe83a72d3b47092e151525fda47b46e119b9ca7bfeecc73fa877eb2e451e69
    /// tsc-span: _tsc.js:68524-68528
    ///
    /// The JS `+s` coercion slice for annotation-reachable strings:
    /// whitespace-trimmed decimal/exponent/hex forms; roundTripOnly
    /// compares against JS number formatting.
    pub(crate) fn is_valid_number_string(&self, s: &str, round_trip_only: bool) -> bool {
        if s.is_empty() {
            return false;
        }
        let Some(n) = js_string_to_number(s) else {
            return false;
        };
        if !n.is_finite() {
            return false;
        }
        !round_trip_only || js_number_to_string(n) == s
    }

    /// tsc-port: isValidBigIntString @6.0.3
    /// tsc-hash: 976d424ef636dd348576f49fa283c5a8b92988960b470c6038e4cc813de41f7e
    /// tsc-span: _tsc.js:18973-18989
    ///
    /// The scan half (probe scanner over `s + "n"`, minus handling,
    /// whole-input + no-separator gates) lives in the syntax crate as
    /// `scan_big_int_string`; this side owns the roundTripOnly
    /// comparison through parsePseudoBigInt. The scanner normalizes
    /// binary/octal values at scan time (tsc defers that to
    /// parsePseudoBigInt) — same composition, one conversion.
    pub(crate) fn is_valid_big_int_string(&self, s: &str, round_trip_only: bool) -> bool {
        if s.is_empty() {
            return false;
        }
        let Some(scan) = tsrs2_syntax::scan_big_int_string(s) else {
            return false;
        };
        if scan.contains_separator {
            return false;
        }
        if !round_trip_only {
            return true;
        }
        let Ok(parsed) = crate::expr::parse_pseudo_big_int(&scan.token_value) else {
            return false;
        };
        s == PseudoBigInt {
            negative: scan.negative,
            base10_value: parsed.base10_value,
        }
        .to_base10_string()
    }

    /// tsc-port: parseBigIntLiteralType @6.0.3
    /// tsc-hash: 40e215218d563af7d0b96ce431a11b999b9a0da89c5831c672392b34d7375f45
    /// tsc-span: _tsc.js:68529-68531
    ///
    /// tsc-port: parseValidBigInt @6.0.3
    /// tsc-hash: cfa855ea7fa2cac2b04dc12d6bc5565366853afaf7ecb25bf4c904f51666f843
    /// tsc-span: _tsc.js:18969-18972
    ///
    /// Callers guard with isValidBigIntString, so the parse-recovery
    /// Err inside parsePseudoBigInt is unreachable here — propagated
    /// rather than asserted all the same.
    pub(crate) fn parse_big_int_literal_type(&mut self, text: &str) -> CheckResult2<TypeId> {
        let negative = text.starts_with('-');
        let digits = if negative { &text[1..] } else { text };
        let parsed = crate::expr::parse_pseudo_big_int(&format!("{digits}n"))?;
        Ok(self.tables.get_bigint_literal_type(PseudoBigInt {
            negative,
            base10_value: parsed.base10_value,
        }))
    }

    /// tsc-port: isTypeMatchedByTemplateLiteralType @6.0.3
    /// tsc-hash: 10e3e6c09b4976cfec5a798ea4a9c37923362c263ea75bc20304a9a7a44b3379
    /// tsc-span: _tsc.js:68580-68583
    pub fn is_type_matched_by_template_literal_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        let Some(inferences) = self.infer_types_from_template_literal_type(source, target)? else {
            return Ok(false);
        };
        let (_, target_types) = self.template_parts_of(target);
        for (i, &inference) in inferences.iter().enumerate() {
            if !self.is_valid_type_for_template_literal_placeholder(inference, target_types[i])? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: inferTypesFromTemplateLiteralType @6.0.3
    /// tsc-hash: 9abaf8ac4504967f931a9a1ac1ff06638761380afe56e951af5c860fd7ac9f3a
    /// tsc-span: _tsc.js:68575-68579
    pub(crate) fn infer_types_from_template_literal_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let source_flags = self.tables.flags_of(source);
        if source_flags.intersects(TypeFlags::STRING_LITERAL) {
            let TypeData::Literal {
                value: tsrs2_types::LiteralValue::String(value),
            } = self.tables.type_of(source).data.clone()
            else {
                unreachable!("string literal data");
            };
            return self.infer_from_literal_parts_to_template_literal(
                &[TemplateText::from_utf8(&value)],
                &[],
                target,
            );
        }
        if source_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let (source_texts, source_types) = self.template_parts_of(source);
            let (target_texts, _) = self.template_parts_of(target);
            if source_texts == target_texts {
                let mut mapped = Vec::with_capacity(source_types.len());
                let (_, target_types) = self.template_parts_of(target);
                for (i, &s) in source_types.iter().enumerate() {
                    // 68577 compares the BASE CONSTRAINTS on both
                    // sides (an M3-era identity shortcut here went
                    // stale once M4 made instantiable placeholder
                    // types constructible — pinned).
                    let source_base = self.get_base_constraint_or_type(s)?;
                    let target_base = self.get_base_constraint_or_type(target_types[i])?;
                    if self.is_type_assignable_to(source_base, target_base)? {
                        mapped.push(s);
                    } else {
                        mapped.push(self.get_string_like_type_for_type(s));
                    }
                }
                return Ok(Some(mapped));
            }
            return self.infer_from_literal_parts_to_template_literal(
                &source_texts,
                &source_types,
                target,
            );
        }
        Ok(None)
    }

    /// tsc-port: getStringLikeTypeForType @6.0.3
    /// tsc-hash: 7fa8931292a2608f41ef4622f7506b00e03fc20e5245961f76342983239d1245
    /// tsc-span: _tsc.js:68584-68586
    fn get_string_like_type_for_type(&mut self, ty: TypeId) -> TypeId {
        if self.tables.flags_of(ty).intersects(TypeFlags::from_bits(
            TypeFlags::ANY.bits() | TypeFlags::STRING_LIKE.bits(),
        )) {
            ty
        } else {
            self.tables
                .get_template_literal_type(&["".to_owned(), "".to_owned()], &[ty])
        }
    }

    /// tsc-port: isValidTypeForTemplateLiteralPlaceholder @6.0.3
    /// tsc-hash: 6de8a2d259eac2128f6d433d0b28171ed84d87f387ad1fa45f45f90d75bdc941
    /// tsc-span: _tsc.js:68550-68574
    ///
    /// The tsc OR-chain rendered as early returns — equivalent
    /// because the arm guards are disjoint type kinds. Bigint arm live
    /// since M6 7.2c (isValidBigIntString); StringMapping arms are M4.
    fn is_valid_type_for_template_literal_placeholder(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        if self
            .tables
            .flags_of(target)
            .intersects(TypeFlags::INTERSECTION)
        {
            let TypeData::Intersection { types } = self.tables.type_of(target).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            for t in types.iter() {
                if *t == self.empty_type_literal_type {
                    continue;
                }
                if !self.is_valid_type_for_template_literal_placeholder(source, *t)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        if self.tables.flags_of(target).intersects(TypeFlags::STRING)
            || self.is_type_assignable_to(source, target)?
        {
            return Ok(true);
        }
        if self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::STRING_LITERAL)
        {
            let TypeData::Literal {
                value: tsrs2_types::LiteralValue::String(value),
            } = self.tables.type_of(source).data.clone()
            else {
                unreachable!("string literal data");
            };
            let target_flags = self.tables.flags_of(target);
            if target_flags.intersects(TypeFlags::NUMBER)
                && self.is_valid_number_string(&value, /*round_trip_only*/ false)
            {
                return Ok(true);
            }
            if target_flags.intersects(TypeFlags::BIG_INT)
                && self.is_valid_big_int_string(&value, /*round_trip_only*/ false)
            {
                return Ok(true);
            }
            if target_flags.intersects(TypeFlags::from_bits(
                TypeFlags::BOOLEAN_LITERAL.bits() | TypeFlags::NULLABLE.bits(),
            )) {
                if let TypeData::Intrinsic { name, .. } = &self.tables.type_of(target).data {
                    return Ok(value == *name);
                }
            }
            if target_flags.intersects(TypeFlags::STRING_MAPPING) {
                return self.is_member_of_string_mapping(source, target);
            }
            if target_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                return self.is_type_matched_by_template_literal_type(source, target);
            }
            return Ok(false);
        }
        if self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::TEMPLATE_LITERAL)
        {
            let (texts, types) = self.template_parts_of(source);
            if texts.len() == 2 && texts[0].is_empty() && texts[1].is_empty() {
                return self.is_type_assignable_to(types[0], target);
            }
            return Ok(false);
        }
        Ok(false)
    }

    /// tsc-port: inferFromLiteralPartsToTemplateLiteral @6.0.3
    /// tsc-hash: de966da853f6f389697dbdb53fbec18574c39bdbf035afd06091fbad6a9877cd
    /// tsc-span: _tsc.js:68587-68636
    ///
    /// The pure text-matching algorithm, ported exactly — over UTF-16
    /// code units, because every JS index/length here (`pos + 1`,
    /// `indexOf`, `slice`) counts code units (the review's `é` pins
    /// panicked the byte-indexed version). A slice that would strand
    /// half a surrogate pair (astral char split by an empty
    /// placeholder step) escapes as Unsupported rather than fabricate
    /// a replacement-character literal.
    #[allow(clippy::needless_range_loop)] // seg/pos cursor walk, ported as tsc wrote it
    fn infer_from_literal_parts_to_template_literal(
        &mut self,
        source_texts: &[TemplateText],
        source_types: &[TypeId],
        target: TypeId,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let source_units: Vec<Vec<u16>> = source_texts
            .iter()
            .map(|text| text.units().to_vec())
            .collect();
        let last_source_index = source_texts.len() - 1;
        let (target_texts, _) = self.template_parts_of(target);
        let target_units: Vec<Vec<u16>> = target_texts
            .iter()
            .map(|text| text.units().to_vec())
            .collect();
        let last_target_index = target_units.len() - 1;
        {
            let source_start = &source_units[0];
            let source_end = &source_units[last_source_index];
            let target_start = &target_units[0];
            let target_end = &target_units[last_target_index];
            if (last_source_index == 0
                && source_start.len() < target_start.len() + target_end.len())
                || !source_start.starts_with(target_start)
                || !source_end.ends_with(target_end)
            {
                return Ok(None);
            }
        }
        let remaining_end_units: Vec<u16> = {
            let source_end = &source_units[last_source_index];
            source_end[..source_end.len() - target_units[last_target_index].len()].to_vec()
        };
        let get_source_units = |index: usize| -> &[u16] {
            if index < last_source_index {
                &source_units[index]
            } else {
                &remaining_end_units
            }
        };
        let mut matches: Vec<TypeId> = Vec::new();
        let mut seg = 0usize;
        let mut pos = target_units[0].len();
        macro_rules! add_match {
            ($s:expr, $p:expr) => {{
                let s = $s;
                let p = $p;
                let match_type = if s == seg {
                    let text = utf16_to_string(&get_source_units(s)[pos..p])?;
                    self.tables.get_string_literal_type(&text)
                } else {
                    let mut texts = vec![TemplateText::from_utf16(&source_units[seg][pos..])];
                    texts.extend(source_texts[seg + 1..s].iter().cloned());
                    texts.push(TemplateText::from_utf16(&get_source_units(s)[..p]));
                    let types = source_types[seg..s].to_vec();
                    self.tables
                        .get_template_literal_type_from_texts(&texts, &types)
                };
                matches.push(match_type);
                #[allow(unused_assignments)]
                {
                    seg = s;
                    pos = p;
                }
            }};
        }
        for i in 1..last_target_index {
            let delim = &target_units[i];
            if !delim.is_empty() {
                let mut s = seg;
                let mut p = pos;
                loop {
                    match find_utf16(get_source_units(s), delim, p) {
                        Some(found) => {
                            p = found;
                            break;
                        }
                        None => {
                            s += 1;
                            if s == source_units.len() {
                                return Ok(None);
                            }
                            p = 0;
                        }
                    }
                }
                add_match!(s, p);
                pos += delim.len();
            } else if pos < get_source_units(seg).len() {
                let p = pos + 1;
                add_match!(seg, p);
            } else if seg < last_source_index {
                add_match!(seg + 1, 0);
            } else {
                return Ok(None);
            }
        }
        add_match!(last_source_index, get_source_units(last_source_index).len());
        Ok(Some(matches))
    }
}

/// `haystack.indexOf(needle, from)` over UTF-16 code units.
fn find_utf16(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(haystack.len()));
    }
    if haystack.len() < needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// Decode a code-unit slice back to a Rust string; a stranded
/// surrogate half (JS would keep it, Rust strings cannot) escapes as
/// Unsupported instead of fabricating U+FFFD literal text.
fn utf16_to_string(units: &[u16]) -> CheckResult2<String> {
    String::from_utf16(units).map_err(|_| {
        Unsupported::new(
            "template inference strands a surrogate half (UTF-16 WTF-16 representation, M8)",
        )
    })
}

/// tsc isNumericLiteralName over JS number round-trip (19205): the
/// name coerces to a number whose string form is the name — over the
/// RAW coercion, so "NaN" (and the Infinity spellings) count exactly
/// like tsc's `(+name).toString() === name`.
fn is_numeric_literal_name_js(name: &str) -> bool {
    js_number_to_string(crate::evaluate::js_string_to_number(name)) == name
}

/// tsrs-native: the `+s` coercion — the full ToNumber port lives in
/// evaluate.rs (M6 expression checking); this face keeps the
/// None-encodes-NaN shape its relation/inference callers branch on.
/// The M4-era local slice it replaces dropped the 0b/0o radix forms
/// (and let Rust's float parser admit "inf" spellings JS rejects) —
/// the 9.3b4 display arm unmasked the 0b/0o gap as `${number}`
/// pattern 2345 fabrications on templateLiteralTypesPatterns.
pub(crate) fn js_string_to_number(s: &str) -> Option<f64> {
    let n = crate::evaluate::js_string_to_number(s);
    if n.is_nan() {
        None
    } else {
        Some(n)
    }
}

/// JS number formatting for the round-trip checks — the canonical
/// ECMAScript Number::toString lives in tsrs2_types (template folding
/// shares it).
fn js_number_to_string(value: f64) -> String {
    tsrs2_types::js_number_to_string(value)
}

#[cfg(test)]
mod tests {
    use tsrs2_binder::bind_source_file;
    use tsrs2_syntax::{parse_source_file, LanguageVariant, ParseOptions};
    use tsrs2_types::CompilerOptions;

    use crate::relpin::find_probe_annotation;
    use crate::relpin::{probe_relation, RelpinQuery, RelpinRelation, RelpinVerdict};
    use crate::state::CheckerState;

    fn probe(
        setup: &str,
        source: &str,
        target: &str,
        fresh: bool,
        relation: RelpinRelation,
    ) -> RelpinVerdict {
        let options = CompilerOptions::default();
        probe_relation(&RelpinQuery {
            setup,
            source,
            target,
            source_is_fresh: fresh,
            relation,
            options: &options,
        })
    }

    #[test]
    fn slice_tuple_type_negative_end_counts_from_the_end() {
        // 61290 + JS Array.prototype.slice: endSkipCount beyond the
        // arity turns the slice end NEGATIVE and JS re-reads it from
        // the END — max(2*len - skip, 0). Reachable since 7.4's
        // impliedArity record (the 69114 both-variadic arm passes
        // endLength + sourceArity - impliedArity, which exceeds
        // sourceArity whenever impliedArity < endLength; fixture
        // corroborated against vendored tsc, scratchpad probe74k.mjs).
        // Pre-fix the port clamped the whole window to empty.
        crate::state::test_support::with_program_state(
            &[("a.ts", "var v: [string, number, boolean];\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation =
                    find_probe_annotation(state.binder.source(0), "v").expect("annotated var");
                let tuple = state
                    .get_type_from_type_node(annotation)
                    .expect("tuple type");
                // len 3, skip 4: JS slice(0, -1) → [0, 2).
                let sliced = state.slice_tuple_type(tuple, 0, 4).expect("slice succeeds");
                let elements = state.get_type_arguments(sliced).expect("elements");
                assert_eq!(
                    elements.len(),
                    2,
                    "negative end counts from the end (2*3 - 4)"
                );
                // len 3, skip 7 (beyond 2*len): floored to empty.
                let floored = state.slice_tuple_type(tuple, 0, 7).expect("slice succeeds");
                let none = state.get_type_arguments(floored).expect("elements");
                assert_eq!(none.len(), 0, "max(2*len - skip, 0) floors at zero");
                // The inverted-range clamp is unchanged: skip 2 puts
                // the end (1) below the start (2) — still empty.
                let inverted = state.slice_tuple_type(tuple, 2, 2).expect("slice succeeds");
                let inv = state.get_type_arguments(inverted).expect("elements");
                assert_eq!(inv.len(), 0, "end before start clamps to empty");
            },
        );
    }

    #[test]
    fn excess_property_checks_fire_on_fresh_probe_sources() {
        assert!(matches!(
            probe(
                "",
                "{ a: number, b: number }",
                "{ a: number }",
                true,
                RelpinRelation::Assignable,
            ),
            RelpinVerdict::NotRelated
        ));
        // The same pair declared (non-fresh) is plain width subtyping.
        assert!(matches!(
            probe(
                "",
                "{ a: number, b: number }",
                "{ a: number }",
                false,
                RelpinRelation::Assignable,
            ),
            RelpinVerdict::Related
        ));
    }

    #[test]
    fn empty_template_fragments_keep_empty_cooked_text() {
        // Regression: current_token_text's missing-token fallback used
        // to turn the empty tail of `a${string}` into the token NAME,
        // breaking template reduction and matching.
        let options = CompilerOptions::default();
        let source = parse_source_file(
            "template-regression.ts".to_owned(),
            "declare var c: `${string}`;\n".to_owned(),
            ParseOptions {
                language_variant: LanguageVariant::Standard,
                javascript_file: false,
                ..ParseOptions::default()
            },
            None,
        );
        assert!(source.parse_diagnostics.is_empty());
        let binder = bind_source_file(&source, &options);
        let mut state = CheckerState::new(&source, &binder, &options);
        let annotation = find_probe_annotation(&source, "c").expect("annotation");
        let ty = state
            .get_type_from_type_node(annotation)
            .expect("template annotation resolves");
        assert_eq!(
            ty, state.tables.intrinsics.string,
            "`${{string}}` reduces to string (62075-62078)"
        );
    }

    #[test]
    fn structural_relations_match_known_verdicts() {
        // Maybe-path recursion.
        let recursive = "interface A { next: B }\ninterface B { next: A }";
        assert!(matches!(
            probe(recursive, "A", "B", false, RelpinRelation::Assignable),
            RelpinVerdict::Related
        ));
        let divergent = "interface A { next: B; x: number }\ninterface B { next: A; x: string }";
        assert!(matches!(
            probe(divergent, "A", "B", false, RelpinRelation::Assignable),
            RelpinVerdict::NotRelated
        ));
        // Tuple arm.
        assert!(matches!(
            probe(
                "",
                "[number]",
                "[number, string?]",
                false,
                RelpinRelation::Assignable
            ),
            RelpinVerdict::Related
        ));
        assert!(matches!(
            probe(
                "",
                "[number, string]",
                "[number]",
                false,
                RelpinRelation::Assignable
            ),
            RelpinVerdict::NotRelated
        ));
        // Template matching.
        assert!(matches!(
            probe(
                "",
                "\"abc\"",
                "`a${string}`",
                false,
                RelpinRelation::Assignable
            ),
            RelpinVerdict::Related
        ));
        // Signatures: strictFunctionTypes contravariance is the
        // default-strict behavior.
        assert!(matches!(
            probe(
                "",
                "(x: 1) => void",
                "(x: number) => void",
                false,
                RelpinRelation::Assignable,
            ),
            RelpinVerdict::NotRelated
        ));
        // Index signatures.
        assert!(matches!(
            probe(
                "",
                "{ a: number }",
                "{ [k: string]: number }",
                false,
                RelpinRelation::Assignable,
            ),
            RelpinVerdict::Related
        ));
    }

    // ---- m4-review A5: createUnionOrIntersectionProperty modifier /
    // writeTypes propagation (tsc-probed rows, vendored 6.0.3 noLib,
    // strict defaults) ----

    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        rows_and_partials(text).0
    }

    /// The containment-aware face (7.5d review): a `(rows, 0)` pin
    /// proves the path verdicts LIVE — a bare `checked_rows == []`
    /// cannot distinguish a clean pass from an Err-contained
    /// statement.
    fn rows_and_partials(text: &str) -> (Vec<(u32, u32, u32)>, usize) {
        crate::state::test_support::with_program_state(
            &[("a.ts", text)],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let rows = state
                    .diagnostics
                    .iter()
                    .filter(|diag| diag.file_name.is_some())
                    .map(|diag| {
                        (
                            diag.code(),
                            diag.start.unwrap_or(u32::MAX),
                            diag.length.unwrap_or(u32::MAX),
                        )
                    })
                    .collect();
                (rows, state.partial_check_records.len())
            },
        )
    }

    #[test]
    fn conflicting_private_union_property_bails_out() {
        // ContainsPrivate now folds per member (59148-59152): distinct
        // private declarations kill the union property -> 2339.
        assert_eq!(
            checked_rows(
                "class A { private x: number = 1; m() { return this.x } }\nclass B { private x: number = 2; m() { return this.x } }\ndeclare const u: A | B;\nu.x;\n"
            ),
            [(2339, 140, 1)]
        );
    }

    #[test]
    fn conflicting_private_intersection_reduces_to_never() {
        // The never-reduction consumer reads CONTAINS_PRIVATE off the
        // synthetic: A & B collapses, so the never assignment is clean.
        assert_eq!(
            checked_rows(
                "class A { private x: number = 1; m() { return this.x } }\nclass B { private x: number = 2; m() { return this.x } }\ndeclare const i: A & B;\nconst n: never = i;\n"
            ),
            []
        );
    }

    #[test]
    fn union_accessor_write_type_is_the_setter_union() {
        // writeTypes propagation (59209-59213/59237-59239): both
        // assignments target (number | string) | (number | boolean).
        assert_eq!(
            checked_rows(
                "class A { get p(): number { return 1 } set p(v: number | string) {} }\nclass B { get p(): number { return 2 } set p(v: number | boolean) {} }\ndeclare const u: A | B;\nu.p = true;\nu.p = 3;\n"
            ),
            []
        );
    }

    #[test]
    fn same_declaration_private_instantiations_survive_the_bailout() {
        // getCommonDeclarationsOfSymbols carve-out (59172): C<string>
        // and C<number> share the one `x` declaration, so the union
        // property survives and in-class access stays legal.
        assert_eq!(
            checked_rows(
                "class C<T> { private x: T; constructor(v: T) { this.x = v } m(o: C<string> | C<number>) { return o.x; } }\n"
            ),
            []
        );
    }

    // ---- M6 7.5 B7: the compareTypePredicateRelatedTo decision
    // table (64577-64628). All rows oracle-pinned 2026-07-21
    // (scratchpad probe75.mjs / probe75b.mjs / probe75c.mjs,
    // vendored 6.0.3 noLib, strict defaults). Verdict pins whose tsc
    // head args sit behind the display curtain ride the
    // @ts-expect-error band, which lives in the PROGRAM driver
    // (directive filtering + 2578 synthesis + the S8 partial-check
    // exemption) — those use program_rows, not checked_rows. ----

    fn program_rows(text: &str) -> Vec<(u32, Option<u32>, Option<u32>)> {
        let result = crate::check_program(
            &[crate::InputFile {
                name: "a.ts".to_owned(),
                text: text.to_owned(),
            }],
            &CompilerOptions::default(),
        );
        result
            .diagnostics
            .iter()
            .map(|d| (d.code(), d.start, d.length))
            .collect()
    }

    #[test]
    fn predicate_both_sides_mismatched_types_fail_the_relation() {
        // Both sides carry identifier predicates; string vs number
        // fails compareTypePredicateRelatedTo's type compare. tsc
        // reports the 2322 head (1226 chain) — the head's
        // function-type args sit behind the display curtain
        // (typeToString 5.4 slice, T2/M8), so the verdict is pinned
        // via the @ts-expect-error band: a used directive is []
        // on both sides, a wrong TRUE verdict would surface 2578,
        // and the display-Err containment path stays exempt (S8).
        assert_eq!(
            program_rows(
                "declare function isCat(x: unknown): x is string;\n// @ts-expect-error\nconst f: (x: unknown) => x is number = isCat;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_expect_error_control_reports_unused_2578() {
        // CONTROL for the verdict pins above/below: when the
        // predicate relation SUCCEEDS, the directive goes unused and
        // the 2578 row fires — proving the [] pins observe verdicts,
        // not blanket suppression.
        assert_eq!(
            program_rows(
                "declare function isCat(x: unknown): x is string;\n// @ts-expect-error\nconst f: (x: unknown) => x is string = isCat;\n"
            ),
            [(2578, Some(49), Some(19))]
        );
    }

    #[test]
    fn predicate_both_sides_equal_types_relate() {
        // Zero partials (7.5d): the entry cells verdict LIVE, not by
        // containment.
        assert_eq!(
            rows_and_partials(
                "declare function isCat(x: unknown): x is string;\nconst f: (x: unknown) => x is string = isCat;\n"
            ),
            (vec![], 0)
        );
    }

    #[test]
    fn predicate_target_only_identifier_fails_the_relation() {
        // Target-only identifier predicate = the 1224-family cell: a
        // plain boolean source can never satisfy `x is string`
        // (verdict via the expect-error band; head display is T2/M8).
        assert_eq!(
            program_rows(
                "declare function plain(x: unknown): boolean;\n// @ts-expect-error\nconst g: (x: unknown) => x is string = plain;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_target_only_asserts_falls_through_silently() {
        // Asserts-form target alone: the void-return early return
        // (64577-64579) catches it before the predicate arm — no error.
        assert_eq!(
            checked_rows(
                "declare function plain2(x: unknown): void;\nconst h: (x: unknown) => asserts x is string = plain2;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_source_only_compares_as_boolean_return() {
        // Source-only is-predicate: plain return comparison — the
        // predicate signature's return type is boolean.
        assert_eq!(
            checked_rows(
                "declare function isNum(x: unknown): x is number;\nconst k: (x: unknown) => boolean = isNum;\n"
            ),
            []
        );
        assert_eq!(
            program_rows(
                "declare function isNum(x: unknown): x is number;\n// @ts-expect-error\nconst k2: (x: unknown) => string = isNum;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_source_only_asserts_compares_as_void_return() {
        // Asserts-source vs plain boolean target: the plain return
        // comparison sees VOID (not boolean) and fails; a void target
        // takes the 64577-64579 early return instead.
        assert_eq!(
            program_rows(
                "declare function aStr(x: unknown): asserts x is string;\n// @ts-expect-error\nconst z: (x: unknown) => boolean = aStr;\n"
            ),
            []
        );
        assert_eq!(
            checked_rows(
                "declare function aStr2(x: unknown): asserts x is string;\nconst z2: (x: unknown) => void = aStr2;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_parameter_index_mismatch_fails_the_relation() {
        // Identifier predicates on different parameter positions fail
        // the 64614 parameterIndex check (1227 chain, T2; verdict via
        // the expect-error band).
        assert_eq!(
            program_rows(
                "declare function isA(a: unknown, b: unknown): a is string;\n// @ts-expect-error\nconst m: (a: unknown, b: unknown) => b is string = isA;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_kind_mismatch_fails_the_relation() {
        // Identifier source vs this-based target (and asserts vs
        // plain is-form) fail the 64607 kind check (2518 chain, T2;
        // verdicts via the expect-error band).
        assert_eq!(
            program_rows(
                "declare function isThis(this: object, x: unknown): boolean;\ndeclare const src: (x: unknown) => x is string;\n// @ts-expect-error\nconst n: { (x: unknown): this is object } = src;\n"
            ),
            []
        );
        assert_eq!(
            program_rows(
                "declare function assertIsStr2(x: unknown): asserts x is string;\n// @ts-expect-error\nconst q: (x: unknown) => x is string = assertIsStr2;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_union_signature_matching_consults_no_predicate() {
        // findMatchingSignature runs ignoreReturnTypes=true — the
        // identical-path predicate consult sits INSIDE
        // !ignoreReturnTypes (67624-67628), so predicate-carrying
        // union members produce a callable union signature (the old
        // gate over-contained this cell).
        assert_eq!(
            checked_rows(
                "declare const u: ((x: unknown) => x is string) | ((x: unknown) => x is string);\nif (u(3)) {}\n"
            ),
            []
        );
        assert_eq!(
            checked_rows(
                "declare const u2: ((x: unknown) => x is string) | ((x: unknown) => boolean);\nif (u2(3)) {}\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_comparable_relation_fails() {
        // The comparable path reaches the predicate arm (boolean
        // return, not void): unrelated predicate types are not
        // comparable either way — tsc's 2352 head args are behind
        // the display curtain, so the verdict rides the expect-error
        // band.
        assert_eq!(
            program_rows(
                "declare const src2: (x: unknown) => x is string;\n// @ts-expect-error\nconst c0 = src2 as (x: unknown) => x is number;\n"
            ),
            []
        );
    }

    // ---- B7 positive movers: statements the old gate contained
    // wholesale now check through and surface their OTHER rows
    // (renderable args). Oracle-pinned 2026-07-21 (probe75c.mjs). ----

    #[test]
    fn predicate_relation_unblocks_sibling_declarator_row() {
        // Declarator 1's predicate relation now succeeds instead of
        // Err-containing the whole statement; declarator 2's plain
        // string→number mismatch surfaces.
        assert_eq!(
            checked_rows(
                "declare function isCat(x: unknown): x is string;\nconst f: (x: unknown) => x is string = isCat, bad: number = \"s\";\n"
            ),
            [(2322, 95, 3)]
        );
    }

    #[test]
    fn predicate_union_call_result_row_surfaces() {
        // findMatchingSignature (ignoreReturnTypes=true) consults no
        // predicate: the union signature resolves and the boolean
        // call result fails the number annotation.
        assert_eq!(
            checked_rows(
                "declare const u2: ((x: unknown) => x is string) | ((x: unknown) => boolean);\nconst r: number = u2(3);\n"
            ),
            [(2322, 83, 1)]
        );
    }

    // ---- M6 7.5 ripple audit: the steps-doc consumer list
    // (contextual element inference, generic new, tagged templates,
    // satisfies, the 2769 failure-path candidate choice) probed
    // port-vs-oracle 11-for-11 (probe75f/probe75g.mjs, 2026-07-21) —
    // representative rows pinned here. ----

    #[test]
    fn ripple_generic_new_and_tagged_template_infer() {
        assert_eq!(
            checked_rows(
                "declare class Box<T> { constructor(x: T); v: T; }\nconst b = new Box(\"s\");\nconst n: number = b.v;\n"
            ),
            [(2322, 80, 1)]
        );
        assert_eq!(
            checked_rows(
                "declare function tag2<T>(parts: unknown, x: T): T;\nconst t2: number = tag2`a${\"s\"}b`;\n"
            ),
            [(2322, 57, 2)]
        );
    }

    #[test]
    fn ripple_satisfies_runs_the_live_relations() {
        // Generic source satisfies via the B8 arm; predicate faces via
        // the B7 table (the failing half rides the expect-error band).
        assert_eq!(
            checked_rows(
                "declare function id<T>(x: T): T;\nconst s = id satisfies (x: number) => number;\n"
            ),
            []
        );
        assert_eq!(
            program_rows(
                "declare function isCat(x: unknown): x is string;\nconst sp = isCat satisfies (x: unknown) => x is string;\n// @ts-expect-error\nconst sp2 = isCat satisfies (x: unknown) => x is number;\n"
            ),
            []
        );
    }

    #[test]
    fn ripple_overload_failure_paths_report_oracle_rows() {
        // 2769 head at the callee; a generic candidate pair takes the
        // single-argument-error 2345 face (getCandidateForOverload-
        // Failure's candidate choice); bare arity keeps 2554.
        assert_eq!(
            checked_rows(
                "declare function f(x: string): void;\ndeclare function f(x: boolean): void;\nf(3);\n"
            ),
            [(2769, 77, 1)]
        );
        assert_eq!(
            checked_rows(
                "declare function g<T extends string>(x: T): T;\ndeclare function g<T extends boolean>(x: T, y: T): T;\ng(3);\n"
            ),
            [(2345, 103, 1)]
        );
        assert_eq!(
            checked_rows("declare function h<T>(x: T, y: T): T;\nh();\n"),
            [(2554, 38, 1)]
        );
    }

    // ---- M6 7.5 B8: compareSignaturesRelated head rebuild —
    // generic-source instantiation (64505-64514), erase honoring
    // (67069-67071), same-target pairwise arm (66952-66966). Rows
    // oracle-pinned 2026-07-21 (probe75.mjs / probe75b.mjs /
    // probe75e.mjs, vendored 6.0.3 noLib). ----

    #[test]
    fn generic_source_instantiates_against_concrete_target() {
        // <T>(x:T)=>T infers T:=number against (x:number)=>number.
        assert_eq!(
            checked_rows(
                "declare function id3<T>(x: T): T;\nconst a1: (x: number) => number = id3;\n"
            ),
            []
        );
        // The failing face: T[] return vs string — verdict via the
        // expect-error band (the 2322 head prints the generic
        // function type, display curtain T2/M8).
        assert_eq!(
            program_rows(
                "declare function id4<T>(x: T): T[];\n// @ts-expect-error\nconst a2: (x: number) => string = id4;\n"
            ),
            []
        );
    }

    #[test]
    fn generic_source_constraint_clamp_compares_under_the_frame() {
        // T extends {id:number} satisfied — the clamp compare passes
        // LIVE (zero partials, 7.5d) and the relation holds.
        assert_eq!(
            rows_and_partials(
                "declare function pick<T extends { id: number }>(x: T): T;\nconst a4: (x: { id: number; name: string }) => { id: number; name: string } = pick;\n"
            ),
            (vec![], 0)
        );
        // T extends string violated by T:=number — the clamp's
        // RelationFrame compare rejects, the instantiated source
        // carries string params, and the param arm fails (verdict via
        // the expect-error band).
        assert_eq!(
            program_rows(
                "declare function pick2<T extends string>(x: T, y: T): T;\n// @ts-expect-error\nconst a5: (x: number, y: number) => number = pick2;\n"
            ),
            []
        );
    }

    #[test]
    fn generic_to_generic_relates_via_canonical_target() {
        assert_eq!(
            checked_rows("declare function id5<T>(x: T): T;\nconst a3: <U>(x: U) => U = id5;\n"),
            []
        );
    }

    #[test]
    fn canonical_signature_recanonicalizes_instantiated_methods() {
        // I<number>.m carries a cloned U (tp.target set): the
        // unconstrained clone re-canonicalizes to its target; the
        // constrained variant keeps the clone.
        assert_eq!(
            rows_and_partials(
                "interface I<T> { m<U>(x: T, y: U): void; }\ndeclare const i: I<number>;\nconst mf: (x: number, y: string) => void = i.m;\n"
            ),
            (vec![], 0)
        );
        assert_eq!(
            rows_and_partials(
                "interface I2<T> { m<U extends T>(x: T, y: U): U; }\ndeclare const i2: I2<number>;\nconst mf2: (x: number, y: 3) => 3 = i2.m;\n"
            ),
            (vec![], 0)
        );
        assert_eq!(
            program_rows(
                "interface I3<T> { m<U extends string>(x: T, y: U): U; }\ndeclare const i3: I3<number>;\n// @ts-expect-error\nconst mf3: (x: number, y: number) => number = i3.m;\n"
            ),
            []
        );
    }

    #[test]
    fn comparable_relation_erases_generics() {
        // eraseGenerics (relation == comparable) now honors the erase
        // parameter: type parameters erase to any, so BOTH the benign
        // and the shape-mismatched as-assertions are comparable —
        // the unused directive (2578) pins the success where the old
        // gate contained the statement (S8-exempt silence).
        assert_eq!(
            checked_rows(
                "declare function id<T>(x: T): T;\nconst c1 = id as (x: number) => number;\n"
            ),
            []
        );
        assert_eq!(
            program_rows(
                "declare function id2<T extends string>(x: T): T;\n// @ts-expect-error\nconst c2 = id2 as (x: number) => boolean;\n"
            ),
            [(2578, Some(49), Some(19))]
        );
    }

    #[test]
    fn same_target_instantiations_compare_pairwise() {
        // Box<number> vs Box<string>: index-to-index (s0 vs t0 fails
        // on number vs string) where the old N×M walk found s1 for
        // every target row and wrongly related.
        assert_eq!(
            checked_rows(
                "interface Box<T> {\n  m(x: T): T;\n  m(x: string): string;\n}\ndeclare const srcb: Box<number>;\nconst dstb: Box<string> = srcb;\n"
            ),
            [(2322, 98, 4)]
        );
        // Compatible instantiations still relate pairwise…
        assert_eq!(
            checked_rows(
                "interface Box<T> {\n  m(x: T): T;\n  m(x: string): string;\n}\ndeclare const src3: Box<number>;\nconst dst: Box<number | string> = src3;\n"
            ),
            []
        );
        // …and the structural twin (distinct targets) keeps N×M.
        assert_eq!(
            checked_rows(
                "interface BoxN {\n  m(x: number): number;\n  m(x: string): string;\n}\ninterface BoxS {\n  m(x: string): string;\n  m(x: string): string;\n}\ndeclare const srcn: BoxN;\nconst dstn: BoxS = srcn;\n"
            ),
            []
        );
    }

    #[test]
    fn generic_predicate_source_infers_through_the_predicate_arm() {
        // applyToReturnTypes' predicate arm (68224-68237) feeds the
        // arm's iSICO: T[] ⇐ number[] under the predicate types.
        assert_eq!(
            checked_rows(
                "declare function isArr<T>(x: unknown): x is T[];\nconst gp: (x: unknown) => x is number[] = isArr;\n"
            ),
            []
        );
    }

    #[test]
    fn predicate_argument_passthrough_row_surfaces() {
        // The argument check's predicate relation succeeds; the call
        // types and the return-annotation mismatch surfaces.
        assert_eq!(
            checked_rows(
                "declare function isCat(x: unknown): x is string;\ndeclare function take(p: (x: unknown) => x is string): number;\nconst ok: string = take(isCat);\n"
            ),
            [(2322, 118, 2)]
        );
    }

    // ---- M6 7.5d review fixes: regression pins. Every fixture
    // oracle-probed against vendored 6.0.3 noLib (2026-07-21,
    // scratchpad probe-review.cjs / probe-port). ----

    #[test]
    fn forward_constraint_generic_resolves_through_the_parked_frame() {
        // Blocker fix: <T extends U, U extends string> — resolving
        // slot T instantiates its constraint through the DEFERRED
        // non-fixing mapper, re-entering slot U (and U's clamp)
        // MID-iSICO; pre-7.5d the parameter-threaded loan missed the
        // thunk path and the RelationFrame dispatch panicked. Pass
        // face: tsc clean — zero containment proves the re-entrant
        // path completes live.
        assert_eq!(
            rows_and_partials(
                "declare function f<T extends U, U extends string>(x: T, y: U): void;\nconst g: (x: \"a\", y: \"a\") => void = f;\n"
            ),
            (vec![], 0)
        );
        // Fail face: 2322 renders at the 9.3b2 signature rung
        // (oracle-probed byte row).
        assert_eq!(
            rows_and_partials(
                "declare function f<T extends U, U extends string>(x: T, y: U): void;\nconst g: (x: \"a\", y: \"b\") => void = f;\n"
            ),
            (vec![(2322, 75, 1)], 0)
        );
    }

    #[test]
    fn forward_constraint_object_member_re_enters_during_the_clamp() {
        // The InFlight face: T's clamp WALKS the instantiated
        // { u: U } constraint, whose lazy member resolution re-enters
        // slot U while the frame is checked out — the fresh-sub-walk
        // fallback (engine.rs RelationFrameLoan::InFlight) carries
        // it. tsc: clean / 2322-behind-the-curtain.
        assert_eq!(
            rows_and_partials(
                "declare function f2<T extends { u: U }, U extends string>(x: T, y: U): void;\nconst g2: (x: { u: \"b\" }, y: \"b\") => void = f2;\n"
            ),
            (vec![], 0)
        );
        // The fail face renders at the 9.3b2 signature rung
        // (oracle-probed byte row).
        assert_eq!(
            rows_and_partials(
                "declare function f2<T extends { u: U }, U extends string>(x: T, y: U): void;\nconst g2: (x: { u: \"a\" }, y: \"b\") => void = f2;\n"
            ),
            (vec![(2322, 83, 2)], 0)
        );
    }

    #[test]
    fn this_parameter_blocks_the_body_inferred_predicate() {
        // tsc iterates func.parameters INCLUDING the this-parameter
        // (79049's forEach index feeds createTypePredicate), so a
        // leading `this: object` yields no USABLE predicate for x —
        // overload 2 (boolean → string) wins and the 2322 reports.
        // tsc-probed q8 (vendored 6.0.3 noLib): port row-identical,
        // no off-by-one divergence.
        assert_eq!(
            rows_and_partials(
                "function isStr(this: object, x: unknown) { return typeof x === \"string\"; }\ndeclare function take(p: (this: object, x: unknown) => x is string): number;\ndeclare function take(p: (this: object, x: unknown) => boolean): string;\nconst n: number = take(isStr);\n",
            ),
            (vec![(2322, 231, 1)], 0)
        );
    }

    #[test]
    fn body_inferred_predicates_decide_for_real() {
        // m6 7.6 flip of the 7.5d containment pins: the body-
        // inference arm (getTypePredicateFromBody, 79019-79074) is
        // LIVE, so these faces DECIDE. tsc-probed q1a/q1b/q1c
        // (vendored 6.0.3 noLib).
        // Overloads: `x is string` is inferred from isStr's body and
        // overload 1 resolves — clean, no containment.
        assert_eq!(
            rows_and_partials(
                "function isStr(x: unknown) { return typeof x === \"string\"; }\ndeclare function take(p: (x: unknown) => x is string): number;\ndeclare function take(p: (x: unknown) => boolean): string;\nconst n: number = take(isStr);\n"
            ),
            (vec![], 0)
        );
        // Override compat: body-inferred predicates on BOTH sides —
        // tsc reports (2416, 82, 3); the row's args are function
        // displays, so the port renders or contains by the display
        // slice, never fabricates.
        assert_eq!(
            rows_and_partials(
                "class A { isS(x: unknown) { return typeof x === \"string\"; } }\nclass B extends A { isS(x: unknown) { return typeof x === \"number\"; } }\n"
            ),
            (vec![(2416, 82, 3)], 0)
        );
        // The inferred source predicate satisfies the annotated
        // target — clean.
        assert_eq!(
            rows_and_partials(
                "function isStr(x: unknown) { return typeof x === \"string\"; }\nconst f: (x: unknown) => x is string = isStr;\n"
            ),
            (vec![], 0)
        );
    }

    #[test]
    fn body_inferred_guard_leaves_plain_boolean_helpers_live() {
        // The related arm consults the source only under a
        // target-side predicate (tsc order), so an unannotated
        // boolean helper against a plain boolean target never
        // reaches the guard — zero containment.
        assert_eq!(
            rows_and_partials(
                "function isPos(x: number) { return x > 0; }\nconst p: (x: number) => boolean = isPos;\n"
            ),
            (vec![], 0)
        );
    }

    #[test]
    fn instantiated_generic_parameter_suppresses_callback_treatment() {
        // Major fix: 64549-64550's SECOND suppression disjunct —
        // I<(x: string) => void>'s m keeps signature.target whose
        // v-position is T (generic), so the position takes the plain
        // bivariant compare (the fewer-params source leg passes),
        // NOT the callback recursion whose arity check wrongly
        // Falsed. tsc: clean.
        assert_eq!(
            rows_and_partials(
                "interface I<T> { m(v: T): void }\ninterface J { m(v: (x: string, y: number) => void): void }\ndeclare const a: I<(x: string) => void>;\nconst b: J = a;\n"
            ),
            (vec![], 0)
        );
        // Control: annotation-derived positions carry no
        // signature.target, so callback treatment SURVIVES — the
        // arity-incompatible pair still Falses, and the 2322 renders
        // at the 9.3b2 signature rung (oracle-probed byte row).
        assert_eq!(
            rows_and_partials(
                "declare function on(cb: (x: string) => void): void;\ndeclare const h: (x: string, y: number) => void;\nconst c: (cb: (x: string, y: number) => void) => void = on;\n"
            ),
            (vec![(2322, 107, 1)], 0)
        );
    }

    #[test]
    fn predicate_type_compare_arm_relates_live() {
        // The both-Some UNEQUAL-types cell ('a' vs string) — the
        // compareTypes arm proper, not the ty == ty shortcut; zero
        // containment proves the verdict is live (the pre-7.5d 2578
        // control only ever exercised the shortcut).
        assert_eq!(
            rows_and_partials(
                "declare function isLit(x: unknown): x is \"a\";\nconst cf: (x: unknown) => x is string = isLit;\n"
            ),
            (vec![], 0)
        );
    }

    #[test]
    fn predicate_parameter_index_match_relates_live() {
        // The nonzero-index positive twin of the mismatch pin:
        // index 1 == 1 passes the 64614 check and the relation
        // completes with zero containment.
        assert_eq!(
            rows_and_partials(
                "declare function isB(a: unknown, b: unknown): b is string;\nconst m2: (a: unknown, b: unknown) => b is string = isB;\n"
            ),
            (vec![], 0)
        );
    }
}

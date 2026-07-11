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
use tsrs2_syntax::{NodeData, SyntaxKind};
use tsrs2_types::{
    CheckFlags, ElementFlags, IntersectionState, ModifierFlags, ObjectFlags, RecursionFlags,
    SymbolFlags, Ternary, TypeData, TypeFlags, TypeId, UnionReduction,
};

use crate::engine::{is_true, ternary_and, RelationChecker};
use crate::relate::RelationKind;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureKind {
    Call,
    Construct,
}

impl<'r, 'a> RelationChecker<'r, 'a> {
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
                    .get_apparent_type_m3(source)
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
            if source_flags.intersects(TypeFlags::from_bits(
                TypeFlags::INDEX.bits()
                    | TypeFlags::INDEXED_ACCESS.bits()
                    | TypeFlags::CONDITIONAL.bits()
                    | TypeFlags::SUBSTITUTION.bits()
                    | TypeFlags::STRING_MAPPING.bits(),
            )) {
                return Err(Unsupported::new(
                    "identity for index/indexed-access/conditional/substitution/string-mapping types (M4)",
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
        // Alias-variance block (66087-66101): alias symbols are M4.
        // Single-element generic tuple fast paths (66102-66104):
        // generic tuples are M4.
        if target_flags.intersects(TypeFlags::TYPE_PARAMETER) {
            return Err(Unsupported::new("type-parameter targets (M4 5.1)"));
        }
        if target_flags.intersects(TypeFlags::INDEX) {
            return Err(Unsupported::new("keyof targets (M4 5.2)"));
        }
        if target_flags.intersects(TypeFlags::INDEXED_ACCESS) {
            return Err(Unsupported::new("indexed-access targets (M4 5.2)"));
        }
        if self.is_generic_mapped_type(target) && self.relation != RelationKind::Identity {
            return Err(Unsupported::new("mapped-type targets (M4 5.2)"));
        }
        if target_flags.intersects(TypeFlags::CONDITIONAL) {
            return Err(Unsupported::new("conditional targets (M4 5.2)"));
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
                // instantiateType(source, reportUnreliableMapper):
                // variance-marker propagation, dead until M4 5.3b.
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
            return Err(Unsupported::new("type-variable sources (M4 5.1)"));
        }
        if source_flags.intersects(TypeFlags::INDEX) {
            return Err(Unsupported::new("keyof sources (M4 5.2)"));
        }
        if source_flags.intersects(TypeFlags::TEMPLATE_LITERAL)
            && !target_flags.intersects(TypeFlags::OBJECT)
        {
            if !target_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                // getBaseConstraintOfType(template) = stringType; the
                // string-vs-target simple rules already ran, so this
                // arm re-checks through the constraint.
                let string = self.st.tables.intrinsics.string;
                if string != source {
                    let related = self.is_related_to(
                        string,
                        target,
                        RecursionFlags::SOURCE,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    )?;
                    if is_true(related) {
                        return Ok(related);
                    }
                }
            }
        } else if source_flags.intersects(TypeFlags::STRING_MAPPING) {
            // 66345-66358.
            if target_flags.intersects(TypeFlags::STRING_MAPPING) {
                if self.st.tables.type_of(source).symbol != self.st.tables.type_of(target).symbol
                {
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
            return Err(Unsupported::new("conditional sources (M4 5.2)"));
        } else if !(source_flags.intersects(TypeFlags::TEMPLATE_LITERAL)
            && target_flags.intersects(TypeFlags::OBJECT)
            && false)
        {
            // Partial mapped targets (66404-66406) are M4. Generic
            // mapped sources/targets handled above.
            let source_is_primitive = source_flags.intersects(TypeFlags::PRIMITIVE);
            if self.relation != RelationKind::Identity {
                source = self.st.get_apparent_type_m3(source)?;
                source_flags = self.flags(source);
            }
            // Reference variance fast path (66418-66431): tuples are
            // excluded (!isTupleType) and no other same-target
            // references are constructible before M4. Readonly-array/
            // array target arms (66432-66438): global array types are
            // M4. Generic-tuple constraint arm (66439-66443): M4.
            // Subtype fresh-empty-target arm (66444-66446): Subtype
            // activates in 4.8, but the guard is ported faithfully.
            if (self.relation == RelationKind::Subtype
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

    fn template_parts(&self, ty: TypeId) -> (Vec<String>, Vec<TypeId>) {
        match &self.st.tables.type_of(ty).data {
            TypeData::TemplateLiteral { texts, types } => (texts.to_vec(), types.to_vec()),
            _ => unreachable!("template flag implies template data"),
        }
    }

    fn is_generic_mapped_type(&self, _ty: TypeId) -> bool {
        // Mapped types are unconstructible before M4 5.2.
        false
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
        } else if target_prop_flags.intersects(ModifierFlags::PROTECTED)
            || source_prop_flags.intersects(ModifierFlags::PROTECTED)
        {
            return Err(Unsupported::new("protected class members (M4 5.3)"));
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
            if self.st.tables.is_tuple_type(source) {
                let source_target = self.st.tables.reference_target(source);
                let target_target = self.st.tables.reference_target(target);
                let TypeData::TupleTarget(source_data) =
                    self.st.tables.type_of(source_target).data.clone()
                else {
                    unreachable!("tuple type targets a tuple target");
                };
                let TypeData::TupleTarget(target_data) =
                    self.st.tables.type_of(target_target).data.clone()
                else {
                    unreachable!("tuple type targets a tuple target");
                };
                if !target_data.readonly && source_data.readonly {
                    return Ok(Ternary::FALSE);
                }
                let source_arity = source_data.type_parameters.len();
                let target_arity = target_data.type_parameters.len();
                let source_rest_flag = source_data.combined_flags.intersects(ElementFlags::REST);
                let target_has_rest_element = target_data
                    .combined_flags
                    .intersects(ElementFlags::VARIABLE);
                let source_min_length = source_data.min_length;
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
                    let source_flags = source_data.element_flags[source_position];
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
                    if source_flags.intersects(ElementFlags::VARIADIC) {
                        return Err(Unsupported::new(
                            "variadic tuple elements in relations (M4 generic tuples)",
                        ));
                    }
                    let source_type = self.st.remove_missing_type(
                        source_type_arguments[source_position],
                        source_flags.intersects(ElementFlags::OPTIONAL)
                            && target_flags.intersects(ElementFlags::OPTIONAL),
                    );
                    let target_type = target_type_arguments[target_position];
                    let target_check_type = self.st.remove_missing_type(
                        target_type,
                        target_flags.intersects(ElementFlags::OPTIONAL),
                    );
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
            if self
                .st
                .tables
                .object_flags_of(source)
                .intersects(ObjectFlags::REFERENCE)
            {
                // Array sources are M4 (global Array); no other
                // references exist in M3.
                return Err(Unsupported::new("array-to-tuple relations (M4 5.3)"));
            }
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
    /// tsc-port: getUnmatchedProperty @6.0.3
    /// tsc-hash: 488e841fefc40d75aa7fa0d3f82f6cd689fd1b4098e12d1f9ce2ba7b050c1df3
    /// tsc-span: _tsc.js:68483-68485
    ///
    /// The generator collapses to first-match (relation callers only
    /// take the first); matchDiscriminantProperties is always false on
    /// this path. Static private identifier properties are M4 classes.
    fn get_unmatched_property(
        &mut self,
        source: TypeId,
        target: TypeId,
        require_optional_properties: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let properties = self.st.get_properties_of_type(target)?;
        for target_prop in properties {
            let flags = self.st.symbol_flags(target_prop);
            if require_optional_properties
                || !(flags.intersects(SymbolFlags::OPTIONAL)
                    || self
                        .st
                        .get_check_flags(target_prop)
                        .intersects(CheckFlags::PARTIAL))
            {
                let name = self.st.binder.symbol(target_prop).escaped_name.clone();
                if self.st.get_property_of_type_full(source, &name)?.is_none() {
                    return Ok(Some(target_prop));
                }
            }
        }
        Ok(None)
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
            return Err(Unsupported::new("private/protected members (M4 5.3)"));
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
    /// anyFunctionType and JS constructors are M4; the instantiated/
    /// same-reference erasure fast path has no M3 inputs (only tuple
    /// references exist, and they carry no signatures).
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
            // constructorVisibilitiesAreCompatible: accessibility
            // modifiers on constructors are M4 class members; type
            // annotation constructors are always public.
        }
        let mut result = Ternary::TRUE;
        if source_signatures.len() == 1 && target_signatures.len() == 1 {
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
    /// getErasedSignature is the identity without type parameters
    /// (generic signatures are M4).
    fn signature_related_to(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        _erase: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let check_mode = match self.relation {
            RelationKind::Subtype => check_mode::STRICT_TOP_SIGNATURE,
            RelationKind::StrictSubtype => {
                check_mode::STRICT_TOP_SIGNATURE | check_mode::STRICT_ARITY
            }
            _ => check_mode::NONE,
        };
        self.compare_signatures_related(source, target, check_mode, intersection_state)
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
    /// partialMatch is false on the identity path.
    fn is_matching_signature(
        &mut self,
        source: SignatureId,
        target: SignatureId,
    ) -> CheckResult2<bool> {
        let source_parameter_count = self.st.get_parameter_count(source)?;
        let target_parameter_count = self.st.get_parameter_count(target)?;
        let source_min = self.st.get_min_argument_count(source)?;
        let target_min = self.st.get_min_argument_count(target)?;
        let source_rest = self.st.has_effective_rest_parameter(source)?;
        let target_rest = self.st.has_effective_rest_parameter(target)?;
        Ok(source_parameter_count == target_parameter_count
            && source_min == target_min
            && source_rest == target_rest)
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
        let mut source = source;
        if source == target {
            return Ok(Ternary::TRUE);
        }
        if !self.is_matching_signature(source, target)? {
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
        let source_this = self.st.get_this_type_of_signature(source)?;
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
        self.st.get_type_predicate_of_signature(source)?;
        self.st.get_type_predicate_of_signature(target)?;
        let source_return = self.st.get_return_type_of_signature(source)?;
        let target_return = self.st.get_return_type_of_signature(target)?;
        let related = self.is_related_to(
            source_return,
            target_return,
            RecursionFlags::BOTH,
            /*report_errors*/ false,
            IntersectionState::NONE,
        )?;
        Ok(ternary_and(result, related))
    }

    /// tsc-port: compareSignaturesRelated @6.0.3
    /// tsc-hash: f0bf35ef85d54ae89a84377951424fb5b87b8ab55c8fc6ea30099c669d861e3b
    /// tsc-span: _tsc.js:64487-64605
    ///
    /// M3 dispositions: generic-signature instantiation (64505-64514)
    /// is M4; rest-parameter positions never construct (array rest
    /// annotations are M4), so getNonArrayRestType is None and the
    /// rest-index machinery is dead; the unreliable-marker
    /// instantiation is variance measurement (M4 5.3b). strictVariance
    /// keys on the target DECLARATION kind (method bivariance,
    /// core-interfaces §4 from_method).
    #[allow(clippy::only_used_in_recursion)] // intersectionState threads through the callback recursion as in tsc
    fn compare_signatures_related(
        &mut self,
        source: SignatureId,
        target: SignatureId,
        check_mode: i32,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if source == target {
            return Ok(Ternary::TRUE);
        }
        // 66727-66730: a generic source instantiates in the context of
        // the target (getCanonicalSignature + instantiateSignatureInContextOf
        // = M6 inference machinery). Signatures with typeParameters are
        // constructible since 5.2e; value-equal parameter lists only
        // arise from the interned same-signature case handled above.
        if self.st.signature_of(source).type_parameters.is_some()
            && self.st.signature_of(source).type_parameters
                != self.st.signature_of(target).type_parameters
        {
            return Err(Unsupported::new(
                "generic-signature relation (instantiateSignatureInContextOf, M6)",
            ));
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
        let source_count = self.st.get_parameter_count(source)?;
        // getNonArrayRestType: rest parameters are unconstructible in
        // M3 (their array annotations are M4), so both are None.
        let source_rest_type: Option<TypeId> = None;
        let target_rest_type: Option<TypeId> = None;
        let target_kind = {
            let declaration = self.st.signature_of(target).declaration;
            self.st.kind_of(declaration)
        };
        let strict_variance = check_mode & check_mode::CALLBACK == 0
            && self.st.strict_function_types
            && target_kind != SyntaxKind::MethodDeclaration
            && target_kind != SyntaxKind::MethodSignature
            && target_kind != SyntaxKind::Constructor;
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
                            IntersectionState::NONE,
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
                            IntersectionState::NONE,
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
        for i in 0..param_count {
            let source_type = self.st.try_get_type_at_position(source, i)?;
            let target_type = self.st.try_get_type_at_position(target, i)?;
            let (Some(source_type), Some(target_type)) = (source_type, target_type) else {
                continue;
            };
            if source_type != target_type || check_mode & check_mode::STRICT_ARITY != 0 {
                let source_sig = if check_mode & check_mode::CALLBACK != 0 {
                    None
                } else {
                    let non_nullable = self.st.remove_nullable_for_callback_gate(source_type);
                    self.st.get_single_call_signature(non_nullable)?
                };
                let target_sig = if check_mode & check_mode::CALLBACK != 0 {
                    None
                } else {
                    let non_nullable = self.st.remove_nullable_for_callback_gate(target_type);
                    self.st.get_single_call_signature(non_nullable)?
                };
                let callbacks = match (source_sig, target_sig) {
                    (Some(source_sig), Some(target_sig)) => {
                        self.st.get_type_predicate_of_signature(source_sig)?;
                        self.st.get_type_predicate_of_signature(target_sig)?;
                        self.st.undefined_null_facts(source_type)
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
                    )?
                } else {
                    let bivariant = if check_mode & check_mode::CALLBACK == 0 && !strict_variance {
                        self.is_related_to(
                            source_type,
                            target_type,
                            RecursionFlags::BOTH,
                            /*report_errors*/ false,
                            IntersectionState::NONE,
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
                            IntersectionState::NONE,
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
                        IntersectionState::NONE,
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
            // Type predicates report Unsupported until M5.
            self.st.get_type_predicate_of_signature(target)?;
            self.st.get_type_predicate_of_signature(source)?;
            let bivariant = if check_mode & check_mode::BIVARIANT_CALLBACK != 0 {
                self.is_related_to(
                    target_return_type,
                    source_return_type,
                    RecursionFlags::BOTH,
                    /*report_errors*/ false,
                    IntersectionState::NONE,
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
                    IntersectionState::NONE,
                )?
            };
            result = ternary_and(result, related);
        }
        Ok(result)
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
fn end_element_count(element_flags: &[ElementFlags], flags: ElementFlags) -> usize {
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
    /// tsc-port: getReducedApparentType @6.0.3
    /// tsc-hash: b1cf0cc54d00b7b1594d9b40a29bcf06e62f81238c47afb524d6e6f82a8f9ec3
    /// tsc-span: _tsc.js:59098-59100
    ///
    /// M3 slice: instantiable constraints are M4; primitive apparent
    /// types resolve through missing globals in the noLib world the
    /// probe shares with the oracle, i.e. the empty object type
    /// (getGlobalType failure → emptyObjectType; the 2318 diagnostics
    /// are program-level and invisible to fixture files). NonPrimitive
    /// → emptyObjectType is real tsc behavior. getReducedType is the
    /// ledgered identity stub (M4 5.3).
    pub fn get_apparent_type_m3(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INSTANTIABLE) {
            return Err(Unsupported::new("instantiable apparent types (M4 5.3)"));
        }
        if flags.intersects(TypeFlags::from_bits(
            TypeFlags::STRING_LIKE.bits()
                | TypeFlags::NUMBER_LIKE.bits()
                | TypeFlags::BIG_INT_LIKE.bits()
                | TypeFlags::BOOLEAN_LIKE.bits()
                | TypeFlags::ES_SYMBOL_LIKE.bits()
                | TypeFlags::NON_PRIMITIVE.bits(),
        )) {
            return Ok(self.empty_object_type);
        }
        if flags.intersects(TypeFlags::UNKNOWN) && !self.tables.strict_null_checks {
            return Ok(self.empty_object_type);
        }
        Ok(ty)
    }

    /// tsc-port: getPropertiesOfType @6.0.3
    /// tsc-hash: 24909f78d7ea360522b5188e5af3c7b09613e4dc2e455ea321c4ec054b4d7576
    /// tsc-span: _tsc.js:58745-58748
    pub fn get_properties_of_type_full(&mut self, ty: TypeId) -> CheckResult2<Vec<SymbolId>> {
        let reduced = self.get_apparent_type_m3(ty)?;
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
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(None);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let Some(symbol) = self.members_of(members).members.get(name).copied() else {
            return Ok(None);
        };
        if self.symbol_flags(symbol).intersects(SymbolFlags::VALUE) {
            Ok(Some(symbol))
        } else {
            Ok(None)
        }
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
        let reduced = self.get_apparent_type_m3(ty)?;
        let flags = self.tables.flags_of(reduced);
        if flags.intersects(TypeFlags::OBJECT) {
            return self.get_property_of_object_type(reduced, name);
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            return self.get_property_of_union_or_intersection_type(
                reduced, name, /*skip_object_function_property_augment*/ false,
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
        if let Some(&cached) = self.links.union_property_cache.get(&key) {
            return Ok(Some(cached));
        }
        let property = self.create_union_or_intersection_property(ty, name, skip)?;
        if let Some(property) = property {
            self.links.union_property_cache.insert(key, property);
        }
        Ok(property)
    }

    /// tsc-port: createUnionOrIntersectionProperty @6.0.3
    /// tsc-hash: 21791f74b0558b599db3de8950d26bd93152bbb62c3e250727503334167bf713
    /// tsc-span: _tsc.js:59101-59245
    ///
    /// M3 slice: no accessors, no private/protected/static members, no
    /// instantiation merging, no write types; the >2-constituent
    /// DeferredType branch computes eagerly (semantics identical, the
    /// deferral is a perf cache). Index fallbacks for union members
    /// route through the applicable-index machinery; tuple rest types
    /// are M4.
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
        for current in members {
            let ty = self.get_apparent_type_m3(current)?;
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
                self.get_property_of_object_type(ty, name)?
            };
            if let Some(prop) = prop {
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
                    None => single_prop = Some(prop),
                    Some(existing) if existing != prop => {
                        if prop_set.is_empty() {
                            prop_set.push(existing);
                        }
                        if !prop_set.contains(&prop) {
                            prop_set.push(prop);
                        }
                    }
                    _ => {}
                }
                if is_union && self.is_readonly_symbol(prop) {
                    check_flags |= CheckFlags::READONLY.bits();
                } else if !is_union && !self.is_readonly_symbol(prop) {
                    check_flags &= !CheckFlags::READONLY.bits();
                }
                check_flags |= CheckFlags::CONTAINS_PUBLIC.bits();
                if !self.is_prototype_property(prop) {
                    syntactic_flag = CheckFlags::SYNTHETIC_PROPERTY;
                }
            } else if is_union {
                let index_info = self.get_applicable_index_info_for_name_info(ty, name)?;
                if let Some(index_info) = index_info {
                    check_flags |= CheckFlags::WRITE_PARTIAL.bits()
                        | if index_info.is_readonly {
                            CheckFlags::READONLY.bits()
                        } else {
                            0
                        };
                    if self.tables.is_tuple_type(ty) {
                        return Err(Unsupported::new("tuple rest index fallbacks (M4)"));
                    }
                    index_types.push(index_info.value_type);
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
        {
            return Ok(None);
        }
        if prop_set.is_empty()
            && check_flags & CheckFlags::READ_PARTIAL.bits() == 0
            && index_types.is_empty()
        {
            return Ok(Some(single_prop));
        }
        let props = if prop_set.is_empty() {
            vec![single_prop]
        } else {
            prop_set
        };
        let mut declarations: Vec<tsrs2_syntax::NodeId> = Vec::new();
        let mut first_type: Option<TypeId> = None;
        let mut prop_types: Vec<TypeId> = Vec::new();
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
            SymbolFlags::PROPERTY.bits() | optional_flag.map(|f| f.bits()).unwrap_or(0),
        );
        let result = self.binder.create_symbol(flags, name.to_owned());
        {
            let symbol = self.binder.symbol_mut(result);
            symbol.declarations = declarations;
            if !has_non_uniform_value_declaration {
                symbol.value_declaration = first_value_declaration;
            }
        }
        let combined = if is_union {
            self.get_union_type_ex(&prop_types, UnionReduction::Literal)?
        } else {
            self.get_intersection_type(&prop_types, tsrs2_types::IntersectionFlags::NONE)?
        };
        self.links.set_symbol_synthetic(
            self.speculation_depth,
            result,
            CheckFlags::from_bits(syntactic_flag.bits() | check_flags),
            containing_type,
            combined,
        );
        Ok(Some(result))
    }

    fn is_prototype_property(&self, prop: SymbolId) -> bool {
        // tsc isPrototypeProperty: methods and prototype-flagged
        // symbols; M3 members are methods or plain properties.
        self.symbol_flags(prop).intersects(SymbolFlags::METHOD)
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
    /// M3 slice: synthetic Readonly check flags + readonly property
    /// modifiers; accessors, const variables and enum members are M4.
    pub fn is_readonly_symbol(&self, symbol: SymbolId) -> bool {
        if self
            .get_check_flags(symbol)
            .intersects(CheckFlags::READONLY)
        {
            return true;
        }
        if !self.symbol_flags(symbol).intersects(SymbolFlags::PROPERTY) {
            return false;
        }
        let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
            return false;
        };
        let modifiers = match &self
            .binder
            .source_of_node(declaration)
            .arena
            .node(declaration)
            .data
        {
            NodeData::PropertySignature(data) => data.modifiers,
            NodeData::Parameter(data) => data.modifiers,
            _ => None,
        };
        let Some(modifiers) = modifiers else {
            return false;
        };
        self.binder
            .node_array(modifiers)
            .nodes
            .iter()
            .any(|&m| self.kind_of(m) == SyntaxKind::ReadonlyKeyword)
    }

    /// getDeclarationModifierFlagsFromSymbol (17436), M3 slice: type
    /// members carry no accessibility/static modifiers; synthetics
    /// read the Contains* check flags.
    pub fn get_declaration_modifier_flags_from_symbol(&self, symbol: SymbolId) -> ModifierFlags {
        if self.binder.symbol(symbol).value_declaration.is_some() {
            return ModifierFlags::from_bits(0);
        }
        let check_flags = self.get_check_flags(symbol);
        if check_flags.intersects(CheckFlags::SYNTHETIC) {
            if check_flags.intersects(CheckFlags::CONTAINS_PRIVATE) {
                return ModifierFlags::PRIVATE;
            }
            if check_flags.intersects(CheckFlags::CONTAINS_PUBLIC) {
                return ModifierFlags::PUBLIC;
            }
            return ModifierFlags::PROTECTED;
        }
        ModifierFlags::from_bits(0)
    }

    /// tsc-port: isDiscriminantProperty @6.0.3
    /// tsc-hash: 1b3d6f14be2183682f24b21ec0f57e84975ced1cf03ab31db92b2b62388d6a8a
    /// tsc-span: _tsc.js:69562-69573
    fn is_discriminant_property(&mut self, ty: TypeId, name: &str) -> CheckResult2<bool> {
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
        let is_discriminant = check_flags.contains(CheckFlags::DISCRIMINANT);
        // !isGenericType(getTypeOfSymbol(prop)): generic types are M4.
        self.links.set_symbol_is_discriminant(prop, is_discriminant);
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
    /// Enum/ValueModule/ObjectRestType/ReverseMapped arms are M4.
    fn is_object_type_with_inferable_index(&mut self, ty: TypeId) -> CheckResult2<bool> {
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
        let reduced = self.get_apparent_type_m3(ty)?;
        // Tuple references have no signatures by construction
        // (createTupleTargetType 61198-61199: declaredCall/Construct
        // = emptyArray); resolving their members is M4 instantiation.
        if self.tables.is_tuple_type(reduced) {
            return Ok(Vec::new());
        }
        if self
            .tables
            .flags_of(reduced)
            .intersects(TypeFlags::INTERSECTION)
        {
            // resolveIntersectionTypeMembers concatenates constituent
            // signatures (58408+); the union-this-type mixing is M4.
            let TypeData::Intersection { types } = self.tables.type_of(reduced).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            let mut result = Vec::new();
            for t in types.iter() {
                result.extend(self.get_signatures_of_type(*t, kind)?);
            }
            return Ok(result);
        }
        if self.tables.flags_of(reduced).intersects(TypeFlags::UNION) {
            return Err(Unsupported::new("union signature resolution (M4)"));
        }
        if !self.tables.flags_of(reduced).intersects(TypeFlags::OBJECT) {
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

    /// The type-predicate gate: predicate-shaped return annotations
    /// (createTypePredicateFromTypePredicateNode) are M5 narrowing
    /// machinery; signatures carrying them report Unsupported instead
    /// of comparing as plain booleans.
    pub fn get_type_predicate_of_signature(&mut self, signature: SignatureId) -> CheckResult2<()> {
        let declaration = self.signature_of(signature).declaration;
        let annotation = match &self
            .binder
            .source_of_node(declaration)
            .arena
            .node(declaration)
            .data
        {
            NodeData::FunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            _ => None,
        };
        if annotation.is_some_and(|node| self.kind_of(node) == SyntaxKind::TypePredicate) {
            return Err(Unsupported::new("type predicates (M5 narrowing)"));
        }
        Ok(())
    }

    /// tsc-port: isTopSignature @6.0.3
    /// tsc-hash: 2deef363c847c3d8dd816c49efdf631572c8bd5cd017402e80809d986d917197
    /// tsc-span: _tsc.js:64479-64486
    ///
    /// Rest parameters never construct in M3 (array annotations are
    /// M4), so signatureHasRestParameter is false and the answer is
    /// always false — ported for the 4.8 Subtype activation.
    pub fn is_top_signature(&mut self, signature: SignatureId) -> CheckResult2<bool> {
        let signature_data = self.signature_of(signature);
        if signature_data.parameters.len() == 1
            && signature_data
                .flags
                .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Err(Unsupported::new("rest-parameter signatures (M4)"));
        }
        Ok(false)
    }

    // ---- arity helpers (78233-78341) ----

    /// tsc-port: getParameterCount @6.0.3
    /// tsc-hash: 88e24efd3edb09e7c4c597f52541cb2cc8bb745ffdfb9ddd606c00c0e7ecb9b7
    /// tsc-span: _tsc.js:78277-78286
    pub fn get_parameter_count(&mut self, signature: SignatureId) -> CheckResult2<usize> {
        let signature_data = self.signature_of(signature);
        let length = signature_data.parameters.len();
        if signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Err(Unsupported::new("rest-parameter signatures (M4)"));
        }
        Ok(length)
    }

    /// tsc-port: getMinArgumentCount @6.0.3
    /// tsc-hash: 7e615bfc72d73516124a8fd89a208b2ca2036519bbaf5b7cad73d2010b1ef3b8
    /// tsc-span: _tsc.js:78287-78321
    ///
    /// The tuple-rest branch is M4; the void-trimming loop lowers the
    /// syntactic count when trailing parameters accept void.
    pub fn get_min_argument_count(&mut self, signature: SignatureId) -> CheckResult2<usize> {
        let signature_data = self.signature_of(signature);
        if signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Err(Unsupported::new("rest-parameter signatures (M4)"));
        }
        let mut min_argument_count = signature_data.min_argument_count as usize;
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

    /// tsc-port: hasEffectiveRestParameter @6.0.3
    /// tsc-hash: 4545af73ef96a3c83fa4e089d6a10737e822c04c947fb002fbfa5e24fe93959f
    /// tsc-span: _tsc.js:78322-78328
    pub fn has_effective_rest_parameter(&mut self, signature: SignatureId) -> CheckResult2<bool> {
        if self
            .signature_of(signature)
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Err(Unsupported::new("rest-parameter signatures (M4)"));
        }
        Ok(false)
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
        if signature_data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER)
        {
            return Err(Unsupported::new("rest-parameter signatures (M4)"));
        }
        let parameters = signature_data.parameters.clone();
        if pos < parameters.len() {
            return Ok(Some(self.get_type_of_parameter(parameters[pos])?));
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
        let reduced = self.get_apparent_type_m3(ty)?;
        // Tuple references carry no index infos by construction
        // (createTupleTargetType 61200).
        if self.tables.is_tuple_type(reduced) {
            return Ok(Vec::new());
        }
        if self
            .tables
            .flags_of(reduced)
            .intersects(TypeFlags::INTERSECTION)
        {
            // resolveIntersectionTypeMembers: same-key infos combine
            // by intersecting value types (appendIndexInfo).
            let TypeData::Intersection { types } = self.tables.type_of(reduced).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            let mut combined: Vec<IndexInfo> = Vec::new();
            for t in types.iter() {
                for info in self.get_index_infos_of_type(*t)? {
                    if let Some(existing) = combined
                        .iter_mut()
                        .find(|existing| existing.key_type == info.key_type)
                    {
                        let value = self.get_intersection_type(
                            &[existing.value_type, info.value_type],
                            tsrs2_types::IntersectionFlags::NONE,
                        )?;
                        existing.value_type = value;
                        existing.is_readonly = existing.is_readonly && info.is_readonly;
                    } else {
                        combined.push(info);
                    }
                }
            }
            return Ok(combined);
        }
        if self.tables.flags_of(reduced).intersects(TypeFlags::UNION) {
            return Err(Unsupported::new("union index info resolution (M4)"));
        }
        if !self.tables.flags_of(reduced).intersects(TypeFlags::OBJECT) {
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
    fn get_applicable_index_info_for_name_info(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<IndexInfo>> {
        let name_type = if is_numeric_name(name) {
            let value: f64 = name
                .parse()
                .map_err(|_| Unsupported::new("unparsable numeric member name"))?;
            self.tables.get_number_literal_type(value)
        } else {
            self.tables.get_string_literal_type(name)
        };
        self.get_applicable_index_info(ty, name_type)
    }

    // ---- callback-parameter helpers ----

    /// getSingleCallSignature/getSingleSignature (75875) slice: exactly
    /// one call signature, nothing else on the type.
    pub fn get_single_call_signature(&mut self, ty: TypeId) -> CheckResult2<Option<SignatureId>> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(None);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        if resolved.call_signatures.len() == 1
            && resolved.construct_signatures.is_empty()
            && resolved.properties.is_empty()
            && resolved.index_infos.is_empty()
        {
            Ok(Some(resolved.call_signatures[0]))
        } else {
            Ok(None)
        }
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
        let source_start = utf16_units(&source_texts[0]);
        let target_start = utf16_units(&target_texts[0]);
        let source_end = utf16_units(&source_texts[source_texts.len() - 1]);
        let target_end = utf16_units(&target_texts[target_texts.len() - 1]);
        let start_len = source_start.len().min(target_start.len());
        let end_len = source_end.len().min(target_end.len());
        source_start[..start_len] != target_start[..start_len]
            || source_end[source_end.len() - end_len..] != target_end[target_end.len() - end_len..]
    }

    fn template_parts_of(&self, ty: TypeId) -> (Vec<String>, Vec<TypeId>) {
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
    fn is_valid_number_string(&self, s: &str, round_trip_only: bool) -> bool {
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

    /// tsc-port: isTypeMatchedByTemplateLiteralType @6.0.3
    /// tsc-hash: 10e3e6c09b4976cfec5a798ea4a9c37923362c263ea75bc20304a9a7a44b3379
    /// tsc-span: _tsc.js:68580-68583
    ///
    /// tsc-port: inferTypesFromTemplateLiteralType @6.0.3
    /// tsc-hash: 9abaf8ac4504967f931a9a1ac1ff06638761380afe56e951af5c860fd7ac9f3a
    /// tsc-span: _tsc.js:68575-68579
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

    fn infer_types_from_template_literal_type(
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
            return self.infer_from_literal_parts_to_template_literal(&[value], &[], target);
        }
        if source_flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let (source_texts, source_types) = self.template_parts_of(source);
            let (target_texts, _) = self.template_parts_of(target);
            if source_texts == target_texts {
                let mut mapped = Vec::with_capacity(source_types.len());
                let (_, target_types) = self.template_parts_of(target);
                for (i, &s) in source_types.iter().enumerate() {
                    // getBaseConstraintOrType is the identity for
                    // non-instantiable types (M3).
                    if self.is_type_assignable_to(s, target_types[i])? {
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
    /// The bigint arm needs scanner-grade isValidBigIntString — M6
    /// expression machinery; bigint placeholders report Unsupported.
    /// StringMapping arms are M4.
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
            if target_flags.intersects(TypeFlags::BIG_INT) {
                return Err(Unsupported::new(
                    "bigint template placeholders (isValidBigIntString, M6)",
                ));
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
        source_texts: &[String],
        source_types: &[TypeId],
        target: TypeId,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let source_units: Vec<Vec<u16>> = source_texts.iter().map(|t| utf16_units(t)).collect();
        let last_source_index = source_texts.len() - 1;
        let (target_texts, _) = self.template_parts_of(target);
        let target_units: Vec<Vec<u16>> = target_texts.iter().map(|t| utf16_units(t)).collect();
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
                    let mut texts = vec![utf16_to_string(&source_units[seg][pos..])?];
                    texts.extend(source_texts[seg + 1..s].iter().cloned());
                    texts.push(utf16_to_string(&get_source_units(s)[..p])?);
                    let types = source_types[seg..s].to_vec();
                    self.tables.get_template_literal_type(&texts, &types)
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

/// JS string indexing operates on UTF-16 code units; the template
/// matcher does all its cursor arithmetic in that domain.
fn utf16_units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
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
    String::from_utf16(units)
        .map_err(|_| Unsupported::new("template inference strands a surrogate half (UTF-16)"))
}

/// tsc isNumericLiteralName over JS number round-trip (19205): the
/// name coerces to a number whose string form is the name.
fn is_numeric_literal_name_js(name: &str) -> bool {
    match js_string_to_number(name) {
        Some(n) => js_number_to_string(n) == name,
        None => false,
    }
}

/// The `+s` coercion slice: trimmed decimal/exponent/hex/infinity
/// forms (full JS ToNumber is M6 with expression checking).
fn js_string_to_number(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).ok().map(|v| v as f64);
    }
    if trimmed == "Infinity" || trimmed == "+Infinity" {
        return Some(f64::INFINITY);
    }
    if trimmed == "-Infinity" {
        return Some(f64::NEG_INFINITY);
    }
    trimmed.parse::<f64>().ok().filter(|n| !n.is_nan())
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
        let mut state = CheckerState::new(&source, binder, &options);
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
}

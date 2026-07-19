//! The relation engine core (m3-types-relations-steps.md stage 4.5,
//! checker-key-functions §1.1-§1.3).
//!
//! Every function returns CheckResult2<Ternary>: an Unsupported means
//! "this query needs machinery from a later stage" and is NEVER cached
//! or converted into a verdict. structuredTypeRelatedTo is the stage
//! 4.6 boundary — at 4.5 it reports itself as the blocker, so pins
//! that resolve through entry fast paths, simple rules and
//! union/intersection dispatch flip green while structural pins stay
//! honestly unsupported.
//!
//! Error REPORTING is deliberately absent (M3 captures verdicts only;
//! chain shaping is T2 work): the probe calls checkTypeRelatedTo with
//! no error node, which is exactly tsc's reportErrors=false mode, so
//! every error-construction block in the source is a no-op here. The
//! error-path recursions that influence VERDICTS do not exist —
//! reportErrorResults and friends only shape diagnostics.

use std::collections::HashSet;

use tsrs2_types::{
    ExpandingFlags, IntersectionState, ObjectFlags, RecursionFlags, RelationComparisonResult,
    SymbolFlags, Ternary, TypeData, TypeFlags, TypeId, UnionReduction,
};

use tsrs2_syntax::NodeId;

use crate::relate::RelationKind;
use crate::state::{CheckResult2, CheckerState, Unsupported};

/// stableTypeOrdering off: binary search keyed by type id over the
/// id-sorted member list (tsc containsType 61327).
pub(crate) fn contains_type(types: &[TypeId], ty: TypeId) -> bool {
    types.binary_search(&ty).is_ok()
}

pub(crate) fn ternary_and(left: Ternary, right: Ternary) -> Ternary {
    Ternary::from_bits(left.bits() & right.bits())
}

pub(crate) fn is_false(result: Ternary) -> bool {
    result == Ternary::FALSE
}

pub(crate) fn is_true(result: Ternary) -> bool {
    result.bits() != 0
}

impl<'a> CheckerState<'a> {
    /// The public boolean APIs the relpin probe (and later checker
    /// call sites) consume.
    pub fn is_type_assignable_to(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        self.is_type_related_to(source, target, RelationKind::Assignable)
    }

    pub fn is_type_comparable_to(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        self.is_type_related_to(source, target, RelationKind::Comparable)
    }

    /// tsc-port: compareTypesIdentical @6.0.3
    /// tsc-hash: 7c4196c179ced7ce413c377112393d84081a4e6835ec31db3da27e82a5222567
    /// tsc-span: _tsc.js:63904-63906
    pub(crate) fn compare_types_identical(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<Ternary> {
        Ok(
            if self.is_type_related_to(source, target, RelationKind::Identity)? {
                Ternary::TRUE
            } else {
                Ternary::FALSE
            },
        )
    }

    /// tsc-port: isTypeRelatedTo @6.0.3
    /// tsc-hash: d24d4e98079949fbb174096cb6dc8fa844f20def414250bc4e6da95157846086
    /// tsc-span: _tsc.js:64762-64803
    ///
    /// Occurrence 1 of 3 of the comparable reversed-simple rule
    /// (64773; the others live in isRelatedTo at 65150/65197).
    pub fn is_type_related_to(
        &mut self,
        mut source: TypeId,
        mut target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<bool> {
        if self.tables.is_fresh_literal_type(source) {
            source = self
                .tables
                .type_of(source)
                .regular_type
                .expect("fresh literals have regular links");
        }
        if self.tables.is_fresh_literal_type(target) {
            target = self
                .tables
                .type_of(target)
                .regular_type
                .expect("fresh literals have regular links");
        }
        if source == target {
            return Ok(true);
        }
        if relation != RelationKind::Identity {
            if (relation == RelationKind::Comparable
                && !self.tables.flags_of(target).intersects(TypeFlags::NEVER)
                && self.is_simple_type_related_to(target, source, relation)?)
                || self.is_simple_type_related_to(source, target, relation)?
            {
                return Ok(true);
            }
        } else if !(self.tables.flags_of(source) | self.tables.flags_of(target)).intersects(
            TypeFlags::from_bits(
                TypeFlags::UNION_OR_INTERSECTION.bits()
                    | TypeFlags::INDEXED_ACCESS.bits()
                    | TypeFlags::CONDITIONAL.bits()
                    | TypeFlags::SUBSTITUTION.bits(),
            ),
        ) {
            if self.tables.flags_of(source) != self.tables.flags_of(target) {
                return Ok(false);
            }
            if self
                .tables
                .flags_of(source)
                .intersects(TypeFlags::SINGLETON)
            {
                return Ok(true);
            }
        }
        if self.tables.flags_of(source).intersects(TypeFlags::OBJECT)
            && self.tables.flags_of(target).intersects(TypeFlags::OBJECT)
        {
            let key = self.get_relation_key(
                source,
                target,
                IntersectionState::NONE,
                relation,
                /*ignore_constraints*/ false,
            )?;
            if let Some(&related) = self.relations.cache(relation).get(&key) {
                return Ok(related.intersects(RelationComparisonResult::SUCCEEDED));
            }
        }
        if self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
            || self
                .tables
                .flags_of(target)
                .intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
        {
            return self.check_type_related_to(source, target, relation);
        }
        Ok(false)
    }

    /// tsc-port: isSimpleTypeRelatedTo @6.0.3
    /// tsc-hash: 364b4fccbfeccb19bc4a4e11466fd7343a2e454b11f86fd9ef244dc4f26ba72b
    /// tsc-span: _tsc.js:64733-64761
    ///
    /// The enum arms route through isEnumTypeRelatedTo (relate.rs)
    /// with its enumRelation symbol-pair cache (M4 5.3b).
    pub fn is_simple_type_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<bool> {
        let s = self.tables.flags_of(source).bits();
        let t = self.tables.flags_of(target).bits();
        if t & TypeFlags::ANY.bits() != 0
            || s & TypeFlags::NEVER.bits() != 0
            || source == self.tables.intrinsics.wildcard
        {
            return Ok(true);
        }
        if t & TypeFlags::UNKNOWN.bits() != 0
            && !(relation == RelationKind::StrictSubtype && s & TypeFlags::ANY.bits() != 0)
        {
            return Ok(true);
        }
        if t & TypeFlags::NEVER.bits() != 0 {
            return Ok(false);
        }
        if s & TypeFlags::STRING_LIKE.bits() != 0 && t & TypeFlags::STRING.bits() != 0 {
            return Ok(true);
        }
        if s & TypeFlags::STRING_LITERAL.bits() != 0
            && s & TypeFlags::ENUM_LITERAL.bits() != 0
            && t & TypeFlags::STRING_LITERAL.bits() != 0
            && t & TypeFlags::ENUM_LITERAL.bits() == 0
            && self.literal_values_equal(source, target)
        {
            return Ok(true);
        }
        if s & TypeFlags::NUMBER_LIKE.bits() != 0 && t & TypeFlags::NUMBER.bits() != 0 {
            return Ok(true);
        }
        if s & TypeFlags::NUMBER_LITERAL.bits() != 0
            && s & TypeFlags::ENUM_LITERAL.bits() != 0
            && t & TypeFlags::NUMBER_LITERAL.bits() != 0
            && t & TypeFlags::ENUM_LITERAL.bits() == 0
            && self.literal_values_equal(source, target)
        {
            return Ok(true);
        }
        if s & TypeFlags::BIG_INT_LIKE.bits() != 0 && t & TypeFlags::BIG_INT.bits() != 0 {
            return Ok(true);
        }
        if s & TypeFlags::BOOLEAN_LIKE.bits() != 0 && t & TypeFlags::BOOLEAN.bits() != 0 {
            return Ok(true);
        }
        if s & TypeFlags::ES_SYMBOL_LIKE.bits() != 0 && t & TypeFlags::ES_SYMBOL.bits() != 0 {
            return Ok(true);
        }
        if s & TypeFlags::ENUM.bits() != 0 && t & TypeFlags::ENUM.bits() != 0 {
            let source_symbol = self
                .tables
                .type_of(source)
                .symbol
                .expect("enum types carry their symbol");
            let target_symbol = self
                .tables
                .type_of(target)
                .symbol
                .expect("enum types carry their symbol");
            if self.binder.symbol(source_symbol).escaped_name
                == self.binder.symbol(target_symbol).escaped_name
                && self.is_enum_type_related_to(source_symbol, target_symbol)?
            {
                return Ok(true);
            }
        }
        if s & TypeFlags::ENUM_LITERAL.bits() != 0 && t & TypeFlags::ENUM_LITERAL.bits() != 0 {
            let source_symbol = self
                .tables
                .type_of(source)
                .symbol
                .expect("enum literal types carry their symbol");
            let target_symbol = self
                .tables
                .type_of(target)
                .symbol
                .expect("enum literal types carry their symbol");
            if s & TypeFlags::UNION.bits() != 0
                && t & TypeFlags::UNION.bits() != 0
                && self.is_enum_type_related_to(source_symbol, target_symbol)?
            {
                return Ok(true);
            }
            if s & TypeFlags::LITERAL.bits() != 0
                && t & TypeFlags::LITERAL.bits() != 0
                && self.literal_values_equal(source, target)
                && self.is_enum_type_related_to(source_symbol, target_symbol)?
            {
                return Ok(true);
            }
        }
        if s & TypeFlags::UNDEFINED.bits() != 0
            && ((!self.tables.strict_null_checks
                && t & TypeFlags::UNION_OR_INTERSECTION.bits() == 0)
                || t & (TypeFlags::UNDEFINED.bits() | TypeFlags::VOID.bits()) != 0)
        {
            return Ok(true);
        }
        if s & TypeFlags::NULL.bits() != 0
            && ((!self.tables.strict_null_checks
                && t & TypeFlags::UNION_OR_INTERSECTION.bits() == 0)
                || t & TypeFlags::NULL.bits() != 0)
        {
            return Ok(true);
        }
        if s & TypeFlags::OBJECT.bits() != 0 && t & TypeFlags::NON_PRIMITIVE.bits() != 0 {
            let strict_subtype_exclusion = relation == RelationKind::StrictSubtype
                && self.is_empty_anonymous_object_type(source)?
                && !self
                    .tables
                    .object_flags_of(source)
                    .intersects(ObjectFlags::FRESH_LITERAL);
            if !strict_subtype_exclusion {
                return Ok(true);
            }
        }
        if relation == RelationKind::Assignable || relation == RelationKind::Comparable {
            if s & TypeFlags::ANY.bits() != 0 {
                return Ok(true);
            }
            if s & TypeFlags::NUMBER.bits() != 0
                && (t & TypeFlags::ENUM.bits() != 0
                    || (t & TypeFlags::NUMBER_LITERAL.bits() != 0
                        && t & TypeFlags::ENUM_LITERAL.bits() != 0))
            {
                return Ok(true);
            }
            if s & TypeFlags::NUMBER_LITERAL.bits() != 0
                && s & TypeFlags::ENUM_LITERAL.bits() == 0
                && (t & TypeFlags::ENUM.bits() != 0
                    || (t & TypeFlags::NUMBER_LITERAL.bits() != 0
                        && t & TypeFlags::ENUM_LITERAL.bits() != 0
                        && self.literal_values_equal(source, target)))
            {
                return Ok(true);
            }
            if self.is_unknown_like_union_type(target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn literal_values_equal(&self, source: TypeId, target: TypeId) -> bool {
        match (
            &self.tables.type_of(source).data,
            &self.tables.type_of(target).data,
        ) {
            (TypeData::Literal { value: source }, TypeData::Literal { value: target }) => {
                source == target
            }
            _ => false,
        }
    }

    /// tsc-port: isUnknownLikeUnionType @6.0.3
    /// tsc-hash: db66676e0affd408e748429ddd64881c82c0a42b92b120e2c54f6726f6e3fed4
    /// tsc-span: _tsc.js:64653-64662
    fn is_unknown_like_union_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self.tables.strict_null_checks || !self.tables.flags_of(ty).intersects(TypeFlags::UNION)
        {
            return Ok(false);
        }
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::IS_UNKNOWN_LIKE_UNION_COMPUTED)
        {
            let TypeData::Union { types, .. } = self.tables.type_of(ty).data.clone() else {
                unreachable!("union flag implies union data");
            };
            let is_unknown_like = types.len() >= 3
                && self
                    .tables
                    .flags_of(types[0])
                    .intersects(TypeFlags::UNDEFINED)
                && self.tables.flags_of(types[1]).intersects(TypeFlags::NULL)
                && {
                    let mut any_empty = false;
                    for &t in types.iter() {
                        if self.is_empty_anonymous_object_type(t)? {
                            any_empty = true;
                            break;
                        }
                    }
                    any_empty
                };
            let new_flags = self.tables.object_flags_of(ty).bits()
                | ObjectFlags::IS_UNKNOWN_LIKE_UNION_COMPUTED.bits()
                | if is_unknown_like {
                    ObjectFlags::IS_UNKNOWN_LIKE_UNION.bits()
                } else {
                    0
                };
            self.tables.type_mut(ty).object_flags = ObjectFlags::from_bits(new_flags);
        }
        Ok(self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::IS_UNKNOWN_LIKE_UNION))
    }

    /// tsc-port: getNormalizedType @6.0.3
    /// tsc-hash: 15b1858f4522bdb103fa55510c586405f260560a3545e142a6a032ea8ac0b792
    /// tsc-span: _tsc.js:64807-64813
    ///
    /// Deferred (node-carrying) references normalize to their eager
    /// twin (createNormalizedTypeReference over the forced arguments);
    /// the Substitution arm fails closed until its base/constraint
    /// payload and reading/writing normalization land in M8.
    pub fn get_normalized_type(&mut self, mut ty: TypeId, writing: bool) -> CheckResult2<TypeId> {
        loop {
            let flags = self.tables.flags_of(ty);
            let t = if self.tables.is_fresh_literal_type(ty) {
                self.tables
                    .type_of(ty)
                    .regular_type
                    .expect("fresh literals have regular links")
            } else if self.is_generic_tuple_type(ty) {
                self.get_normalized_tuple_type(ty, writing)?
            } else if self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::REFERENCE)
                && !matches!(self.tables.type_of(ty).data, TypeData::TupleTarget(_))
            {
                if self.links.ty(ty).deferred_node.is_some() {
                    let target = self.tables.reference_target(ty);
                    let arguments = self.get_type_arguments(ty)?;
                    self.create_normalized_type_reference_forced(target, &arguments)?
                } else {
                    self.get_single_base_for_non_augmenting_subtype(ty)?
                        .unwrap_or(ty)
                }
            } else if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
                self.get_normalized_union_or_intersection_type(ty, writing)?
            } else if flags.intersects(TypeFlags::SUBSTITUTION) {
                return Err(Unsupported::new(
                    "getNormalizedType for substitution types (unported family, M8-stub)",
                ));
            } else if flags.intersects(TypeFlags::SIMPLIFIABLE) {
                self.get_simplified_type(ty, writing)?
            } else {
                ty
            };
            if t == ty {
                return Ok(t);
            }
            ty = t;
        }
    }

    /// tsc-port: getSingleBaseForNonAugmentingSubtype @6.0.3
    /// tsc-hash: 587ed64c85dc88067b489e77dd0d80526ba1e2da68c3476319f36cb8f840205e
    /// tsc-span: _tsc.js:67686-67714
    ///
    /// The class base-type-node kind gate (67695-67700) sits before
    /// class bases exist — class members are 5.3e, but the gate is
    /// syntax-only and ports now.
    pub(crate) fn get_single_base_for_non_augmenting_subtype(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(None);
        }
        let target = self.tables.reference_target(ty);
        if !self
            .tables
            .object_flags_of(target)
            .intersects(ObjectFlags::CLASS_OR_INTERFACE)
        {
            return Ok(None);
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::IDENTICAL_BASE_TYPE_CALCULATED)
        {
            return Ok(
                if self
                    .tables
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::IDENTICAL_BASE_TYPE_EXISTS)
                {
                    self.links.ty(ty).cached_equivalent_base_type
                } else {
                    None
                },
            );
        }
        let with_calculated = self.tables.object_flags_of(ty).bits()
            | ObjectFlags::IDENTICAL_BASE_TYPE_CALCULATED.bits();
        self.tables.type_mut(ty).object_flags = ObjectFlags::from_bits(with_calculated);
        if self
            .tables
            .type_of(target)
            .symbol
            .is_some_and(|symbol| self.symbol_flags(symbol).intersects(SymbolFlags::CLASS))
        {
            // 67695-67700: the collapse only applies when the extends
            // expression is a plain identifier/property access.
            if let Some(base_type_node) = self.get_base_type_node_of_class(target) {
                let expression = match &self
                    .binder
                    .source_of_node(base_type_node)
                    .arena
                    .node(base_type_node)
                    .data
                {
                    tsrs2_syntax::NodeData::ExpressionWithTypeArguments(data) => data.expression,
                    _ => None,
                };
                let is_simple = expression.is_some_and(|expression| {
                    matches!(
                        self.kind_of(expression),
                        tsrs2_syntax::SyntaxKind::Identifier
                            | tsrs2_syntax::SyntaxKind::PropertyAccessExpression
                    )
                });
                if !is_simple {
                    return Ok(None);
                }
            }
        }
        let bases = self.get_base_types(target)?;
        if bases.len() != 1 {
            return Ok(None);
        }
        let target_symbol = self
            .tables
            .type_of(target)
            .symbol
            .expect("class/interface targets carry their symbol");
        if !self.get_members_of_symbol(target_symbol)?.is_empty() {
            return Ok(None);
        }
        let target_parameters: Vec<TypeId> = match &self.tables.type_of(target).data {
            TypeData::GenericType {
                type_parameters, ..
            } => type_parameters.to_vec(),
            _ => Vec::new(),
        };
        let mut instantiated_base = if target_parameters.is_empty() {
            bases[0]
        } else {
            let arguments = self.get_type_arguments(ty)?;
            let mapper = self.create_type_mapper(
                target_parameters.clone(),
                Some(arguments[..target_parameters.len()].to_vec()),
            );
            self.instantiate_type(bases[0], Some(mapper))?
        };
        let arguments = self.get_type_arguments(ty)?;
        if arguments.len() > target_parameters.len() {
            let last = *arguments.last().expect("nonempty argument list");
            instantiated_base = self.get_type_with_this_argument(
                instantiated_base,
                Some(last),
                /*need_apparent_type*/ false,
            )?;
        }
        let with_exists =
            self.tables.object_flags_of(ty).bits() | ObjectFlags::IDENTICAL_BASE_TYPE_EXISTS.bits();
        self.tables.type_mut(ty).object_flags = ObjectFlags::from_bits(with_exists);
        self.links.ty_mut_cached_equivalent_base_type(
            self.speculation_depth,
            ty,
            instantiated_base,
        );
        Ok(Some(instantiated_base))
    }

    /// tsc-port: getNormalizedUnionOrIntersectionType @6.0.3
    /// tsc-hash: 3dae128d6a34d9e3525686ca6159a04b73c849d6f3c552641a7f2f6c2e03cbb1
    /// tsc-span: _tsc.js:64814-64826
    ///
    /// getReducedType is REAL (structural.rs; the review pin
    /// `{kind:"a"}&{kind:"b"}` → `{q:number}` caught the identity
    /// stub); getReducedApparentType stays M4 5.3.
    fn get_normalized_union_or_intersection_type(
        &mut self,
        ty: TypeId,
        writing: bool,
    ) -> CheckResult2<TypeId> {
        let reduced = self.get_reduced_type(ty)?;
        if reduced != ty {
            return Ok(reduced);
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION)
            && self.should_normalize_intersection(ty)?
        {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            let mut normalized = Vec::with_capacity(types.len());
            let mut changed = false;
            for &t in types.iter() {
                let n = self.get_normalized_type(t, writing)?;
                changed |= n != t;
                normalized.push(n);
            }
            if changed {
                return self
                    .get_intersection_type(&normalized, tsrs2_types::IntersectionFlags::NONE);
            }
        }
        Ok(ty)
    }

    /// tsc-port: shouldNormalizeIntersection @6.0.3
    /// tsc-hash: ac2985b3bcb9ba84f303ae672a68613c34772919f604484580cb8cb21b69edf1
    /// tsc-span: _tsc.js:64827-64836
    fn should_normalize_intersection(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
            unreachable!("intersection flag implies intersection data");
        };
        let mut has_instantiable = false;
        let mut has_nullable_or_empty = false;
        for &t in types.iter() {
            has_instantiable |= self.tables.flags_of(t).intersects(TypeFlags::INSTANTIABLE);
            has_nullable_or_empty |= self.tables.flags_of(t).intersects(TypeFlags::NULLABLE)
                || self.is_empty_anonymous_object_type(t)?;
            if has_instantiable && has_nullable_or_empty {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc isGenericTupleType (67794) — the tables twin holds the body
    /// (getGenericObjectFlags reads it there since M4 5.2).
    pub(crate) fn is_generic_tuple_type(&self, ty: TypeId) -> bool {
        self.tables.is_generic_tuple_type(ty)
    }

    /// tsc-port: getNormalizedTupleType @6.0.3
    /// tsc-hash: 5e3b7a86845b858ca8bb7bc77565a77978ae9c5b794e1c5bf53bd572dac4bec3
    /// tsc-span: _tsc.js:64837-64841
    ///
    /// The checker keeps its eager re-expansion of the tuple reference,
    /// but first simplifies every SIMPLIFIABLE element in the requested
    /// reading/writing direction, matching tsc's sameMap step.
    fn get_normalized_tuple_type(&mut self, ty: TypeId, writing: bool) -> CheckResult2<TypeId> {
        let target = self.tables.reference_target(ty);
        // getTypeArguments (64838) — deferred tuple references force
        // lazily; the wrapper pre-forces variadic elements for the
        // tables re-expansion.
        let arity = match &self.tables.type_of(target).data {
            TypeData::TupleTarget(data) => data.type_parameters.len(),
            _ => unreachable!("generic tuple targets a tuple target"),
        };
        let mut elements = self.get_type_arguments(ty)?;
        elements.truncate(arity);
        for element in &mut elements {
            if self
                .tables
                .flags_of(*element)
                .intersects(TypeFlags::SIMPLIFIABLE)
            {
                *element = self.get_simplified_type(*element, writing)?;
            }
        }
        self.create_normalized_type_reference_forced(target, &elements)
    }

    /// tsc-port: checkTypeRelatedTo @6.0.3
    /// tsc-hash: 3ce5a7fee9a3a4c896a2e34f5f2d8ff2e385b952cc67b32c7e4ec94fd8565fc8
    /// tsc-span: _tsc.js:64842-67230
    ///
    /// M3 slice: verdicts only. The probe passes no error node, which
    /// is tsc's reportErrors=false mode — the error machinery
    /// (errorInfo chains, incompatibleStack, elaborations) never runs
    /// and is deferred to T2 diagnostics work. The overflow path DOES
    /// cache Failed|ComplexityOverflow / Failed|StackDepthOverflow
    /// exactly like 64872-64882 (diagnostics 2859/2321 deferred with
    /// error reporting).
    pub fn check_type_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        relation: RelationKind,
    ) -> CheckResult2<bool> {
        let relation_count = (16_000_000 - self.relations.cache(relation).len() as i64) >> 3;
        let mut checker = RelationChecker {
            st: self,
            relation,
            maybe_keys: Vec::new(),
            maybe_keys_set: HashSet::new(),
            source_stack: Vec::new(),
            target_stack: Vec::new(),
            maybe_count: 0,
            source_depth: 0,
            target_depth: 0,
            expanding_flags: ExpandingFlags::NONE,
            overflow: false,
            relation_count,
        };
        let result = checker.is_related_to(
            source,
            target,
            RecursionFlags::BOTH,
            /*report_errors*/ false,
            IntersectionState::NONE,
        )?;
        if checker.overflow {
            let overflow_bits = if checker.relation_count <= 0 {
                RelationComparisonResult::COMPLEXITY_OVERFLOW
            } else {
                RelationComparisonResult::STACK_DEPTH_OVERFLOW
            };
            let id = self.get_relation_key(
                source,
                target,
                IntersectionState::NONE,
                relation,
                /*ignore_constraints*/ false,
            )?;
            self.relations.cache_mut(relation).insert(
                id,
                RelationComparisonResult::from_bits(
                    RelationComparisonResult::FAILED.bits() | overflow_bits.bits(),
                ),
            );
        }
        Ok(is_true(result))
    }
}

/// The checkTypeRelatedTo closure state (maybe stack, recursion
/// stacks, complexity budget) — checker-key §1.2's four invariants:
/// Maybe results are never cached mid-recursion, commit happens on
/// unwind of the outermost frame, relationCount bounds complexity,
/// depth 100 per side hard-cuts recursion.
pub(crate) struct RelationChecker<'r, 'a> {
    pub(crate) st: &'r mut CheckerState<'a>,
    pub(crate) relation: RelationKind,
    pub(crate) maybe_keys: Vec<String>,
    pub(crate) maybe_keys_set: HashSet<String>,
    pub(crate) source_stack: Vec<TypeId>,
    pub(crate) target_stack: Vec<TypeId>,
    pub(crate) maybe_count: usize,
    pub(crate) source_depth: usize,
    pub(crate) target_depth: usize,
    pub(crate) expanding_flags: ExpandingFlags,
    pub(crate) overflow: bool,
    pub(crate) relation_count: i64,
}

impl<'r, 'a> RelationChecker<'r, 'a> {
    pub(crate) fn flags(&self, ty: TypeId) -> TypeFlags {
        self.st.tables.flags_of(ty)
    }

    pub(crate) fn union_members(&self, ty: TypeId) -> Vec<TypeId> {
        match &self.st.tables.type_of(ty).data {
            TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("member access on a non-union/intersection"),
        }
    }

    /// tsc-port: isRelatedTo @6.0.3
    /// tsc-hash: 47404a0f4ae9f5c59d0f972b7e338adbd1a9cbb8419eae7187f6415cd1d68fd5
    /// tsc-span: _tsc.js:65147-65247
    ///
    /// Occurrences 2 and 3 of the comparable reversed-simple rule live
    /// here: the object-vs-primitive entry (65150) and the
    /// post-normalization simple check (65197). The excess-property
    /// and weak-type checks live HERE (65199/65208), before
    /// union/intersection dispatch — NOT in propertiesRelatedTo.
    pub(crate) fn is_related_to(
        &mut self,
        original_source: TypeId,
        original_target: TypeId,
        recursion_flags: RecursionFlags,
        report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if original_source == original_target {
            return Ok(Ternary::TRUE);
        }
        if self.flags(original_source).intersects(TypeFlags::OBJECT)
            && self.flags(original_target).intersects(TypeFlags::PRIMITIVE)
        {
            if (self.relation == RelationKind::Comparable
                && !self.flags(original_target).intersects(TypeFlags::NEVER)
                && self.st.is_simple_type_related_to(
                    original_target,
                    original_source,
                    self.relation,
                )?)
                || self.st.is_simple_type_related_to(
                    original_source,
                    original_target,
                    self.relation,
                )?
            {
                return Ok(Ternary::TRUE);
            }
            return Ok(Ternary::FALSE);
        }
        let source = self
            .st
            .get_normalized_type(original_source, /*writing*/ false)?;
        let mut target = self
            .st
            .get_normalized_type(original_target, /*writing*/ true)?;
        if source == target {
            return Ok(Ternary::TRUE);
        }
        if self.relation == RelationKind::Identity {
            if self.flags(source) != self.flags(target) {
                return Ok(Ternary::FALSE);
            }
            if self.flags(source).intersects(TypeFlags::SINGLETON) {
                return Ok(Ternary::TRUE);
            }
            return self.recursive_type_related_to(
                source,
                target,
                /*report_errors*/ false,
                IntersectionState::NONE,
                recursion_flags,
            );
        }
        // Type-parameter-equals-constraint fast path (65181): M3 type
        // parameters are tuple synthetics whose constraint is None, so
        // this reads the stored constraint directly.
        if self.flags(source).intersects(TypeFlags::TYPE_PARAMETER) {
            if let TypeData::TypeParameter {
                constraint: Some(constraint),
                ..
            } = &self.st.tables.type_of(source).data
            {
                if *constraint == target {
                    return Ok(Ternary::TRUE);
                }
            }
        }
        if self
            .flags(source)
            .intersects(TypeFlags::DEFINITELY_NON_NULLABLE)
            && self.flags(target).intersects(TypeFlags::UNION)
        {
            let types = self.union_members(target);
            let candidate =
                if types.len() == 2 && self.flags(types[0]).intersects(TypeFlags::NULLABLE) {
                    Some(types[1])
                } else if types.len() == 3
                    && self.flags(types[0]).intersects(TypeFlags::NULLABLE)
                    && self.flags(types[1]).intersects(TypeFlags::NULLABLE)
                {
                    Some(types[2])
                } else {
                    None
                };
            if let Some(candidate) = candidate {
                if !self.flags(candidate).intersects(TypeFlags::NULLABLE) {
                    target = self.st.get_normalized_type(candidate, /*writing*/ true)?;
                    if source == target {
                        return Ok(Ternary::TRUE);
                    }
                }
            }
        }
        if (self.relation == RelationKind::Comparable
            && !self.flags(target).intersects(TypeFlags::NEVER)
            && self
                .st
                .is_simple_type_related_to(target, source, self.relation)?)
            || self
                .st
                .is_simple_type_related_to(source, target, self.relation)?
        {
            return Ok(Ternary::TRUE);
        }
        if self
            .flags(source)
            .intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
            || self
                .flags(target)
                .intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE)
        {
            let is_performing_excess_property_checks = !intersection_state
                .intersects(IntersectionState::TARGET)
                && self.st.is_object_literal_type(source)
                && self
                    .st
                    .tables
                    .object_flags_of(source)
                    .intersects(ObjectFlags::FRESH_LITERAL);
            if is_performing_excess_property_checks && self.has_excess_properties(source, target)? {
                return Ok(Ternary::FALSE);
            }
            let is_performing_common_property_checks = (self.relation != RelationKind::Comparable
                || self.st.is_unit_type(source))
                && !intersection_state.intersects(IntersectionState::TARGET)
                && self.flags(source).intersects(TypeFlags::from_bits(
                    TypeFlags::PRIMITIVE.bits()
                        | TypeFlags::OBJECT.bits()
                        | TypeFlags::INTERSECTION.bits(),
                ))
                && source != self.st.global_object_type()?
                && self.flags(target).intersects(TypeFlags::from_bits(
                    TypeFlags::OBJECT.bits() | TypeFlags::INTERSECTION.bits(),
                ))
                && self.st.is_weak_type(target)?
                && (!self.st.get_properties_of_type(source)?.is_empty()
                    || self.st.type_has_call_or_construct_signatures(source)?);
            if is_performing_common_property_checks
                && !self.st.has_common_properties(source, target)?
            {
                return Ok(Ternary::FALSE);
            }
            let skip_caching = (self.flags(source).intersects(TypeFlags::UNION)
                && self.union_members(source).len() < 4
                && !self.flags(target).intersects(TypeFlags::UNION))
                || (self.flags(target).intersects(TypeFlags::UNION)
                    && self.union_members(target).len() < 4
                    && !self
                        .flags(source)
                        .intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE));
            let result = if skip_caching {
                self.union_or_intersection_related_to(
                    source,
                    target,
                    report_errors,
                    intersection_state,
                )?
            } else {
                self.recursive_type_related_to(
                    source,
                    target,
                    report_errors,
                    intersection_state,
                    recursion_flags,
                )?
            };
            if is_true(result) {
                return Ok(result);
            }
        }
        Ok(Ternary::FALSE)
    }

    /// tsc-port: hasExcessProperties @6.0.3
    /// tsc-hash: 2feb57fb3012195ec298b8373aae179205e425727845272eac7ef6231ed69cc7
    /// tsc-span: _tsc.js:65347-65410
    ///
    /// tsc-port: shouldCheckAsExcessProperty @6.0.3
    /// tsc-hash: c8dc3058980bc1ec2f14c28bd887a86aa6b6419cef8d42dbebc1728006d1ec6d
    /// tsc-span: _tsc.js:65411-65413
    ///
    /// The subset-of head consults the synthesized empty anonymous
    /// type where tsc reads globalObjectType (an M3 noLib-probe
    /// stand-in that outlived M4 lib loading — the "until M4"
    /// justification lapsed). KNOWN-GAP since M4 (m4-review B2): the
    /// JSX arms (isComparingJsxAttributes + the JSX-flavored reports)
    /// are missing while jsx.rs constructs JSX_ATTRIBUTES types and
    /// feeds them to the relation machinery — the old "JSX arms are
    /// dead" claim is false.
    fn has_excess_properties(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        if !self.st.is_excess_property_check_target(target)
            || self
                .st
                .tables
                .object_flags_of(target)
                .intersects(ObjectFlags::JS_LITERAL)
        {
            return Ok(false);
        }
        if (self.relation == RelationKind::Assignable || self.relation == RelationKind::Comparable)
            && (self
                .st
                .is_type_subset_of(self.st.empty_object_type, target)?
                || self.st.is_empty_object_type(target)?)
        {
            return Ok(false);
        }
        let mut reduced_target = target;
        let mut check_types: Option<Vec<TypeId>> = None;
        if self.flags(target).intersects(TypeFlags::UNION) {
            reduced_target = match self.find_matching_discriminant_type(source, target)? {
                Some(discriminant) => discriminant,
                None => self.st.filter_primitives_if_contains_non_primitive(target),
            };
            check_types = Some(if self.flags(reduced_target).intersects(TypeFlags::UNION) {
                self.union_members(reduced_target)
            } else {
                vec![reduced_target]
            });
        }
        let source_symbol = self.st.tables.type_of(source).symbol;
        for prop in self.st.get_properties_of_type(source)? {
            if self.should_check_as_excess_property(prop, source_symbol) {
                let name = self.st.binder.symbol(prop).escaped_name.clone();
                if !self.st.is_known_property(reduced_target, &name)? {
                    return Ok(true);
                }
                if let Some(check_types) = &check_types {
                    let prop_type = self.st.get_type_of_symbol(prop)?;
                    let target_prop_type =
                        self.get_type_of_property_in_types(check_types.clone(), &name)?;
                    if !is_true(self.is_related_to(
                        prop_type,
                        target_prop_type,
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    )?) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn should_check_as_excess_property(
        &self,
        prop: tsrs2_binder::SymbolId,
        container: Option<tsrs2_binder::SymbolId>,
    ) -> bool {
        let Some(container) = container else {
            return false;
        };
        let prop_declaration = self.st.binder.symbol(prop).value_declaration;
        let container_declaration = self.st.binder.symbol(container).value_declaration;
        match (prop_declaration, container_declaration) {
            (Some(prop_declaration), Some(container_declaration)) => {
                self.st
                    .binder
                    .source_of_node(prop_declaration)
                    .arena
                    .node(prop_declaration)
                    .parent
                    == Some(container_declaration)
            }
            _ => false,
        }
    }

    /// tsc-port: getTypeOfPropertyInTypes @6.0.3
    /// tsc-hash: c9ac69cbc7b688113c52b401fd2f3f9bfc51f883792a0ce5250f12e1fd6bd128
    /// tsc-span: _tsc.js:65332-65346
    ///
    /// Union constituents can themselves be unions or intersections
    /// (the review pin `({a}&{b})|{c}` caught the object-only slice
    /// fabricating undefinedType), so the property lookup branches on
    /// UnionOrIntersection exactly as 65336 does.
    pub(crate) fn get_type_of_property_in_types(
        &mut self,
        types: Vec<TypeId>,
        name: &str,
    ) -> CheckResult2<TypeId> {
        let mut prop_types = Vec::with_capacity(types.len());
        for ty in types {
            let apparent = self.st.get_apparent_type(ty)?;
            let prop = if self
                .st
                .tables
                .flags_of(apparent)
                .intersects(TypeFlags::UNION_OR_INTERSECTION)
            {
                self.st
                    .get_property_of_union_or_intersection_type(apparent, name, false)?
            } else {
                self.st.get_property_of_object_type(apparent, name)?
            };
            let prop_type = match prop {
                Some(prop) => self.st.get_type_of_symbol(prop)?,
                None => match self.st.get_applicable_index_info_for_name(apparent, name)? {
                    Some(index_type) => index_type,
                    None => self.st.tables.intrinsics.undefined,
                },
            };
            prop_types.push(prop_type);
        }
        self.st
            .get_union_type_ex(&prop_types, UnionReduction::Literal)
    }

    /// tsc-port: findMatchingDiscriminantType @6.0.3
    /// tsc-hash: e07652728870af7f3805be6bccca52c6ee84e585ffe0f0357a2c25c15d97a973
    /// tsc-span: _tsc.js:90518-90536
    ///
    /// Both arms are live (the review pin `{kind:"a",x:1,y:2}` →
    /// discriminated union caught the missing
    /// discriminateTypeByDiscriminableItems arm returning a wrong
    /// Related); `isRelatedTo` is the closure argument, so the port
    /// lives on RelationChecker.
    pub(crate) fn find_matching_discriminant_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.flags(target).intersects(TypeFlags::UNION)
            && self.flags(source).intersects(TypeFlags::from_bits(
                TypeFlags::INTERSECTION.bits() | TypeFlags::OBJECT.bits(),
            ))
        {
            if let Some(matched) = self
                .st
                .get_matching_union_constituent_for_type(target, source)?
            {
                return Ok(Some(matched));
            }
            let source_properties = self.st.get_properties_of_type(source)?;
            if let Some(filtered) = self
                .st
                .find_discriminant_properties(&source_properties, target)?
            {
                let discriminated =
                    self.discriminate_type_by_discriminable_items(target, &filtered)?;
                if discriminated != target {
                    return Ok(Some(discriminated));
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: unionOrIntersectionRelatedTo @6.0.3
    /// tsc-hash: 2b6ff8e453c7e405f85843c53237bbaa689bc3452e0b9d7da8c90f9e57331414
    /// tsc-span: _tsc.js:65414-65465
    ///
    /// The comparable instantiable-constraint replacement (65440-65460)
    /// keys on Instantiable members — unconstructible before M4, so
    /// sameMap is the identity and the branch falls through as in tsc.
    pub(crate) fn union_or_intersection_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        if self.flags(source).intersects(TypeFlags::UNION) {
            if self.flags(target).intersects(TypeFlags::UNION) {
                // Named-union origin fast paths (65416-65426) key on
                // alias symbols — M4; both are None in M3.
            }
            return if self.relation == RelationKind::Comparable {
                self.some_type_related_to_type(
                    source,
                    target,
                    report_errors && !self.flags(source).intersects(TypeFlags::PRIMITIVE),
                    intersection_state,
                )
            } else {
                self.each_type_related_to_type(
                    source,
                    target,
                    report_errors && !self.flags(source).intersects(TypeFlags::PRIMITIVE),
                    intersection_state,
                )
            };
        }
        if self.flags(target).intersects(TypeFlags::UNION) {
            let regular_source = self.st.get_regular_type_of_object_literal(source)?;
            return self.type_related_to_some_type(regular_source, target, intersection_state);
        }
        if self.flags(target).intersects(TypeFlags::INTERSECTION) {
            return self.type_related_to_each_type(source, target, IntersectionState::TARGET);
        }
        // 65433-65456: for comparability against a primitive, an
        // intersection source steps its instantiable constituents to
        // their base constraints first; a non-intersection result
        // compares in both directions.
        let mut source = source;
        if self.relation == RelationKind::Comparable
            && self.flags(target).intersects(TypeFlags::PRIMITIVE)
        {
            let members = self.union_members(source);
            let mut constraints = Vec::with_capacity(members.len());
            let mut changed = false;
            for &member in &members {
                let constraint = if self.flags(member).intersects(TypeFlags::INSTANTIABLE) {
                    let base = self.st.get_base_constraint_of_type(member)?;
                    base.unwrap_or(self.st.tables.intrinsics.unknown)
                } else {
                    member
                };
                if constraint != member {
                    changed = true;
                }
                constraints.push(constraint);
            }
            if changed {
                source = self
                    .st
                    .get_intersection_type(&constraints, tsrs2_types::IntersectionFlags::NONE)?;
                if self.flags(source).intersects(TypeFlags::NEVER) {
                    return Ok(Ternary::FALSE);
                }
                if !self.flags(source).intersects(TypeFlags::INTERSECTION) {
                    let result = self.is_related_to(
                        source,
                        target,
                        RecursionFlags::SOURCE,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    )?;
                    if !is_false(result) {
                        return Ok(result);
                    }
                    return self.is_related_to(
                        target,
                        source,
                        RecursionFlags::SOURCE,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    );
                }
            }
        }
        self.some_type_related_to_type(
            source,
            target,
            /*report_errors*/ false,
            IntersectionState::SOURCE,
        )
    }

    /// Consumed by the 4.6 structural arms (structuredTypeRelatedTo's
    /// source-union dispatch); ported with its family per the steps
    /// doc.
    #[allow(dead_code)]
    /// tsc-port: eachTypeRelatedToSomeType @6.0.3
    /// tsc-hash: b9bb261a94173a03379c5bc2d6e22817708d020d37d3922ecd5556d86b7eda94
    /// tsc-span: _tsc.js:65466-65483
    pub(crate) fn each_type_related_to_some_type(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<Ternary> {
        let mut result = Ternary::TRUE;
        for source_type in self.union_members(source) {
            let related =
                self.type_related_to_some_type(source_type, target, IntersectionState::NONE)?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: typeRelatedToSomeType @6.0.3
    /// tsc-hash: 60b1f206bc16f1514cf3b15ef5285f32cb1049c794249abdad2cc0be24424b46
    /// tsc-span: _tsc.js:65484-65543
    ///
    /// getBestMatchingType (error elaboration) is dead without error
    /// reporting.
    fn type_related_to_some_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let target_types = self.union_members(target);
        if self.flags(target).intersects(TypeFlags::UNION) {
            if contains_type(&target_types, source) {
                return Ok(Ternary::TRUE);
            }
            let source_flags = self.flags(source).bits();
            if self.relation != RelationKind::Comparable
                && self
                    .st
                    .tables
                    .object_flags_of(target)
                    .intersects(ObjectFlags::PRIMITIVE_UNION)
                && source_flags & TypeFlags::ENUM_LITERAL.bits() == 0
                && (source_flags
                    & (TypeFlags::STRING_LITERAL.bits()
                        | TypeFlags::BOOLEAN_LITERAL.bits()
                        | TypeFlags::BIG_INT_LITERAL.bits())
                    != 0
                    || ((self.relation == RelationKind::Subtype
                        || self.relation == RelationKind::StrictSubtype)
                        && source_flags & TypeFlags::NUMBER_LITERAL.bits() != 0))
            {
                let source_type = self.st.tables.type_of(source);
                let alternate_form = if Some(source) == source_type.regular_type {
                    source_type.fresh_type
                } else {
                    source_type.regular_type
                };
                let primitive = if source_flags & TypeFlags::STRING_LITERAL.bits() != 0 {
                    Some(self.st.tables.intrinsics.string)
                } else if source_flags & TypeFlags::NUMBER_LITERAL.bits() != 0 {
                    Some(self.st.tables.intrinsics.number)
                } else if source_flags & TypeFlags::BIG_INT_LITERAL.bits() != 0 {
                    Some(self.st.tables.intrinsics.bigint)
                } else {
                    None
                };
                let matched = primitive.is_some_and(|p| contains_type(&target_types, p))
                    || alternate_form.is_some_and(|a| contains_type(&target_types, a));
                return Ok(if matched {
                    Ternary::TRUE
                } else {
                    Ternary::FALSE
                });
            }
            if let Some(matched) = self
                .st
                .get_matching_union_constituent_for_type(target, source)?
            {
                let related = self.is_related_to(
                    source,
                    matched,
                    RecursionFlags::TARGET,
                    /*report_errors*/ false,
                    intersection_state,
                )?;
                if is_true(related) {
                    return Ok(related);
                }
            }
        }
        for ty in target_types {
            let related = self.is_related_to(
                source,
                ty,
                RecursionFlags::TARGET,
                /*report_errors*/ false,
                intersection_state,
            )?;
            if is_true(related) {
                return Ok(related);
            }
        }
        Ok(Ternary::FALSE)
    }

    /// tsc-port: typeRelatedToEachType @6.0.3
    /// tsc-hash: 765c5043d4861a0824a4234be5ef00f60eb9fbe48e444270eda51957e9dad8c2
    /// tsc-span: _tsc.js:65544-65563
    fn type_related_to_each_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let mut result = Ternary::TRUE;
        for target_type in self.union_members(target) {
            let related = self.is_related_to(
                source,
                target_type,
                RecursionFlags::TARGET,
                /*report_errors*/ false,
                intersection_state,
            )?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: someTypeRelatedToType @6.0.3
    /// tsc-hash: 1d847d64530df31f378de687a582848df74fb7d0643c74f51646142a6a0b52da
    /// tsc-span: _tsc.js:65564-65585
    fn some_type_related_to_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        _report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let source_types = self.union_members(source);
        if self.flags(source).intersects(TypeFlags::UNION) && contains_type(&source_types, target) {
            return Ok(Ternary::TRUE);
        }
        for source_type in source_types {
            let related = self.is_related_to(
                source_type,
                target,
                RecursionFlags::SOURCE,
                /*report_errors*/ false,
                intersection_state,
            )?;
            if is_true(related) {
                return Ok(related);
            }
        }
        Ok(Ternary::FALSE)
    }

    /// tsc-port: getUndefinedStrippedTargetIfNeeded @6.0.3
    /// tsc-hash: 45e8325a55be5a972853ecc64b0da3218d6a633e6d2087f63b706a5499a2d645
    /// tsc-span: _tsc.js:65586-65591
    fn get_undefined_stripped_target_if_needed(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> TypeId {
        if self.flags(source).intersects(TypeFlags::UNION)
            && self.flags(target).intersects(TypeFlags::UNION)
        {
            let source_types = self.union_members(source);
            let target_types = self.union_members(target);
            if !self.flags(source_types[0]).intersects(TypeFlags::UNDEFINED)
                && self.flags(target_types[0]).intersects(TypeFlags::UNDEFINED)
            {
                return self.st.tables.filter_type(target, |tables, t| {
                    !tables.flags_of(t).intersects(TypeFlags::UNDEFINED)
                });
            }
        }
        target
    }

    /// tsc-port: eachTypeRelatedToType @6.0.3
    /// tsc-hash: 47835eb4cc21aa44bca8d8ab92f0ffc03902a74c0a8dfc924d255f361f45de2a
    /// tsc-span: _tsc.js:65592-65629
    fn each_type_related_to_type(
        &mut self,
        source: TypeId,
        target: TypeId,
        _report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        let mut result = Ternary::TRUE;
        let source_types = self.union_members(source);
        let undefined_stripped_target =
            self.get_undefined_stripped_target_if_needed(source, target);
        for (i, &source_type) in source_types.iter().enumerate() {
            if self
                .flags(undefined_stripped_target)
                .intersects(TypeFlags::UNION)
            {
                let stripped_types = self.union_members(undefined_stripped_target);
                if source_types.len() >= stripped_types.len()
                    && source_types.len().is_multiple_of(stripped_types.len())
                {
                    let related = self.is_related_to(
                        source_type,
                        stripped_types[i % stripped_types.len()],
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        intersection_state,
                    )?;
                    if is_true(related) {
                        result = ternary_and(result, related);
                        continue;
                    }
                }
            }
            let related = self.is_related_to(
                source_type,
                target,
                RecursionFlags::SOURCE,
                /*report_errors*/ false,
                intersection_state,
            )?;
            if !is_true(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    /// tsc-port: recursiveTypeRelatedTo @6.0.3
    /// tsc-hash: c01e16ed6704c2e6750b4db767394bc24c68f6fdc6a15238ce6cb1ca77a1de10
    /// tsc-span: _tsc.js:65725-65871
    ///
    /// THE maybe stack, with checker-key §1.2's four invariants. The
    /// cache-hit variance-replay branch (65744-65750, instantiateType
    /// against reportUnmeasurable/reportUnreliable mappers) is only
    /// reachable during variance measurement — dead until M4 5.3b, as
    /// is the propagatingVarianceFlags accumulation (outofband handler
    /// is never installed in M3). An Unsupported unwinds the stacks
    /// exactly like a False WITHOUT caching anything.
    /// tsc-port: typeArgumentsRelatedTo @6.0.3
    /// tsc-hash: 48c9af2e688dd0f7f130ca540d0bd3d9202216800c1de96f5c3f5cacdf2be4e3
    /// tsc-span: _tsc.js:65630-65724
    pub(crate) fn type_arguments_related_to(
        &mut self,
        sources: &[TypeId],
        targets: &[TypeId],
        variances: &[tsrs2_types::VarianceFlags],
        report_errors: bool,
        intersection_state: IntersectionState,
    ) -> CheckResult2<Ternary> {
        use tsrs2_types::VarianceFlags;
        if sources.len() != targets.len() && self.relation == RelationKind::Identity {
            return Ok(Ternary::FALSE);
        }
        let length = sources.len().min(targets.len());
        let mut result = Ternary::TRUE;
        for i in 0..length {
            let variance_flags = if i < variances.len() {
                variances[i]
            } else {
                VarianceFlags::COVARIANT
            };
            let variance = variance_flags.bits() & VarianceFlags::VARIANCE_MASK.bits();
            if variance == VarianceFlags::INDEPENDENT.bits() {
                continue;
            }
            let s = sources[i];
            let t = targets[i];
            let related;
            if variance_flags.intersects(VarianceFlags::UNMEASURABLE) {
                // Unmeasurable arguments compare identically (or as
                // mutually related under the identity relation).
                related = if self.relation == RelationKind::Identity {
                    self.is_related_to(
                        s,
                        t,
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    )?
                } else {
                    self.st.compare_types_identical(s, t)?
                };
            } else {
                if self.st.in_variance_computation
                    && variance_flags.intersects(VarianceFlags::UNRELIABLE)
                {
                    let mapper = self.st.report_unreliable_mapper;
                    self.st.instantiate_type(s, Some(mapper))?;
                }
                if variance == VarianceFlags::COVARIANT.bits() {
                    related = self.is_related_to(
                        s,
                        t,
                        RecursionFlags::BOTH,
                        report_errors,
                        intersection_state,
                    )?;
                } else if variance == VarianceFlags::CONTRAVARIANT.bits() {
                    related = self.is_related_to(
                        t,
                        s,
                        RecursionFlags::BOTH,
                        report_errors,
                        intersection_state,
                    )?;
                } else if variance == VarianceFlags::BIVARIANT.bits() {
                    let backwards = self.is_related_to(
                        t,
                        s,
                        RecursionFlags::BOTH,
                        /*report_errors*/ false,
                        IntersectionState::NONE,
                    )?;
                    related = if !is_false(backwards) {
                        backwards
                    } else {
                        self.is_related_to(
                            s,
                            t,
                            RecursionFlags::BOTH,
                            report_errors,
                            intersection_state,
                        )?
                    };
                } else {
                    // Invariant: related in both directions.
                    let forward = self.is_related_to(
                        s,
                        t,
                        RecursionFlags::BOTH,
                        report_errors,
                        intersection_state,
                    )?;
                    related = if !is_false(forward) {
                        ternary_and(
                            forward,
                            self.is_related_to(
                                t,
                                s,
                                RecursionFlags::BOTH,
                                report_errors,
                                intersection_state,
                            )?,
                        )
                    } else {
                        forward
                    };
                }
            }
            if is_false(related) {
                return Ok(Ternary::FALSE);
            }
            result = ternary_and(result, related);
        }
        Ok(result)
    }

    pub(crate) fn recursive_type_related_to(
        &mut self,
        source: TypeId,
        target: TypeId,
        report_errors: bool,
        intersection_state: IntersectionState,
        recursion_flags: RecursionFlags,
    ) -> CheckResult2<Ternary> {
        if self.overflow {
            return Ok(Ternary::FALSE);
        }
        let id = self.st.get_relation_key(
            source,
            target,
            intersection_state,
            self.relation,
            /*ignore_constraints*/ false,
        )?;
        if let Some(&entry) = self.st.relations.cache(self.relation).get(&id) {
            // The reportErrors && Failed && !Overflow recompute
            // (65740) and the overflow error emission (65751-65755)
            // are reportErrors-only — dead in the reportErrors=false
            // engine.
            if !self.st.variance_handler_stack.is_empty() {
                // 65742-65750: replay the entry's Reports* bits into
                // the active handler via the reporter mappers.
                let saved = entry.bits() & RelationComparisonResult::REPORTS_MASK.bits();
                if saved & RelationComparisonResult::REPORTS_UNMEASURABLE.bits() != 0 {
                    let mapper = self.st.report_unmeasurable_mapper;
                    self.st.instantiate_type(source, Some(mapper))?;
                }
                if saved & RelationComparisonResult::REPORTS_UNRELIABLE.bits() != 0 {
                    let mapper = self.st.report_unreliable_mapper;
                    self.st.instantiate_type(source, Some(mapper))?;
                }
            }
            return Ok(if entry.intersects(RelationComparisonResult::SUCCEEDED) {
                Ternary::TRUE
            } else {
                Ternary::FALSE
            });
        }
        if self.relation_count <= 0 {
            self.overflow = true;
            return Ok(Ternary::FALSE);
        }
        if !self.maybe_keys.is_empty() {
            if self.maybe_keys_set.contains(&id) {
                return Ok(Ternary::MAYBE);
            }
            if id.starts_with('*') {
                let broadest = self.st.get_relation_key(
                    source,
                    target,
                    intersection_state,
                    self.relation,
                    /*ignore_constraints*/ true,
                )?;
                if self.maybe_keys_set.contains(&broadest) {
                    return Ok(Ternary::MAYBE);
                }
            }
            if self.source_depth == 100 || self.target_depth == 100 {
                self.overflow = true;
                return Ok(Ternary::FALSE);
            }
        }
        let maybe_start = self.maybe_count;
        if self.maybe_keys.len() == self.maybe_count {
            self.maybe_keys.push(id.clone());
        } else {
            self.maybe_keys[self.maybe_count] = id.clone();
        }
        self.maybe_keys_set.insert(id.clone());
        self.maybe_count += 1;
        let save_expanding_flags = self.expanding_flags;
        if recursion_flags.intersects(RecursionFlags::SOURCE) {
            if self.source_stack.len() == self.source_depth {
                self.source_stack.push(source);
            } else {
                self.source_stack[self.source_depth] = source;
            }
            self.source_depth += 1;
            if !self.expanding_flags.intersects(ExpandingFlags::SOURCE)
                && self
                    .st
                    .is_deeply_nested_type(source, &self.source_stack, self.source_depth, 3)
            {
                self.expanding_flags = ExpandingFlags::from_bits(
                    self.expanding_flags.bits() | ExpandingFlags::SOURCE.bits(),
                );
            }
        }
        if recursion_flags.intersects(RecursionFlags::TARGET) {
            if self.target_stack.len() == self.target_depth {
                self.target_stack.push(target);
            } else {
                self.target_stack[self.target_depth] = target;
            }
            self.target_depth += 1;
            if !self.expanding_flags.intersects(ExpandingFlags::TARGET)
                && self
                    .st
                    .is_deeply_nested_type(target, &self.target_stack, self.target_depth, 3)
            {
                self.expanding_flags = ExpandingFlags::from_bits(
                    self.expanding_flags.bits() | ExpandingFlags::TARGET.bits(),
                );
            }
        }
        // 65803-65810: wrap the active handler with a propagating
        // accumulator — only when a handler exists, like tsc's
        // `if (outofbandVarianceMarkerHandler)` gate.
        let pushed_handler =
            if !self.st.variance_handler_stack.is_empty() {
                self.st.variance_handler_stack.push(
                    crate::state::VarianceHandlerFrame::Propagating(RelationComparisonResult::NONE),
                );
                true
            } else {
                false
            };
        let outcome = if self.expanding_flags == ExpandingFlags::BOTH {
            Ok(Ternary::MAYBE)
        } else {
            self.structured_type_related_to(source, target, report_errors, intersection_state)
        };
        // 65828-65830: restore the handler — on the Err unwind too.
        let propagating_variance_flags = if pushed_handler {
            match self.st.variance_handler_stack.pop() {
                Some(crate::state::VarianceHandlerFrame::Propagating(flags)) => flags,
                _ => unreachable!("the propagating frame pushed above is still on top"),
            }
        } else {
            RelationComparisonResult::NONE
        };
        if recursion_flags.intersects(RecursionFlags::SOURCE) {
            self.source_depth -= 1;
        }
        if recursion_flags.intersects(RecursionFlags::TARGET) {
            self.target_depth -= 1;
        }
        self.expanding_flags = save_expanding_flags;
        let result = match outcome {
            Ok(result) => result,
            Err(err) => {
                // Unsupported: unwind this frame's maybe entries
                // without caching any verdict.
                self.reset_maybe_stack(
                    maybe_start,
                    /*mark_all_as_succeeded*/ false,
                    propagating_variance_flags,
                );
                return Err(err);
            }
        };
        if is_true(result) || result == Ternary::MAYBE {
            if result == Ternary::TRUE || (self.source_depth == 0 && self.target_depth == 0) {
                if result == Ternary::TRUE || result == Ternary::MAYBE {
                    self.reset_maybe_stack(
                        maybe_start,
                        /*mark_all_as_succeeded*/ true,
                        propagating_variance_flags,
                    );
                } else {
                    self.reset_maybe_stack(
                        maybe_start,
                        /*mark_all_as_succeeded*/ false,
                        propagating_variance_flags,
                    );
                }
            }
        } else {
            self.st.relations.cache_mut(self.relation).insert(
                id,
                RelationComparisonResult::from_bits(
                    RelationComparisonResult::FAILED.bits() | propagating_variance_flags.bits(),
                ),
            );
            self.relation_count -= 1;
            self.reset_maybe_stack(
                maybe_start,
                /*mark_all_as_succeeded*/ false,
                propagating_variance_flags,
            );
        }
        Ok(result)
    }

    fn reset_maybe_stack(
        &mut self,
        maybe_start: usize,
        mark_all_as_succeeded: bool,
        propagating_variance_flags: RelationComparisonResult,
    ) {
        for i in maybe_start..self.maybe_count {
            let key = self.maybe_keys[i].clone();
            self.maybe_keys_set.remove(&key);
            if mark_all_as_succeeded {
                self.st.relations.cache_mut(self.relation).insert(
                    key,
                    RelationComparisonResult::from_bits(
                        RelationComparisonResult::SUCCEEDED.bits()
                            | propagating_variance_flags.bits(),
                    ),
                );
                self.relation_count -= 1;
            }
        }
        self.maybe_count = maybe_start;
    }
}

impl<'a> CheckerState<'a> {
    // ---- relation-entry helpers ----

    /// tsc-port: isObjectLiteralType @6.0.3
    /// tsc-hash: c47c60152d783c315c9e05b4a0ca88eeafc1a6dd81e68f5db92af14310588f65
    /// tsc-span: _tsc.js:69244-69246
    pub fn is_object_literal_type(&self, ty: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::OBJECT_LITERAL)
    }

    /// tsc-port: isUnitType @6.0.3
    /// tsc-hash: 6e636909716f4ae0c717befd065338d9a01ff626a72801bdab9aa76a44aae2e8
    /// tsc-span: _tsc.js:67742-67744
    pub fn is_unit_type(&self, ty: TypeId) -> bool {
        self.tables.flags_of(ty).intersects(TypeFlags::UNIT)
    }

    /// tsc-port: isLiteralType @6.0.3
    /// tsc-hash: 9be296b7217cffdc581f6f6d9a2af5c9e29b1ab668b6a135bc64ab5b2906f774
    /// tsc-span: _tsc.js:67752-67754
    pub(crate) fn is_literal_type(&self, ty: TypeId) -> bool {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::BOOLEAN) {
            return true;
        }
        if flags.intersects(TypeFlags::UNION) {
            if flags.intersects(TypeFlags::ENUM_LITERAL) {
                return true;
            }
            let TypeData::Union { types, .. } = &self.tables.type_of(ty).data else {
                unreachable!("union flag implies union data");
            };
            return types.iter().all(|&t| self.is_unit_type(t));
        }
        self.is_unit_type(ty)
    }

    /// tsc-port: isWeakType @6.0.3
    /// tsc-hash: 1953585bcdc5c930273bb00a55255f46d4aec77870ba0b1ba4526e965052b8dc
    /// tsc-span: _tsc.js:67285-67297
    ///
    /// The Substitution arm is dead until M4.
    pub fn is_weak_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        // Tuple references resolve members through
        // resolveTypeReferenceMembers (M4 5.3); every tuple has the
        // required `length` property (createTupleTargetType
        // 61177-61185), so the weak verdict is false by construction.
        if self.tables.is_tuple_type(ty) {
            return Ok(false);
        }
        if flags.intersects(TypeFlags::OBJECT) {
            let members = self.resolve_structured_type_members(ty)?;
            let resolved = self.members_of(members);
            return Ok(resolved.call_signatures.is_empty()
                && resolved.construct_signatures.is_empty()
                && resolved.index_infos.is_empty()
                && !resolved.properties.is_empty()
                && resolved.properties.iter().all(|&p| {
                    self.binder
                        .symbol(p)
                        .flags
                        .intersects(tsrs2_types::SymbolFlags::OPTIONAL)
                }));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            for t in types.iter() {
                if !self.is_weak_type(*t)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: hasCommonProperties @6.0.3
    /// tsc-hash: fa8485b4fc4b88a1b9d0c08aca138b7fb718b7a06b9cd790ad2e31d28cb70004
    /// tsc-span: _tsc.js:67298-67305
    pub fn has_common_properties(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        for prop in self.get_properties_of_type(source)? {
            let name = self.binder.symbol(prop).escaped_name.clone();
            if self.is_known_property(target, &name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// getPropertiesOfType, M3 slice: object types read their resolved
    /// properties; primitives have no properties in the noLib world
    /// the probe shares with the oracle (getApparentType against
    /// missing globals resolves to the empty object type — M4 5.3
    /// makes this apparent-type-driven). Union/intersection property
    /// synthesis (createUnionOrIntersectionProperty) is 4.6/M4.
    pub fn get_properties_of_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Vec<tsrs2_binder::SymbolId>> {
        self.get_properties_of_type_full(ty)
    }

    /// tsc-port: getTypeOfPropertyOfType @6.0.3
    /// tsc-hash: ddd47344f8b1b3d0de20c2241560a370790f21248f978ea95d10914e91566057
    /// tsc-span: _tsc.js:55803-55806
    ///
    /// A bare getPropertyOfType → getTypeOfSymbol with NO
    /// receiver-flags guard — getPropertyOfType itself hops through
    /// getReducedApparentType, so primitives and other non-structured
    /// receivers resolve their APPARENT-type members (string.length —
    /// the 6.6-review destructuring-assignment FP face: the old
    /// OBJECT|UNION|INTERSECTION pre-guard degraded the assigned type
    /// and manufactured a 2322 tsc never reports). The binder-flags
    /// VALUE re-filter is gone too: symbolIsValue already gates
    /// inside the property lookup.
    pub fn get_type_of_property_of_type(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<TypeId>> {
        self.get_type_of_property_of_type_full(ty, name)
    }

    pub fn type_has_call_or_construct_signatures(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT) {
            return Ok(false);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        Ok(!resolved.call_signatures.is_empty() || !resolved.construct_signatures.is_empty())
    }

    /// The value-type of the applicable index info for a property
    /// name (getApplicableIndexInfoForName slice: string index catches
    /// every name, number index catches numeric names; symbol keys are
    /// late-bound names, M4). Unions/intersections read their OWN
    /// resolved members — resolveUnionTypeMembers synthesizes the
    /// intersected index infos (getUnionIndexInfos).
    pub fn get_applicable_index_info_for_name(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<TypeId>> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::from_bits(
            TypeFlags::OBJECT.bits() | TypeFlags::UNION_OR_INTERSECTION.bits(),
        )) {
            return Ok(None);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        let numeric = is_numeric_literal_name(name);
        let mut applicable = None;
        for info in &resolved.index_infos {
            let key_flags = self.tables.flags_of(info.key_type);
            if key_flags.intersects(TypeFlags::STRING)
                || (numeric && key_flags.intersects(TypeFlags::NUMBER))
            {
                applicable = Some(info.value_type);
                if numeric && key_flags.intersects(TypeFlags::NUMBER) {
                    break;
                }
            }
        }
        Ok(applicable)
    }

    /// tsc-port: isKnownProperty @6.0.3
    /// tsc-hash: c928c9606661159d5023fee7846acf04060c6eb8ce6e98256afb823214df19ba
    /// tsc-span: _tsc.js:74826-74843
    ///
    /// KNOWN-GAP since M4 (m4-review B2): the
    /// `isLateBoundName(name) && getIndexInfoOfType(target, string)`
    /// disjunct, the isComparingJsxAttributes parameter (hyphenated
    /// JSX names), and the Substitution recursion arm are missing —
    /// late-bound names and JSX attribute types are constructible
    /// since M4, so the old "dead in M3" claim is false.
    pub fn is_known_property(&mut self, target: TypeId, name: &str) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(target);
        if flags.intersects(TypeFlags::OBJECT) {
            let members = self.resolve_structured_type_members(target)?;
            let resolved = self.members_of(members);
            let has_property = resolved.members.get(name).copied().is_some_and(|symbol| {
                self.binder
                    .symbol(symbol)
                    .flags
                    .intersects(tsrs2_types::SymbolFlags::VALUE)
            });
            if has_property
                || self
                    .get_applicable_index_info_for_name(target, name)?
                    .is_some()
            {
                return Ok(true);
            }
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
            && self.is_excess_property_check_target(target)
        {
            let members = match &self.tables.type_of(target).data {
                TypeData::Union { types, .. } => types.to_vec(),
                TypeData::Intersection { types } => types.to_vec(),
                _ => Vec::new(),
            };
            for t in members {
                if self.is_known_property(t, name)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: isExcessPropertyCheckTarget @6.0.3
    /// tsc-hash: 07112f77b564ce3b0cbfa4d2e8d92bd344306e716e3ef034c088cb2295432496
    /// tsc-span: _tsc.js:74844-74846
    pub fn is_excess_property_check_target(&self, ty: TypeId) -> bool {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::OBJECT) {
            return !self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::OBJECT_LITERAL_PATTERN_WITH_COMPUTED_PROPERTIES);
        }
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return true;
        }
        if flags.intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = &self.tables.type_of(ty).data else {
                unreachable!("union flag implies union data");
            };
            return types
                .iter()
                .any(|&t| self.is_excess_property_check_target(t));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = &self.tables.type_of(ty).data else {
                unreachable!("intersection flag implies intersection data");
            };
            return types
                .iter()
                .all(|&t| self.is_excess_property_check_target(t));
        }
        false
    }

    /// tsc-port: isTypeSubsetOf @6.0.3
    /// tsc-hash: cff054b83e62f92ea64e2e70f22fb901c4de67ce2f26f1719c2090d434fb0f65
    /// tsc-span: _tsc.js:69962-69964
    ///
    /// tsc-port: isTypeSubsetOfUnion @6.0.3
    /// tsc-hash: e640160f7bf31f88a27cfa98387ab7365e3b441e08e33396b3121f5d9461ecaf
    /// tsc-span: _tsc.js:69965-69978
    ///
    /// Fallible since the 6.3 joins (their subtypeReduction test is
    /// the first load-bearing consumer): the EnumLike base-type arm
    /// (69974-69976) resolves the parent enum's declared type through
    /// getBaseTypeOfEnumLikeType (&mut + fallible).
    pub fn is_type_subset_of(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        if source == target || self.tables.flags_of(source).intersects(TypeFlags::NEVER) {
            return Ok(true);
        }
        if !self.tables.flags_of(target).intersects(TypeFlags::UNION) {
            return Ok(false);
        }
        let TypeData::Union {
            types: target_types,
            ..
        } = &self.tables.type_of(target).data
        else {
            unreachable!("union flag implies union data");
        };
        if self.tables.flags_of(source).intersects(TypeFlags::UNION) {
            let TypeData::Union {
                types: source_types,
                ..
            } = &self.tables.type_of(source).data
            else {
                unreachable!("union flag implies union data");
            };
            return Ok(source_types.iter().all(|&t| contains_type(target_types, t)));
        }
        if self
            .tables
            .flags_of(source)
            .intersects(TypeFlags::ENUM_LIKE)
            && self.get_base_type_of_enum_like_type(source)? == target
        {
            return Ok(true);
        }
        let TypeData::Union {
            types: target_types,
            ..
        } = &self.tables.type_of(target).data
        else {
            unreachable!("union flag implies union data");
        };
        Ok(contains_type(target_types, source))
    }

    /// tsc-port: isEmptyObjectType @6.0.3
    /// tsc-hash: 3c1001f65e3ebe1f4b8362dce6b6cb1ee18638b6444af9a62b651808c112c9a5
    /// tsc-span: _tsc.js:64647-64649
    pub fn is_empty_object_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        // Tuples carry at least the `length` property — never empty
        // (member resolution for references is M4 5.3).
        if self.tables.is_tuple_type(ty) {
            return Ok(false);
        }
        if flags.intersects(TypeFlags::OBJECT) {
            let members = self.resolve_structured_type_members(ty)?;
            let resolved = self.members_of(members);
            return Ok(resolved.properties.is_empty()
                && resolved.call_signatures.is_empty()
                && resolved.construct_signatures.is_empty()
                && resolved.index_infos.is_empty());
        }
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return Ok(true);
        }
        if flags.intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = self.tables.type_of(ty).data.clone() else {
                unreachable!("union flag implies union data");
            };
            for t in types.iter() {
                if self.is_empty_object_type(*t)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            for t in types.iter() {
                if !self.is_empty_object_type(*t)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: filterPrimitivesIfContainsNonPrimitive @6.0.3
    /// tsc-hash: c075a9b738b739e48b0722e99e35369f201b1df2a15cca9f79228c597960b891
    /// tsc-span: _tsc.js:90509-90517
    pub fn filter_primitives_if_contains_non_primitive(&mut self, ty: TypeId) -> TypeId {
        let maybe_non_primitive = match &self.tables.type_of(ty).data {
            TypeData::Union { types, .. } => types
                .iter()
                .any(|&t| self.tables.flags_of(t).intersects(TypeFlags::NON_PRIMITIVE)),
            _ => self
                .tables
                .flags_of(ty)
                .intersects(TypeFlags::NON_PRIMITIVE),
        };
        if maybe_non_primitive {
            let result = self.tables.filter_type(ty, |tables, t| {
                !tables.flags_of(t).intersects(TypeFlags::PRIMITIVE)
            });
            if !self.tables.flags_of(result).intersects(TypeFlags::NEVER) {
                return result;
            }
        }
        ty
    }

    /// tsc-port: getRegularTypeOfObjectLiteral @6.0.3
    /// tsc-hash: ba6c91001232662c53f26b817dbbb93bd01295b1301268a1e9906c4d8bd7be24
    /// tsc-span: _tsc.js:67923-67938
    ///
    /// transformTypeOfMembers (67914-67922) is inlined: property types
    /// recurse through getRegularTypeOfObjectLiteral, unchanged
    /// properties keep their symbol (createSymbolWithType only on
    /// change). Call/construct signatures and index infos carry over
    /// from the resolved source.
    pub fn get_regular_type_of_object_literal(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if !(self.is_object_literal_type(ty)
            && self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::FRESH_LITERAL))
        {
            return Ok(ty);
        }
        if let Some(regular) = self.tables.type_of(ty).regular_type {
            if regular != ty {
                return Ok(regular);
            }
        }
        let resolved = self.resolve_structured_type_members(ty)?;
        let mut members = tsrs2_binder::SymbolTable::default();
        let mut properties: Vec<tsrs2_binder::SymbolId> = Vec::new();
        for property in self.members_of(resolved).properties.clone() {
            let original = self.get_type_of_symbol(property)?;
            let updated = self.get_regular_type_of_object_literal(original)?;
            let member = if updated == original {
                property
            } else {
                self.create_symbol_with_type(property, Some(updated))
            };
            let name = self.binder.symbol(member).escaped_name.clone();
            members.insert(name, member);
            properties.push(member);
        }
        let source_members = self.members_of(resolved);
        let members_id = self.alloc_members(crate::state::ResolvedMembers {
            members,
            properties,
            call_signatures: source_members.call_signatures.clone(),
            construct_signatures: source_members.construct_signatures.clone(),
            index_infos: source_members.index_infos.clone(),
        });
        let symbol = self.tables.type_of(ty).symbol;
        let regular = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        let object_flags =
            self.tables.object_flags_of(ty).bits() & !ObjectFlags::FRESH_LITERAL.bits();
        self.tables.type_mut(regular).object_flags = ObjectFlags::from_bits(object_flags);
        self.tables.type_mut(regular).symbol = symbol;
        self.tables.type_mut(regular).regular_type = Some(regular);
        self.links.set_type_members(
            self.speculation_depth,
            regular,
            crate::links::LinkSlot::Resolved(members_id),
        );
        self.tables.type_mut(ty).regular_type = Some(regular);
        Ok(regular)
    }

    // ---- union key-property machinery (69587-69634) ----

    /// tsc-port: getKeyPropertyName @6.0.3
    /// tsc-hash: 19ca365a85f004645438b49e9e34c0daafde7967e5b56be95b0c6b1ab809f3d8
    /// tsc-span: _tsc.js:69612-69624
    ///
    /// tsc-port: mapTypesByKeyProperty @6.0.3
    /// tsc-hash: 7c3609e0c956d059e800b32aba8a8dfab76f6cabe97ff557d758b46325fef232
    /// tsc-span: _tsc.js:69587-69611
    ///
    /// The keyPropertyName/constituentMap caches live on TypeLinks
    /// (tsc stores them on the union type object). M5's discriminant
    /// narrowing reuses this machinery.
    pub(crate) fn get_key_property_name(&mut self, union: TypeId) -> CheckResult2<Option<String>> {
        if let Some(cached) = self.links.ty(union).union_key_property.resolved() {
            return Ok(cached.name);
        }
        let TypeData::Union { types, .. } = self.tables.type_of(union).data.clone() else {
            unreachable!("union flag implies union data");
        };
        let object_like = TypeFlags::from_bits(
            TypeFlags::OBJECT.bits() | TypeFlags::INSTANTIABLE_NON_PRIMITIVE.bits(),
        );
        if types.len() < 10
            || self
                .tables
                .object_flags_of(union)
                .intersects(ObjectFlags::PRIMITIVE_UNION)
            || types
                .iter()
                .filter(|&&t| self.tables.flags_of(t).intersects(object_like))
                .count()
                < 10
        {
            return Ok(None);
        }
        let mut key_property_name = None;
        'outer: for &t in types.iter() {
            if self.tables.flags_of(t).intersects(object_like) {
                for prop in self.get_properties_of_type(t)? {
                    let prop_type = self.get_type_of_symbol(prop)?;
                    if self.is_unit_type(prop_type) {
                        key_property_name = Some(self.binder.symbol(prop).escaped_name.clone());
                        break 'outer;
                    }
                }
            }
        }
        let map = match &key_property_name {
            Some(name) => self.map_types_by_key_property(&types, &name.clone())?,
            None => None,
        };
        let resolved_name = if map.is_some() {
            key_property_name
        } else {
            None
        };
        self.links.set_type_union_key_property(
            self.speculation_depth,
            union,
            crate::links::UnionKeyProperty {
                name: resolved_name.clone(),
                constituent_map: map,
            },
        );
        Ok(resolved_name)
    }

    fn map_types_by_key_property(
        &mut self,
        types: &[TypeId],
        name: &str,
    ) -> CheckResult2<Option<std::collections::HashMap<TypeId, TypeId>>> {
        let mut map = std::collections::HashMap::new();
        let mut count = 0usize;
        let object_like = TypeFlags::from_bits(
            TypeFlags::OBJECT.bits()
                | TypeFlags::INTERSECTION.bits()
                | TypeFlags::INSTANTIABLE_NON_PRIMITIVE.bits(),
        );
        let unknown = self.tables.intrinsics.unknown;
        for &ty in types {
            if self.tables.flags_of(ty).intersects(object_like) {
                let Some(discriminant) = self.get_type_of_property_of_type(ty, name)? else {
                    return Ok(None);
                };
                if !self.is_literal_type(discriminant) {
                    return Ok(None);
                }
                let mut duplicate = false;
                let constituents = if self
                    .tables
                    .flags_of(discriminant)
                    .intersects(TypeFlags::UNION)
                {
                    match &self.tables.type_of(discriminant).data {
                        TypeData::Union { types, .. } => types.to_vec(),
                        _ => vec![discriminant],
                    }
                } else {
                    vec![discriminant]
                };
                for t in constituents {
                    let regular = self.tables.get_regular_type_of_literal_type(t);
                    match map.get(&regular).copied() {
                        None => {
                            map.insert(regular, ty);
                        }
                        Some(existing) if existing != unknown => {
                            map.insert(regular, unknown);
                            duplicate = true;
                        }
                        Some(_) => {}
                    }
                }
                if !duplicate {
                    count += 1;
                }
            }
        }
        Ok(if count >= 10 && count * 2 >= types.len() {
            Some(map)
        } else {
            None
        })
    }

    /// tsc-port: getConstituentTypeForKeyType @6.0.3
    /// tsc-hash: 4359544adbcb805ecf85f0af3cfc44554a4fb7c49aa520d6af4971018d006671
    /// tsc-span: _tsc.js:69625-69629
    ///
    /// tsc-port: getMatchingUnionConstituentForType @6.0.3
    /// tsc-hash: 0390be57e7781c489ec22c638ca6a5945b2dac5ce311681580c4d938a945dbab
    /// tsc-span: _tsc.js:69630-69634
    pub fn get_matching_union_constituent_for_type(
        &mut self,
        union: TypeId,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(key_property_name) = self.get_key_property_name(union)? else {
            return Ok(None);
        };
        let Some(prop_type) = self.get_type_of_property_of_type(ty, &key_property_name)? else {
            return Ok(None);
        };
        self.get_constituent_type_for_key_type(union, prop_type)
    }

    /// tsc-port: getConstituentTypeForKeyType @6.0.3 (shared tail)
    /// tsc-hash: 4359544adbcb805ecf85f0af3cfc44554a4fb7c49aa520d6af4971018d006671
    /// tsc-span: _tsc.js:69625-69629
    ///
    /// getKeyPropertyName must have populated the constituent map (the
    /// two callers read it first).
    pub(crate) fn get_constituent_type_for_key_type(
        &mut self,
        union: TypeId,
        key_type: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let key = self.tables.get_regular_type_of_literal_type(key_type);
        let unknown = self.tables.intrinsics.unknown;
        let result = self
            .links
            .ty(union)
            .union_key_property
            .resolved()
            .and_then(|cached| {
                cached
                    .constituent_map
                    .as_ref()
                    .and_then(|m| m.get(&key).copied())
            });
        Ok(result.filter(|&r| r != unknown))
    }

    /// Conservative pre-M8 isGenericMappedType gate. Every mapped type
    /// is treated as generic until the mapped payload and the precise
    /// constraint/name-type test land; this keeps the dormant mapped
    /// branches fail-closed as soon as mapped objects become constructible.
    pub(crate) fn is_generic_mapped_type_state(&self, ty: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
    }

    // ---- recursion depth (checker-key §1.3) ----

    /// tsc-port: isDeeplyNestedType @6.0.3
    /// tsc-hash: f3ba77d18312de37ff50b6ea012109ca7f22c5431a117fe8a2f634af12290010
    /// tsc-span: _tsc.js:67465-67490
    ///
    /// tsc-port: hasMatchingRecursionIdentity @6.0.3
    /// tsc-hash: b609ca6b38a7271c1f10d10ddfca694e157816b8e465e9d9a3847bc35e0bec9e
    /// tsc-span: _tsc.js:67498-67506
    ///
    /// maxDepth defaults to 3 (greenfield §4.7's "5" was the audited
    /// erratum). InstantiatedMapped unwrapping is dead until M4.
    pub fn is_deeply_nested_type(
        &self,
        ty: TypeId,
        stack: &[TypeId],
        depth: usize,
        max_depth: usize,
    ) -> bool {
        if depth < max_depth {
            return false;
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = &self.tables.type_of(ty).data else {
                unreachable!("intersection flag implies intersection data");
            };
            return types
                .iter()
                .any(|&t| self.is_deeply_nested_type(t, stack, depth, max_depth));
        }
        let identity = self.get_recursion_identity(ty);
        let mut count = 0usize;
        let mut last_type_id = 0u32;
        for &t in stack.iter().take(depth) {
            let matches = if self.tables.flags_of(t).intersects(TypeFlags::INTERSECTION) {
                match &self.tables.type_of(t).data {
                    TypeData::Intersection { types } => types
                        .iter()
                        .any(|&member| self.get_recursion_identity(member) == identity),
                    _ => false,
                }
            } else {
                self.get_recursion_identity(t) == identity
            };
            if matches {
                if t.0 >= last_type_id {
                    count += 1;
                    if count >= max_depth {
                        return true;
                    }
                }
                last_type_id = t.0;
            }
        }
        false
    }

    /// tsc-port: getRecursionIdentity @6.0.3
    /// tsc-hash: ad1c79d106e2d5dec2b7bd40792f7c0de78641f6dea366f1794fa4fe9d61d29c
    /// tsc-span: _tsc.js:67507-67532
    ///
    /// tsc's identity is a node/symbol/type object; here it is a
    /// discriminated key. KNOWN-GAP since M4 (m4-review B3): the
    /// symbol arm lacks tsc's `!(Anonymous && Class)` exclusion, and
    /// the TypeParameter arm returns the TYPE where tsc returns
    /// type.symbol — M4 declared type parameters carry symbols
    /// (constraints.rs attaches them; the old "symbol-less
    /// synthetics" claim is false), so instantiation clones never
    /// unify and deep generic recursion can overflow into a wrong
    /// False. The Conditional arm (type.root) stays out with
    /// conditional types themselves.
    pub(crate) fn get_recursion_identity(&self, ty: TypeId) -> RecursionIdentity {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::OBJECT) && !self.is_object_or_array_literal_type(ty) {
            if self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::REFERENCE)
            {
                if let Some(node) = self.links.ty(ty).deferred_node {
                    return RecursionIdentity::Node(node);
                }
            }
            if let Some(symbol) = self.tables.type_of(ty).symbol {
                return RecursionIdentity::Symbol(symbol);
            }
            if self.tables.is_tuple_type(ty) {
                return RecursionIdentity::Type(self.tables.reference_target(ty));
            }
        }
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            // KNOWN-GAP (m4-review B3): tsc returns type.symbol here;
            // declared type parameters have carried symbols since M4.
            return RecursionIdentity::Type(ty);
        }
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            // 67522-67527: chase the objectType chain.
            let mut current = ty;
            while self
                .tables
                .flags_of(current)
                .intersects(TypeFlags::INDEXED_ACCESS)
            {
                let TypeData::IndexedAccess { object_type, .. } = self.tables.type_of(current).data
                else {
                    unreachable!("indexed-access flag implies indexed-access data");
                };
                current = object_type;
            }
            return RecursionIdentity::Type(current);
        }
        RecursionIdentity::Type(ty)
    }

    fn is_object_or_array_literal_type(&self, ty: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::from_bits(
                ObjectFlags::OBJECT_LITERAL.bits() | ObjectFlags::ARRAY_LITERAL.bits(),
            ))
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getBaseTypeOfLiteralType @6.0.3
    /// tsc-hash: c16f94f06e54359337919a8bc85571e3f9f6017fc573396231819c88a0d6de60
    /// tsc-span: _tsc.js:67755-67757
    ///
    /// The union arm maps members (tsc's `B{id}` cachedTypes entry is
    /// a perf cache, not semantics).
    pub fn get_base_type_of_literal_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::ENUM_LIKE) {
            return self.get_base_type_of_enum_like_type(ty);
        }
        if flags.intersects(TypeFlags::from_bits(
            TypeFlags::STRING_LITERAL.bits()
                | TypeFlags::TEMPLATE_LITERAL.bits()
                | TypeFlags::STRING_MAPPING.bits(),
        )) {
            return Ok(self.tables.intrinsics.string);
        }
        if flags.intersects(TypeFlags::NUMBER_LITERAL) {
            return Ok(self.tables.intrinsics.number);
        }
        if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            return Ok(self.tables.intrinsics.bigint);
        }
        if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
            return Ok(self.tables.intrinsics.boolean);
        }
        if flags.intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = self.tables.type_of(ty).data.clone() else {
                unreachable!("union flag implies union data");
            };
            let mut mapped = Vec::with_capacity(types.len());
            let mut changed = false;
            for &t in types.iter() {
                let base = self.get_base_type_of_literal_type(t)?;
                changed |= base != t;
                mapped.push(base);
            }
            if !changed {
                return Ok(ty);
            }
            return self.get_union_type_ex(&mapped, UnionReduction::Literal);
        }
        Ok(ty)
    }

    /// tsc-port: getBaseTypeOfEnumLikeType @6.0.3
    /// tsc-hash: 858147aecede12f638a65d11df4e363261107bd21691b950f2d9b966c18bbe9d
    /// tsc-span: _tsc.js:57436-57438
    pub(crate) fn get_base_type_of_enum_like_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::ENUM_LIKE) {
            if let Some(symbol) = self.tables.type_of(ty).symbol {
                if self
                    .binder
                    .symbol(symbol)
                    .flags
                    .intersects(tsrs2_types::SymbolFlags::ENUM_MEMBER)
                {
                    let parent = self
                        .get_parent_of_symbol(symbol)
                        .expect("enum member symbols have enum parents");
                    // getDeclaredTypeOfSymbol reduces to the enum arm:
                    // an enum member's parent is always an enum.
                    return self.get_declared_type_of_enum(parent);
                }
            }
        }
        Ok(ty)
    }

    /// tsc-port: checkAssertionDeferred @6.0.3
    /// tsc-hash: f6ba47fa52cafe10b5a25a331f4c920416a684b8b2ca6f9b65a4faea0fcaca32
    /// tsc-span: _tsc.js:77939-77955
    ///
    /// The comparable-pin fixture shape: `s as Target` errors (2352)
    /// iff NEITHER comparable(target, widened(exprType)) NOR
    /// comparable(exprType, target) holds, where exprType =
    /// getRegularTypeOfObjectLiteral(getBaseTypeOfLiteralType(source)).
    /// getWidenedType is the identity in M3: no constructible type
    /// carries ObjectFlags::RequiresWidening (widening contexts are
    /// M6 expression checking).
    pub fn is_assertion_legal(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        let base = self.get_base_type_of_literal_type(source)?;
        let expr_type = self.get_regular_type_of_object_literal(base)?;
        let widened = expr_type; // getWidenedType stub (identity in M3)
        let first = self.is_type_comparable_to(target, widened);
        if let Ok(true) = first {
            return Ok(true);
        }
        let second = self.is_type_comparable_to(expr_type, target);
        if let Ok(true) = second {
            return Ok(true);
        }
        first?;
        second?;
        Ok(false)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecursionIdentity {
    Symbol(tsrs2_binder::SymbolId),
    Type(TypeId),
    /// Deferred references key on their node (67509-67511): every
    /// mapper-carrying instance over the same annotation shares one
    /// identity, distinct nodes over the same interface do not.
    Node(NodeId),
}

/// tsc isNumericLiteralName (19205): the name round-trips through
/// numeric conversion. The annotation-reachable slice: canonical
/// non-negative integer strings.
fn is_numeric_literal_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_digit())
        && (name == "0" || !name.starts_with('0'))
}

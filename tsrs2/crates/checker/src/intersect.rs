//! getIntersectionType — the eight-step normalization
//! (checker-foundations §4.2, m3-types-relations-steps.md stage 4.3).
//!
//! Lives in the checker because isEmptyAnonymousObjectType reads
//! binder symbol tables; the pure helpers (eachUnionContains,
//! intersectUnionsOfPrimitiveTypes, filterType, createIntersectionType,
//! the intersectionTypes interning map) live in TypeTables.

use tsrs2_types::{IntersectionFlags, ObjectFlags, TypeData, TypeFlags, TypeId, UnionReduction};

use crate::state::{CheckResult2, CheckerState};

impl<'a> CheckerState<'a> {
    /// tsc-port: isEmptyAnonymousObjectType @6.0.3
    /// tsc-hash: 15e9f25e82c373110efd8e96193cc36f560c22b338ce439eb0987cad8a63e6f1
    /// tsc-span: _tsc.js:64650-64652
    ///
    /// tsc-port: isEmptyResolvedType @6.0.3
    /// tsc-hash: 5fc6c0aedd7943649ee13f65800363b3d537bbf465e929a68d26562fb217af60
    /// tsc-span: _tsc.js:64644-64646
    ///
    /// The `t !== anyFunctionType` exclusion is live since M4 5.0
    /// built anyFunctionType (checker init block, 47179).
    pub fn is_empty_anonymous_object_type(&self, ty: TypeId) -> bool {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::ANONYMOUS)
        {
            return false;
        }
        // `type.members && isEmptyResolvedType(type)`: checks ALREADY
        // resolved members without forcing resolution.
        if let Some(members) = self.links.ty(ty).resolved_members.resolved() {
            let resolved = self.members_of(members);
            return ty != self.any_function_type
                && resolved.properties.is_empty()
                && resolved.call_signatures.is_empty()
                && resolved.construct_signatures.is_empty()
                && resolved.index_infos.is_empty();
        }
        match self.tables.type_of(ty).symbol {
            Some(symbol) => {
                self.symbol_flags(symbol)
                    .intersects(tsrs2_types::SymbolFlags::TYPE_LITERAL)
                    && self.symbol_members(symbol).is_empty()
            }
            None => false,
        }
    }

    /// tsc-port: addTypeToIntersection @6.0.3
    /// tsc-hash: 71c5ede0aea5221fc7f1799fb76ee547932e58fef9430140a85026a198e47458
    /// tsc-span: _tsc.js:61650-61679
    ///
    /// The typeMembershipMap is identity-keyed AND order-preserving:
    /// a Vec with membership checks. Note the Unit∧Unit quirk (61670):
    /// a second unit type sets the NonPrimitive INCLUDES bit so the
    /// DisjointDomains check collapses `1 & 2` to never.
    fn add_type_to_intersection(
        &mut self,
        type_set: &mut Vec<TypeId>,
        mut includes: i32,
        ty: TypeId,
    ) -> i32 {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = self.tables.type_of(ty).data.clone() else {
                unreachable!("intersection flag implies intersection data");
            };
            return self.add_types_to_intersection(type_set, includes, &types);
        }
        if self.is_empty_anonymous_object_type(ty) {
            if includes & TypeFlags::INCLUDES_EMPTY_OBJECT.bits() == 0 {
                includes |= TypeFlags::INCLUDES_EMPTY_OBJECT.bits();
                type_set.push(ty);
            }
        } else {
            if flags.intersects(TypeFlags::ANY_OR_UNKNOWN) {
                if ty == self.tables.intrinsics.wildcard {
                    includes |= TypeFlags::INCLUDES_WILDCARD.bits();
                }
                if self.tables.is_error_type(ty) {
                    includes |= TypeFlags::INCLUDES_ERROR.bits();
                }
            } else if self.tables.strict_null_checks || !flags.intersects(TypeFlags::NULLABLE) {
                let mut ty = ty;
                if ty == self.tables.intrinsics.missing {
                    includes |= TypeFlags::INCLUDES_MISSING_TYPE.bits();
                    ty = self.tables.intrinsics.undefined;
                }
                if !type_set.contains(&ty) {
                    if self.tables.flags_of(ty).intersects(TypeFlags::UNIT)
                        && includes & TypeFlags::UNIT.bits() != 0
                    {
                        includes |= TypeFlags::NON_PRIMITIVE.bits();
                    }
                    type_set.push(ty);
                }
            }
            includes |= flags.bits() & TypeFlags::INCLUDES_MASK.bits();
        }
        includes
    }

    /// tsc-port: addTypesToIntersection @6.0.3
    /// tsc-hash: 85b3abb27206ce7edc7bea3fd69d92426ca2ff03347ffc9b2f7799aba52e18cf
    /// tsc-span: _tsc.js:61680-61685
    fn add_types_to_intersection(
        &mut self,
        type_set: &mut Vec<TypeId>,
        mut includes: i32,
        types: &[TypeId],
    ) -> i32 {
        for &ty in types {
            let regular = self.tables.get_regular_type_of_literal_type(ty);
            includes = self.add_type_to_intersection(type_set, includes, regular);
        }
        includes
    }

    /// tsc-port: removeRedundantSupertypes @6.0.3
    /// tsc-hash: e5493d8e8fca52c9df30c4d0f8896f71949b552a67b270198ad38d45edda2810
    /// tsc-span: _tsc.js:61686-61696
    fn remove_redundant_supertypes(&mut self, types: &mut Vec<TypeId>, includes: i32) {
        let mut i = types.len();
        while i > 0 {
            i -= 1;
            let t = types[i];
            let flags = self.tables.flags_of(t);
            let remove = (flags.intersects(TypeFlags::STRING)
                && includes
                    & (TypeFlags::STRING_LITERAL.bits()
                        | TypeFlags::TEMPLATE_LITERAL.bits()
                        | TypeFlags::STRING_MAPPING.bits())
                    != 0)
                || (flags.intersects(TypeFlags::NUMBER)
                    && includes & TypeFlags::NUMBER_LITERAL.bits() != 0)
                || (flags.intersects(TypeFlags::BIG_INT)
                    && includes & TypeFlags::BIG_INT_LITERAL.bits() != 0)
                || (flags.intersects(TypeFlags::ES_SYMBOL)
                    && includes & TypeFlags::UNIQUE_ES_SYMBOL.bits() != 0)
                || (flags.intersects(TypeFlags::VOID)
                    && includes & TypeFlags::UNDEFINED.bits() != 0)
                || (self.is_empty_anonymous_object_type(t)
                    && includes & TypeFlags::DEFINITELY_NON_NULLABLE.bits() != 0);
            if remove {
                types.remove(i);
            }
        }
    }

    /// tsc-port: getIntersectionType @6.0.3
    /// tsc-hash: 9aea88818c9c1ebe2075ffa20ada35d8a3a725412c6976f5c44c6dc86ae8c85f
    /// tsc-span: _tsc.js:61789-61870
    ///
    /// M3 dispositions:
    /// - extractRedundantTemplateLiterals (61800-61802) needs
    ///   isTypeSubtypeOf — Unsupported until the 4.6 template relation
    ///   arm provides the matcher.
    /// - The 2-member type-variable constraint collapse
    ///   (checker-foundations §4.2 step 6) runs live since M4 5.1c —
    ///   getBaseConstraintOfType + strict-subtype over declared type
    ///   parameters.
    /// - Alias parameters are M4 5.1b rows; the intern key alias
    ///   segment is empty, `*` for NoConstraintReduction as in tsc.
    pub fn get_intersection_type(
        &mut self,
        types: &[TypeId],
        flags: IntersectionFlags,
    ) -> CheckResult2<TypeId> {
        self.get_intersection_type_ex(types, flags, None, None)
    }

    /// The alias-carrying entry (tsc's optional aliasSymbol/
    /// aliasTypeArguments parameters at 61789).
    pub fn get_intersection_type_ex(
        &mut self,
        types: &[TypeId],
        flags: IntersectionFlags,
        alias_symbol: Option<tsrs2_binder::SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let mut type_set: Vec<TypeId> = Vec::new();
        let includes = self.add_types_to_intersection(&mut type_set, 0, types);
        // objectFlags picks up IsConstrainedTypeVariable only in the
        // M4 step-6 branch below; zero until then.
        let object_flags = ObjectFlags::from_bits(0);
        if includes & TypeFlags::NEVER.bits() != 0 {
            return Ok(if type_set.contains(&self.tables.intrinsics.silent_never) {
                self.tables.intrinsics.silent_never
            } else {
                self.tables.intrinsics.never
            });
        }
        let disjoint = TypeFlags::DISJOINT_DOMAINS.bits();
        if (self.tables.strict_null_checks
            && includes & TypeFlags::NULLABLE.bits() != 0
            && includes
                & (TypeFlags::OBJECT.bits()
                    | TypeFlags::NON_PRIMITIVE.bits()
                    | TypeFlags::INCLUDES_EMPTY_OBJECT.bits())
                != 0)
            || (includes & TypeFlags::NON_PRIMITIVE.bits() != 0
                && includes & (disjoint & !TypeFlags::NON_PRIMITIVE.bits()) != 0)
            || (includes & TypeFlags::STRING_LIKE.bits() != 0
                && includes & (disjoint & !TypeFlags::STRING_LIKE.bits()) != 0)
            || (includes & TypeFlags::NUMBER_LIKE.bits() != 0
                && includes & (disjoint & !TypeFlags::NUMBER_LIKE.bits()) != 0)
            || (includes & TypeFlags::BIG_INT_LIKE.bits() != 0
                && includes & (disjoint & !TypeFlags::BIG_INT_LIKE.bits()) != 0)
            || (includes & TypeFlags::ES_SYMBOL_LIKE.bits() != 0
                && includes & (disjoint & !TypeFlags::ES_SYMBOL_LIKE.bits()) != 0)
            || (includes & TypeFlags::VOID_LIKE.bits() != 0
                && includes & (disjoint & !TypeFlags::VOID_LIKE.bits()) != 0)
        {
            return Ok(self.tables.intrinsics.never);
        }
        if includes & (TypeFlags::TEMPLATE_LITERAL.bits() | TypeFlags::STRING_MAPPING.bits()) != 0
            && includes & TypeFlags::STRING_LITERAL.bits() != 0
            && self.extract_redundant_template_literals(&mut type_set)?
        {
            return Ok(self.tables.intrinsics.never);
        }
        if includes & TypeFlags::ANY.bits() != 0 {
            return Ok(if includes & TypeFlags::INCLUDES_WILDCARD.bits() != 0 {
                self.tables.intrinsics.wildcard
            } else if includes & TypeFlags::INCLUDES_ERROR.bits() != 0 {
                self.tables.intrinsics.error
            } else {
                self.tables.intrinsics.any
            });
        }
        if !self.tables.strict_null_checks && includes & TypeFlags::NULLABLE.bits() != 0 {
            return Ok(if includes & TypeFlags::INCLUDES_EMPTY_OBJECT.bits() != 0 {
                self.tables.intrinsics.never
            } else if includes & TypeFlags::UNDEFINED.bits() != 0 {
                self.tables.intrinsics.undefined
            } else {
                self.tables.intrinsics.null
            });
        }
        if ((includes & TypeFlags::STRING.bits() != 0
            && includes
                & (TypeFlags::STRING_LITERAL.bits()
                    | TypeFlags::TEMPLATE_LITERAL.bits()
                    | TypeFlags::STRING_MAPPING.bits())
                != 0)
            || (includes & TypeFlags::NUMBER.bits() != 0
                && includes & TypeFlags::NUMBER_LITERAL.bits() != 0)
            || (includes & TypeFlags::BIG_INT.bits() != 0
                && includes & TypeFlags::BIG_INT_LITERAL.bits() != 0)
            || (includes & TypeFlags::ES_SYMBOL.bits() != 0
                && includes & TypeFlags::UNIQUE_ES_SYMBOL.bits() != 0)
            || (includes & TypeFlags::VOID.bits() != 0
                && includes & TypeFlags::UNDEFINED.bits() != 0)
            || (includes & TypeFlags::INCLUDES_EMPTY_OBJECT.bits() != 0
                && includes & TypeFlags::DEFINITELY_NON_NULLABLE.bits() != 0))
            && !flags.intersects(IntersectionFlags::NO_SUPERTYPE_REDUCTION)
        {
            self.remove_redundant_supertypes(&mut type_set, includes);
        }
        if includes & TypeFlags::INCLUDES_MISSING_TYPE.bits() != 0 {
            let undefined = self.tables.intrinsics.undefined;
            if let Some(position) = type_set.iter().position(|&t| t == undefined) {
                type_set[position] = self.tables.intrinsics.missing;
            }
        }
        if type_set.is_empty() {
            return Ok(self.tables.intrinsics.unknown);
        }
        if type_set.len() == 1 {
            return Ok(type_set[0]);
        }
        // Step 6 (61821-61840): the 2-member type-variable constraint
        // collapse — `T & string` where T's base constraint covers the
        // primitive collapses to T (or never), else the intersection
        // interns with IsConstrainedTypeVariable so union construction
        // can run removeConstrainedTypeVariables over it.
        let mut object_flags = object_flags;
        if type_set.len() == 2 && !flags.intersects(IntersectionFlags::NO_CONSTRAINT_REDUCTION) {
            let type_var_index = usize::from(
                !self
                    .tables
                    .flags_of(type_set[0])
                    .intersects(TypeFlags::TYPE_VARIABLE),
            );
            let type_variable = type_set[type_var_index];
            let primitive_type = type_set[1 - type_var_index];
            if self
                .tables
                .flags_of(type_variable)
                .intersects(TypeFlags::TYPE_VARIABLE)
                && ((self
                    .tables
                    .flags_of(primitive_type)
                    .intersects(TypeFlags::PRIMITIVE | TypeFlags::NON_PRIMITIVE)
                    && !self.is_generic_string_like_type(primitive_type))
                    || includes & TypeFlags::INCLUDES_EMPTY_OBJECT.bits() != 0)
            {
                if let Some(constraint) = self.get_base_constraint_of_type(type_variable)? {
                    let all_primitive_like = {
                        let members = self.union_members_or_self(constraint);
                        members.iter().all(|&member| {
                            self.tables
                                .flags_of(member)
                                .intersects(TypeFlags::PRIMITIVE | TypeFlags::NON_PRIMITIVE)
                                || self.is_empty_anonymous_object_type(member)
                        })
                    };
                    if all_primitive_like {
                        if self.is_type_strict_subtype_of(constraint, primitive_type)? {
                            return Ok(type_variable);
                        }
                        let union_member_subtype = if self
                            .tables
                            .flags_of(constraint)
                            .intersects(TypeFlags::UNION)
                        {
                            let members = self.union_members_or_self(constraint);
                            let mut any = false;
                            for member in members {
                                if self.is_type_strict_subtype_of(member, primitive_type)? {
                                    any = true;
                                    break;
                                }
                            }
                            any
                        } else {
                            false
                        };
                        if !union_member_subtype
                            && !self.is_type_strict_subtype_of(primitive_type, constraint)?
                        {
                            return Ok(self.tables.intrinsics.never);
                        }
                        object_flags = ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE;
                    }
                }
            }
        }
        let key = format!(
            "{}{}",
            self.tables.get_type_list_id(&type_set),
            if flags.intersects(IntersectionFlags::NO_CONSTRAINT_REDUCTION) {
                "*".to_owned()
            } else {
                self.tables.get_alias_id(alias_symbol, alias_type_arguments)
            }
        );
        if let Some(result) = self.tables.intersection_types_get(&key) {
            return Ok(result);
        }
        let result = if includes & TypeFlags::UNION.bits() != 0 {
            self.distribute_intersection_over_unions(
                &mut type_set,
                flags,
                alias_symbol,
                alias_type_arguments,
                types.len(),
            )?
        } else {
            self.tables.create_intersection_type(
                type_set,
                object_flags,
                alias_symbol,
                alias_type_arguments,
            )
        };
        self.tables.intersection_types_insert(key, result);
        Ok(result)
    }

    /// The union-member distribution tail of getIntersectionType
    /// (61841-61864): primitive-union intersection, the undefined/null
    /// pull-outs, the ≥3 binary split, and the cross-product fallback.
    fn distribute_intersection_over_unions(
        &mut self,
        type_set: &mut Vec<TypeId>,
        flags: IntersectionFlags,
        alias_symbol: Option<tsrs2_binder::SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
        original_arity: usize,
    ) -> CheckResult2<TypeId> {
        if self.tables.intersect_unions_of_primitive_types(type_set) {
            return self.get_intersection_type_ex(
                &type_set.clone(),
                flags,
                alias_symbol,
                alias_type_arguments,
            );
        }
        let undefined = self.tables.intrinsics.undefined;
        let all_first_undefined = type_set.iter().all(|&t| {
            self.tables.flags_of(t).intersects(TypeFlags::UNION)
                && match &self.tables.type_of(t).data {
                    TypeData::Union { types, .. } => types
                        .first()
                        .is_some_and(|&m| self.tables.flags_of(m).intersects(TypeFlags::UNDEFINED)),
                    _ => false,
                }
        });
        if all_first_undefined {
            let contained = if type_set
                .iter()
                .any(|&t| self.tables.contains_missing_type(t))
            {
                self.tables.intrinsics.missing
            } else {
                undefined
            };
            self.tables.remove_from_each(type_set, TypeFlags::UNDEFINED);
            let inner = self.get_intersection_type(&type_set.clone(), flags)?;
            return self.get_union_type_ex_with_origin(
                &[inner, contained],
                UnionReduction::Literal,
                alias_symbol,
                alias_type_arguments,
                None,
            );
        }
        let all_contain_null = type_set.iter().all(|&t| {
            self.tables.flags_of(t).intersects(TypeFlags::UNION)
                && match &self.tables.type_of(t).data {
                    TypeData::Union { types, .. } => {
                        types
                            .first()
                            .is_some_and(|&m| self.tables.flags_of(m).intersects(TypeFlags::NULL))
                            || types.get(1).is_some_and(|&m| {
                                self.tables.flags_of(m).intersects(TypeFlags::NULL)
                            })
                    }
                    _ => false,
                }
        });
        if all_contain_null {
            let null = self.tables.intrinsics.null;
            self.tables.remove_from_each(type_set, TypeFlags::NULL);
            let inner = self.get_intersection_type(&type_set.clone(), flags)?;
            return self.get_union_type_ex_with_origin(
                &[inner, null],
                UnionReduction::Literal,
                alias_symbol,
                alias_type_arguments,
                None,
            );
        }
        if type_set.len() >= 3 && original_arity > 2 {
            let middle = type_set.len() / 2;
            let left = self.get_intersection_type(&type_set[..middle], flags)?;
            let right = self.get_intersection_type(&type_set[middle..], flags)?;
            return self.get_intersection_type_ex(
                &[left, right],
                flags,
                alias_symbol,
                alias_type_arguments,
            );
        }
        if !self.check_cross_product_union_guard(type_set) {
            return Ok(self.tables.intrinsics.error);
        }
        let constituents = self.get_cross_product_intersections(type_set, flags)?;
        let has_intersection = constituents
            .iter()
            .any(|&t| self.tables.flags_of(t).intersects(TypeFlags::INTERSECTION));
        let origin = if has_intersection
            && self.tables.get_constituent_count_of_types(&constituents)
                > self.tables.get_constituent_count_of_types(type_set)
        {
            Some(self.tables.create_origin_union_or_intersection_type(
                TypeFlags::INTERSECTION,
                type_set.clone(),
            ))
        } else {
            None
        };
        self.get_union_type_ex_with_origin(
            &constituents,
            UnionReduction::Literal,
            alias_symbol,
            alias_type_arguments,
            origin,
        )
    }

    /// tsc-port: getCrossProductIntersections @6.0.3
    /// tsc-hash: 58c3d15f520c8d78e23d81f83235848b71028a8242a12a9263b747caeab32ca5
    /// tsc-span: _tsc.js:61884-61902
    fn get_cross_product_intersections(
        &mut self,
        types: &[TypeId],
        flags: IntersectionFlags,
    ) -> CheckResult2<Vec<TypeId>> {
        let count = self.cross_product_union_size(types);
        let mut intersections = Vec::new();
        for i in 0..count {
            let mut constituents = types.to_vec();
            let mut n = i;
            for j in (0..types.len()).rev() {
                if self.tables.flags_of(types[j]).intersects(TypeFlags::UNION) {
                    let TypeData::Union {
                        types: source_types,
                        ..
                    } = self.tables.type_of(types[j]).data.clone()
                    else {
                        unreachable!("union flag implies union data");
                    };
                    let length = source_types.len();
                    constituents[j] = source_types[n % length];
                    n /= length;
                }
            }
            let t = self.get_intersection_type(&constituents, flags)?;
            if !self.tables.flags_of(t).intersects(TypeFlags::NEVER) {
                intersections.push(t);
            }
        }
        Ok(intersections)
    }

    /// checkCrossProductUnion (61874) for the intersection path — the
    /// diagnostic (2799 family) is deferred with error reporting (T2).
    fn check_cross_product_union_guard(&self, types: &[TypeId]) -> bool {
        self.cross_product_union_size(types) < 100_000
    }

    pub(crate) fn cross_product_union_size(&self, types: &[TypeId]) -> usize {
        let mut size: usize = 1;
        for &t in types {
            if self.tables.flags_of(t).intersects(TypeFlags::UNION) {
                if let TypeData::Union { types: members, .. } = &self.tables.type_of(t).data {
                    size = size.saturating_mul(members.len());
                }
            } else if self.tables.flags_of(t).intersects(TypeFlags::NEVER) {
                size = 0;
            }
        }
        size
    }
}

//! M4 5.5c: the type-facts core (getTypeFacts family L69690-69783),
//! pulled FORWARD from the 5.5d access band (extraction doc §0
//! [FACTS] / §11 risk #4). Rationale: the literals band consumes it —
//! isValidSpreadType's falsy-strip and getSpreadType's right-optional
//! merge change VERDICTS on nullable unions (`{ ...maybeNull }` must
//! spread cleanly), so an identity stub is an FP generator and an
//! escape would contain every spread statement. Porting the classifier
//! COMPLETE and verbatim here retires the risk; 5.5d only wires its
//! remaining consumers (nonnull selection, getAdjustedTypeWithFacts —
//! which stays unported until then).
//!
//! Codegen note: TypeFacts.FunctionFacts was regenerated for this
//! slice — the enum emitter dropped the `16728e3` exponent (16728 vs
//! 16728000), a latent wrong-facts bomb for every non-strict Object
//! classification.

use tsrs2_types::{
    IntersectionFlags, ObjectFlags, SymbolFlags, TypeData, TypeFacts, TypeFlags, TypeId,
    UnionReduction,
};

use crate::state::{CheckResult2, CheckerState};

impl<'a> CheckerState<'a> {
    /// tsc-port: getTypeFacts @6.0.3
    /// tsc-hash: 9ebc7f3fcc2cc025223d75d01134c416284c42ce877e8934427ca333d8a3cf15
    /// tsc-span: _tsc.js:69697-69699
    pub(crate) fn get_type_facts(
        &mut self,
        ty: TypeId,
        mask: TypeFacts,
    ) -> CheckResult2<TypeFacts> {
        Ok(TypeFacts::from_bits(
            self.get_type_facts_worker(ty, mask)?.bits() & mask.bits(),
        ))
    }

    /// tsc-port: hasTypeFacts @6.0.3
    /// tsc-hash: 4f06d60bbba177c53e3a163c6144509e67902619f008771a0d4b16f8f1a423c0
    /// tsc-span: _tsc.js:69700-69702
    pub(crate) fn has_type_facts(&mut self, ty: TypeId, mask: TypeFacts) -> CheckResult2<bool> {
        Ok(!self.get_type_facts(ty, mask)?.is_empty())
    }

    /// tsc-port: isNullableType @6.0.3
    /// tsc-hash: f7b677457f8e94a6bd55aa41baf3f2dd44f4fcafb0b0c2b2ee9b5f0810c77b70
    /// tsc-span: _tsc.js:74993-74995
    pub(crate) fn is_nullable_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        self.has_type_facts(ty, TypeFacts::IS_UNDEFINED_OR_NULL)
    }

    /// tsc-port: getNonNullableTypeIfNeeded @6.0.3
    /// tsc-hash: 49e58f1edfe3016bf12af5d2d847006af7e55b11c6027d89255c6b9c53bb52d1
    /// tsc-span: _tsc.js:74996-74998
    pub(crate) fn get_non_nullable_type_if_needed(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self.is_nullable_type(ty)? {
            self.get_non_nullable_type(ty)
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getTypeFactsWorker @6.0.3
    /// tsc-hash: 6c1e2c95f8abc6a317ecb892a100b67c5d86d9166a1d2a4919ac9f2d695fb241
    /// tsc-span: _tsc.js:69703-69767
    fn get_type_facts_worker(
        &mut self,
        ty: TypeId,
        caller_only_needs: TypeFacts,
    ) -> CheckResult2<TypeFacts> {
        let mut ty = ty;
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::INTERSECTION | TypeFlags::INSTANTIABLE)
        {
            ty = self
                .get_base_constraint_of_type(ty)?
                .unwrap_or(self.tables.intrinsics.unknown);
        }
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::STRING | TypeFlags::STRING_MAPPING) {
            return Ok(if strict_null_checks {
                TypeFacts::STRING_STRICT_FACTS
            } else {
                TypeFacts::STRING_FACTS
            });
        }
        if flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::TEMPLATE_LITERAL) {
            let is_empty = flags.intersects(TypeFlags::STRING_LITERAL)
                && matches!(
                    &self.tables.type_of(ty).data,
                    TypeData::Literal { value: tsrs2_types::LiteralValue::String(value) } if value.is_empty()
                );
            return Ok(match (strict_null_checks, is_empty) {
                (true, true) => TypeFacts::EMPTY_STRING_STRICT_FACTS,
                (true, false) => TypeFacts::NON_EMPTY_STRING_STRICT_FACTS,
                (false, true) => TypeFacts::EMPTY_STRING_FACTS,
                (false, false) => TypeFacts::NON_EMPTY_STRING_FACTS,
            });
        }
        if flags.intersects(TypeFlags::NUMBER | TypeFlags::ENUM) {
            return Ok(if strict_null_checks {
                TypeFacts::NUMBER_STRICT_FACTS
            } else {
                TypeFacts::NUMBER_FACTS
            });
        }
        if flags.intersects(TypeFlags::NUMBER_LITERAL) {
            let is_zero = matches!(
                &self.tables.type_of(ty).data,
                TypeData::Literal { value: tsrs2_types::LiteralValue::Number(value) } if *value == 0.0
            );
            return Ok(match (strict_null_checks, is_zero) {
                (true, true) => TypeFacts::ZERO_NUMBER_STRICT_FACTS,
                (true, false) => TypeFacts::NON_ZERO_NUMBER_STRICT_FACTS,
                (false, true) => TypeFacts::ZERO_NUMBER_FACTS,
                (false, false) => TypeFacts::NON_ZERO_NUMBER_FACTS,
            });
        }
        if flags.intersects(TypeFlags::BIG_INT) {
            return Ok(if strict_null_checks {
                TypeFacts::BIG_INT_STRICT_FACTS
            } else {
                TypeFacts::BIG_INT_FACTS
            });
        }
        if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            let is_zero = self.is_zero_big_int(ty);
            return Ok(match (strict_null_checks, is_zero) {
                (true, true) => TypeFacts::ZERO_BIG_INT_STRICT_FACTS,
                (true, false) => TypeFacts::NON_ZERO_BIG_INT_STRICT_FACTS,
                (false, true) => TypeFacts::ZERO_BIG_INT_FACTS,
                (false, false) => TypeFacts::NON_ZERO_BIG_INT_FACTS,
            });
        }
        if flags.intersects(TypeFlags::BOOLEAN) {
            return Ok(if strict_null_checks {
                TypeFacts::BOOLEAN_STRICT_FACTS
            } else {
                TypeFacts::BOOLEAN_FACTS
            });
        }
        if flags.intersects(TypeFlags::BOOLEAN_LIKE) {
            let is_false = ty == self.tables.intrinsics.false_fresh
                || ty == self.tables.intrinsics.false_regular;
            return Ok(match (strict_null_checks, is_false) {
                (true, true) => TypeFacts::FALSE_STRICT_FACTS,
                (true, false) => TypeFacts::TRUE_STRICT_FACTS,
                (false, true) => TypeFacts::FALSE_FACTS,
                (false, false) => TypeFacts::TRUE_FACTS,
            });
        }
        if flags.intersects(TypeFlags::OBJECT) {
            let possible_facts = if strict_null_checks {
                TypeFacts::EMPTY_OBJECT_STRICT_FACTS
                    | TypeFacts::FUNCTION_STRICT_FACTS
                    | TypeFacts::OBJECT_STRICT_FACTS
            } else {
                TypeFacts::EMPTY_OBJECT_FACTS | TypeFacts::FUNCTION_FACTS | TypeFacts::OBJECT_FACTS
            };
            if !caller_only_needs.intersects(possible_facts) {
                return Ok(TypeFacts::NONE);
            }
            let anonymous_empty = self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::ANONYMOUS)
                && self.is_empty_object_type(ty)?;
            return Ok(if anonymous_empty {
                if strict_null_checks {
                    TypeFacts::EMPTY_OBJECT_STRICT_FACTS
                } else {
                    TypeFacts::EMPTY_OBJECT_FACTS
                }
            } else if self.is_function_object_type(ty)? {
                if strict_null_checks {
                    TypeFacts::FUNCTION_STRICT_FACTS
                } else {
                    TypeFacts::FUNCTION_FACTS
                }
            } else if strict_null_checks {
                TypeFacts::OBJECT_STRICT_FACTS
            } else {
                TypeFacts::OBJECT_FACTS
            });
        }
        if flags.intersects(TypeFlags::VOID) {
            return Ok(TypeFacts::VOID_FACTS);
        }
        if flags.intersects(TypeFlags::UNDEFINED) {
            return Ok(TypeFacts::UNDEFINED_FACTS);
        }
        if flags.intersects(TypeFlags::NULL) {
            return Ok(TypeFacts::NULL_FACTS);
        }
        if flags.intersects(TypeFlags::ES_SYMBOL_LIKE) {
            return Ok(if strict_null_checks {
                TypeFacts::SYMBOL_STRICT_FACTS
            } else {
                TypeFacts::SYMBOL_FACTS
            });
        }
        if flags.intersects(TypeFlags::NON_PRIMITIVE) {
            return Ok(if strict_null_checks {
                TypeFacts::OBJECT_STRICT_FACTS
            } else {
                TypeFacts::OBJECT_FACTS
            });
        }
        if flags.intersects(TypeFlags::NEVER) {
            return Ok(TypeFacts::NONE);
        }
        if flags.intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            let mut facts = TypeFacts::NONE;
            for member in members {
                facts |= self.get_type_facts_worker(member, caller_only_needs)?;
            }
            return Ok(facts);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            return self.get_intersection_type_facts(ty, caller_only_needs);
        }
        Ok(TypeFacts::UNKNOWN_FACTS)
    }

    /// tsc-port: getIntersectionTypeFacts @6.0.3
    /// tsc-hash: a350d85e286b979ab1fb3010b79a423ab4780a49443d5ad1430682373f0c5204
    /// tsc-span: _tsc.js:69768-69780
    fn get_intersection_type_facts(
        &mut self,
        ty: TypeId,
        caller_only_needs: TypeFacts,
    ) -> CheckResult2<TypeFacts> {
        let ignore_objects = self.maybe_type_of_kind(ty, TypeFlags::PRIMITIVE);
        let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
            TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("intersection flag implies intersection data"),
        };
        let mut ored = TypeFacts::NONE;
        let mut anded = TypeFacts::ALL;
        for member in members {
            if !(ignore_objects && self.tables.flags_of(member).intersects(TypeFlags::OBJECT)) {
                let facts = self.get_type_facts_worker(member, caller_only_needs)?;
                ored |= facts;
                anded = TypeFacts::from_bits(anded.bits() & facts.bits());
            }
        }
        Ok(TypeFacts::from_bits(
            (ored.bits() & TypeFacts::OR_FACTS_MASK.bits())
                | (anded.bits() & TypeFacts::AND_FACTS_MASK.bits()),
        ))
    }

    /// tsc-port: isFunctionObjectType @6.0.3
    /// tsc-hash: bf4a3b7a20ec1aaafa2be264237f9b971856a1239702dc041d8daf662cfb0ebe
    /// tsc-span: _tsc.js:69690-69696
    ///
    /// The EvolvingArray early-out is M5 shape (no producer yet).
    fn is_function_object_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::EVOLVING_ARRAY)
        {
            return Ok(false);
        }
        let resolved = self.resolve_structured_type_members(ty)?;
        let members = self.members_of(resolved);
        if !members.call_signatures.is_empty() || !members.construct_signatures.is_empty() {
            return Ok(true);
        }
        let has_bind = members.members.get("bind").is_some();
        if !has_bind {
            return Ok(false);
        }
        let global_function = self.global_function_type()?;
        self.is_type_subtype_of(ty, global_function)
    }

    /// tsc-port: isZeroBigInt @6.0.3
    /// tsc-hash: 93d3ccbfa0ecb15d0e4b08e3b15ebbca39894c8849ae3e89d9c8143885dd83cd
    /// tsc-span: _tsc.js:67836-67838
    fn is_zero_big_int(&self, ty: TypeId) -> bool {
        matches!(
            &self.tables.type_of(ty).data,
            TypeData::Literal { value: tsrs2_types::LiteralValue::BigInt(value) } if value.base10_value == "0"
        )
    }

    /// tsc-port: getTypeWithFacts @6.0.3
    /// tsc-hash: 53e770dd10dd1701ff49e017516753a2a1e305954de0f5b91395bbb0255b6f87
    /// tsc-span: _tsc.js:69781-69783
    pub(crate) fn get_type_with_facts(
        &mut self,
        ty: TypeId,
        include: TypeFacts,
    ) -> CheckResult2<TypeId> {
        self.filter_type_with(ty, |state, t| state.has_type_facts(t, include))
    }

    /// tsc-port: removeDefinitelyFalsyTypes @6.0.3
    /// tsc-hash: 4b35dd27b02e56d454abacaf93da8d180122234771a27ad38f9428a382799575
    /// tsc-span: _tsc.js:67839-67841
    pub(crate) fn remove_definitely_falsy_types(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        self.filter_type_with(ty, |state, t| state.has_type_facts(t, TypeFacts::TRUTHY))
    }

    /// tsc-port: removeMissingOrUndefinedType @6.0.3
    /// tsc-hash: f50a22dd1cfc61b3a3c81cd42b05113f97189bdd6403db6cbdd3722cdc94538a
    /// tsc-span: _tsc.js:67889-67891
    ///
    /// The exactOptionalPropertyTypes arm inlines tsc removeType
    /// (filterType with a `t !== missingType` predicate, 69993).
    pub(crate) fn remove_missing_or_undefined_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self.options.exact_optional_property_types == Some(true) {
            let missing = self.tables.intrinsics.missing;
            return Ok(self.tables.filter_type(ty, |_, t| t != missing));
        }
        self.get_type_with_facts(ty, TypeFacts::NE_UNDEFINED)
    }

    /// tsc-port: getAdjustedTypeWithFacts @6.0.3
    /// tsc-hash: 7adc7a7a39ef6d5f3cb3b3a7d5487cb9c601a79570cb1ead9d3c9e18ea152f9c
    /// tsc-span: _tsc.js:69784-69798
    ///
    /// The 5.5d facts consumer: `unknown` filters as its union
    /// decomposition (unknownUnionType) and recombines on the way out;
    /// the strictNullChecks switch matches the EXACT facts value, not
    /// a mask.
    pub(crate) fn get_adjusted_type_with_facts(
        &mut self,
        ty: TypeId,
        facts: TypeFacts,
    ) -> CheckResult2<TypeId> {
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let input = if strict_null_checks && self.tables.flags_of(ty).intersects(TypeFlags::UNKNOWN)
        {
            self.unknown_union_type
        } else {
            ty
        };
        let filtered = self.get_type_with_facts(input, facts)?;
        let reduced = self.recombine_unknown_type(filtered);
        if strict_null_checks {
            if facts == TypeFacts::NE_UNDEFINED {
                let null = self.tables.intrinsics.null;
                return self.remove_nullable_by_intersection(
                    reduced,
                    TypeFacts::EQ_UNDEFINED,
                    TypeFacts::EQ_NULL,
                    TypeFacts::IS_NULL,
                    null,
                );
            }
            if facts == TypeFacts::NE_NULL {
                let undefined = self.tables.intrinsics.undefined;
                return self.remove_nullable_by_intersection(
                    reduced,
                    TypeFacts::EQ_NULL,
                    TypeFacts::EQ_UNDEFINED,
                    TypeFacts::IS_UNDEFINED,
                    undefined,
                );
            }
            if facts == TypeFacts::NE_UNDEFINED_OR_NULL || facts == TypeFacts::TRUTHY {
                return Ok(self
                    .map_type(
                        reduced,
                        &mut |state, t| {
                            if state.has_type_facts(t, TypeFacts::EQ_UNDEFINED_OR_NULL)? {
                                state
                                    .get_global_non_nullable_type_instantiation(t)
                                    .map(Some)
                            } else {
                                Ok(Some(t))
                            }
                        },
                        false,
                    )?
                    .expect("mapper is total"));
            }
        }
        Ok(reduced)
    }

    /// tsc-port: removeNullableByIntersection @6.0.3
    /// tsc-hash: c97094efb5ff6a39e7f440adb289501ace56fc69192afd72975b3cb37785f085
    /// tsc-span: _tsc.js:69799-69806
    fn remove_nullable_by_intersection(
        &mut self,
        ty: TypeId,
        target_facts: TypeFacts,
        other_facts: TypeFacts,
        other_includes_facts: TypeFacts,
        other_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let facts = self.get_type_facts(
            ty,
            TypeFacts::EQ_UNDEFINED
                | TypeFacts::EQ_NULL
                | TypeFacts::IS_UNDEFINED
                | TypeFacts::IS_NULL,
        )?;
        if !facts.intersects(target_facts) {
            return Ok(ty);
        }
        let empty_object = self.empty_object_type;
        let empty_and_other_union =
            self.get_union_type_ex(&[empty_object, other_type], UnionReduction::Literal)?;
        Ok(self
            .map_type(
                ty,
                &mut |state, t| {
                    if state.has_type_facts(t, target_facts)? {
                        let widen = !facts.intersects(other_includes_facts)
                            && state.has_type_facts(t, other_facts)?;
                        let second = if widen {
                            empty_and_other_union
                        } else {
                            state.empty_object_type
                        };
                        state
                            .get_intersection_type(&[t, second], IntersectionFlags::NONE)
                            .map(Some)
                    } else {
                        Ok(Some(t))
                    }
                },
                false,
            )?
            .expect("mapper is total"))
    }

    /// tsc-port: recombineUnknownType @6.0.3
    /// tsc-hash: 183a60fa027c58fd5e71c37f15f0c07e12dbb906b989fa8e8e4405adaa7cc76f
    /// tsc-span: _tsc.js:69807-69809
    pub(crate) fn recombine_unknown_type(&self, ty: TypeId) -> TypeId {
        if ty == self.unknown_union_type {
            self.tables.intrinsics.unknown
        } else {
            ty
        }
    }

    /// tsc-port: getNonNullableType @6.0.3
    /// tsc-hash: e64e3f0d08a085a8b0a5a597fb450e623bdf865ae234a5a8565c24f338871e98
    /// tsc-span: _tsc.js:67868-67870
    pub(crate) fn get_non_nullable_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self
            .options
            .strict_option_value(self.options.strict_null_checks)
        {
            self.get_adjusted_type_with_facts(ty, TypeFacts::NE_UNDEFINED_OR_NULL)
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getGlobalNonNullableTypeInstantiation @6.0.3
    /// tsc-hash: 914679d3e8905107c24464dbcdcf2e37c870a7974bbcf3e4437860ca9acc285a
    /// tsc-span: _tsc.js:67855-67867
    ///
    /// The lib NonNullable<T> alias when it exists (getGlobalSymbol
    /// with NO diagnostic — no suggestion-budget interaction); the
    /// noLib fallback is `T & {}`. The miss memoizes as tsc's
    /// unknownSymbol sentinel.
    fn get_global_non_nullable_type_instantiation(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self.deferred_global_non_nullable_type_alias.is_none() {
            let symbol = self.get_global_symbol("NonNullable", SymbolFlags::TYPE_ALIAS, None);
            self.deferred_global_non_nullable_type_alias = Some(symbol);
        }
        match self
            .deferred_global_non_nullable_type_alias
            .expect("memoized above")
        {
            Some(alias) => self.get_type_alias_instantiation(alias, Some(&[ty]), None, None),
            None => {
                let empty_object = self.empty_object_type;
                self.get_intersection_type(&[ty, empty_object], IntersectionFlags::NONE)
            }
        }
    }

    /// tsc filterType (69991) with a checker-side predicate: verdicts
    /// are precomputed per flat member (tsc's `filter` evaluates f on
    /// every member in list order — no short-circuit to preserve), then
    /// the tables twin performs the identical union reconstruction
    /// (origin filtering, PrimitiveUnion/ContainsIntersections carry,
    /// never tail).
    pub(crate) fn filter_type_with(
        &mut self,
        ty: TypeId,
        mut predicate: impl FnMut(&mut Self, TypeId) -> CheckResult2<bool>,
    ) -> CheckResult2<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            let mut keep = std::collections::HashSet::new();
            for member in members {
                if predicate(self, member)? {
                    keep.insert(member);
                }
            }
            return Ok(self.tables.filter_type(ty, |_, t| keep.contains(&t)));
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) || predicate(self, ty)? {
            Ok(ty)
        } else {
            Ok(self.tables.intrinsics.never)
        }
    }
}

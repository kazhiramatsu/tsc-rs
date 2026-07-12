//! M4 5.5b: the literal-level widening carve-out (extraction doc §0
//! [WIDEN]) — pure type→type classifiers with no widening context.
//! Object-level widening (getWidenedType 68013, getRegularTypeOfObjectLiteral
//! 67923, reportErrorsFromWidening 68187, reportImplicitAny) stays 5.6
//! and builds out this module then.

use tsrs2_types::{TypeFlags, TypeId};

use crate::state::{CheckResult2, CheckerState};

impl<'a> CheckerState<'a> {
    /// tsc-port: getBaseTypeOfLiteralTypeForComparison @6.0.3
    /// tsc-hash: bd554f80bd0a6cab1d2af095a19a79fe0e7cd393ac2bc946ff4c28e353b40f72
    /// tsc-span: _tsc.js:67762-67764
    ///
    /// NO enum-like arm — Enum (65536) maps to number (the extraction
    /// doc calls this out against the getBaseTypeOfLiteralType shape).
    #[allow(dead_code)] // consumer: the relational-operator band (5.5e)
    pub(crate) fn get_base_type_of_literal_type_for_comparison(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(
            TypeFlags::STRING_LITERAL | TypeFlags::TEMPLATE_LITERAL | TypeFlags::STRING_MAPPING,
        ) {
            Ok(self.tables.intrinsics.string)
        } else if flags.intersects(TypeFlags::NUMBER_LITERAL | TypeFlags::ENUM) {
            Ok(self.tables.intrinsics.number)
        } else if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            Ok(self.tables.intrinsics.bigint)
        } else if flags.intersects(TypeFlags::BOOLEAN_LITERAL) {
            Ok(self.tables.intrinsics.boolean)
        } else if flags.intersects(TypeFlags::UNION) {
            Ok(self
                .map_type(
                    ty,
                    &mut |state, t| {
                        state.get_base_type_of_literal_type_for_comparison(t).map(Some)
                    },
                    false,
                )?
                .expect("mapper is total"))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getWidenedLiteralType @6.0.3
    /// tsc-hash: 34e9ce1ae0d68d982398871f0aa07073f045e652899c902b0c4a97d64dd04f9a
    /// tsc-span: _tsc.js:67765-67767
    ///
    /// Only FRESH literals widen; regular literals pass through.
    pub(crate) fn get_widened_literal_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        let fresh = self.tables.is_fresh_literal_type(ty);
        if flags.intersects(TypeFlags::ENUM_LIKE) && fresh {
            self.get_base_type_of_enum_like_type(ty)
        } else if flags.intersects(TypeFlags::STRING_LITERAL) && fresh {
            Ok(self.tables.intrinsics.string)
        } else if flags.intersects(TypeFlags::NUMBER_LITERAL) && fresh {
            Ok(self.tables.intrinsics.number)
        } else if flags.intersects(TypeFlags::BIG_INT_LITERAL) && fresh {
            Ok(self.tables.intrinsics.bigint)
        } else if flags.intersects(TypeFlags::BOOLEAN_LITERAL) && fresh {
            Ok(self.tables.intrinsics.boolean)
        } else if flags.intersects(TypeFlags::UNION) {
            Ok(self
                .map_type(
                    ty,
                    &mut |state, t| state.get_widened_literal_type(t).map(Some),
                    false,
                )?
                .expect("mapper is total"))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getWidenedUniqueESSymbolType @6.0.3
    /// tsc-hash: 004e6feb812db03248e01232736667a491d945d662999742b4b85398a051d86a
    /// tsc-span: _tsc.js:67768-67770
    pub(crate) fn get_widened_unique_es_symbol_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
            Ok(self.tables.intrinsics.es_symbol)
        } else if flags.intersects(TypeFlags::UNION) {
            Ok(self
                .map_type(
                    ty,
                    &mut |state, t| state.get_widened_unique_es_symbol_type(t).map(Some),
                    false,
                )?
                .expect("mapper is total"))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getWidenedLiteralLikeTypeForContextualType @6.0.3
    /// tsc-hash: e37987d1c869b101752178cb673a0723ddca1f24e403363c9eba0b8238ba7107
    /// tsc-span: _tsc.js:67771-67776
    pub(crate) fn get_widened_literal_like_type_for_contextual_type(
        &mut self,
        ty: TypeId,
        contextual_type: Option<TypeId>,
    ) -> CheckResult2<TypeId> {
        let mut ty = ty;
        if !self.is_literal_of_contextual_type(ty, contextual_type)? {
            let widened = self.get_widened_literal_type(ty)?;
            ty = self.get_widened_unique_es_symbol_type(widened)?;
        }
        Ok(self.tables.get_regular_type_of_literal_type(ty))
    }

    /// tsc-port: getWidenedLiteralLikeTypeForContextualReturnTypeIfNeeded @6.0.3
    /// tsc-hash: e4a1b137182f82fa0678d1ae3be9c2b587f29bdc5a8011d40f409f55fadf4e28
    /// tsc-span: _tsc.js:67777-67783
    ///
    /// The async arm reads getPromisedTypeOfPromise — [ASYNC → 5.5f];
    /// the consumer (getReturnTypeFromBody 78752) lands there too.
    #[allow(dead_code)]
    pub(crate) fn get_widened_literal_like_type_for_contextual_return_type_if_needed(
        &mut self,
        ty: Option<TypeId>,
        contextual_signature_return_type: Option<TypeId>,
        is_async: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(current) = ty else {
            return Ok(ty);
        };
        if !self.is_unit_type(current) {
            return Ok(ty);
        }
        let contextual_type = match contextual_signature_return_type {
            None => None,
            Some(_signature_return) if is_async => {
                return Err(crate::state::Unsupported::new(
                    "getWidenedLiteralLikeTypeForContextualReturnTypeIfNeeded async arm \
                     (getPromisedTypeOfPromise, 5.5f)",
                ));
            }
            Some(signature_return) => Some(signature_return),
        };
        Ok(Some(self.get_widened_literal_like_type_for_contextual_type(
            current,
            contextual_type,
        )?))
    }

    /// tsc-port: getWidenedLiteralLikeTypeForContextualIterationTypeIfNeeded @6.0.3
    /// tsc-hash: bf2483d08e235cdcd45b14bb2336905208671b3533c617ac30218cc53188328e
    /// tsc-span: _tsc.js:67784-67790
    ///
    /// The generator arm reads getIterationTypeOfGeneratorFunctionReturnType
    /// — [ITER → 5.5f]; the consumers (yield/return aggregation) land
    /// there too.
    #[allow(dead_code)]
    pub(crate) fn get_widened_literal_like_type_for_contextual_iteration_type_if_needed(
        &mut self,
        ty: Option<TypeId>,
        contextual_signature_return_type: Option<TypeId>,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(current) = ty else {
            return Ok(ty);
        };
        if !self.is_unit_type(current) {
            return Ok(ty);
        }
        if contextual_signature_return_type.is_some() {
            return Err(crate::state::Unsupported::new(
                "getWidenedLiteralLikeTypeForContextualIterationTypeIfNeeded generator arm \
                 (getIterationTypeOfGeneratorFunctionReturnType, 5.5f)",
            ));
        }
        Ok(Some(self.get_widened_literal_like_type_for_contextual_type(current, None)?))
    }
    /// tsc-port: getWidenedType @6.0.3
    /// tsc-hash: 069e9d9013f7a91a2991169dff22caaa5a08c140f3782950300edb0b9f8f5800
    /// tsc-span: _tsc.js:68013-68052
    ///
    /// The 5.5f slice: the any/nullable arm (non-strict `return null`
    /// getters widen to any — conformance FP find, classPropertyAsPrivate)
    /// and the union recursion port; the object-literal and
    /// array/tuple arms are the 5.6 widening machinery proper
    /// (getWidenedTypeOfObjectLiteral + widening contexts) and ESCAPE
    /// — containment (FN) rather than an identity that leaks
    /// unwidened types into renderable relations (FP). The `widened`
    /// memo rides with 5.6 (every live arm here is cheap).
    pub(crate) fn get_widened_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        use tsrs2_types::ObjectFlags;
        const REQUIRES_WIDENING: i32 =
            ObjectFlags::CONTAINS_WIDENING_TYPE.bits() | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL.bits();
        if self.tables.object_flags_of(ty).bits() & REQUIRES_WIDENING == 0 {
            return Ok(ty);
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::ANY | TypeFlags::NULLABLE) {
            return Ok(self.tables.intrinsics.any);
        }
        if self.is_object_literal_type(ty) {
            return Err(crate::state::Unsupported::new(
                "object-literal widening (getWidenedTypeOfObjectLiteral, 5.6)",
            ));
        }
        if flags.intersects(TypeFlags::UNION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            let mut widened = Vec::with_capacity(members.len());
            for member in &members {
                let next = if self.tables.flags_of(*member).intersects(TypeFlags::NULLABLE) {
                    *member
                } else {
                    self.get_widened_type(*member)?
                };
                widened.push(next);
            }
            let mut any_empty_object = false;
            for member in &widened {
                if self.is_empty_object_type(*member)? {
                    any_empty_object = true;
                    break;
                }
            }
            let reduction = if any_empty_object {
                tsrs2_types::UnionReduction::Subtype
            } else {
                tsrs2_types::UnionReduction::Literal
            };
            return self.get_union_type_ex(&widened, reduction);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                tsrs2_types::TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            let mut widened = Vec::with_capacity(members.len());
            for member in members {
                widened.push(self.get_widened_type(member)?);
            }
            return self.get_intersection_type(&widened, tsrs2_types::IntersectionFlags::NONE);
        }
        if self.is_array_type(ty)? || self.tables.is_tuple_type(ty) {
            return Err(crate::state::Unsupported::new(
                "array/tuple widening (createTypeReference over widened elements, 5.6)",
            ));
        }
        Ok(ty)
    }
}

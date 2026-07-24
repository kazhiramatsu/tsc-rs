//! keyof + indexed access (M4 5.2f) — getIndexType and the
//! TYPE-POSITION slice of getIndexedAccessType.
//!
//! Construction commit: Index/IndexedAccess types intern, instantiate
//! and compute base constraints; the relation arms over them stay
//! escaped until 5.3b pins land. Error paths that need typeToString's
//! nodeBuilder (tuple/object displays) unwind as Unsupported — display
//! work is T2/M8; expression-position access (accessExpression, flow,
//! deprecation, write types) is 5.5/M7 and structurally skipped, each
//! noted in place.

use tsrs2_binder::SymbolId;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    AccessFlags, IndexFlags, IntersectionFlags, ObjectFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, UnionReduction,
};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, Unsupported};
use tsrs2_diags::gen as diagnostics;

impl<'a> CheckerState<'a> {
    /// tsc-port: getGlobalExtractSymbol @6.0.3
    /// tsc-hash: b22ea48a736bc228a748d543706f8186374bca9aba21f2032144f7b55757e5a6
    /// tsc-span: _tsc.js:60907-60916
    ///
    /// Miss AND wrong-arity (2317 reported inside the shared worker)
    /// both memoize unknownSymbol.
    fn get_global_extract_symbol(&mut self) -> CheckResult2<Option<tsrs2_binder::SymbolId>> {
        if self.deferred_global_extract_symbol.is_none() {
            let resolved =
                self.get_global_type_alias_symbol("Extract", 2, /*report_errors*/ true)?;
            self.deferred_global_extract_symbol =
                Some(Some(resolved.unwrap_or(self.unknown_symbol)));
        }
        let memo = self
            .deferred_global_extract_symbol
            .expect("filled above")
            .expect("memo holds symbol-or-unknown");
        Ok((memo != self.unknown_symbol).then_some(memo))
    }

    /// tsc-port: getExtractStringType @6.0.3
    /// tsc-hash: e3c1341d9f62620207da8f9e6a74a7fd55301b67530491a046d2a53018028dac
    /// tsc-span: _tsc.js:62020-62023
    pub(crate) fn get_extract_string_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        match self.get_global_extract_symbol()? {
            Some(alias) => {
                let string = self.tables.intrinsics.string;
                self.get_type_alias_instantiation(alias, Some(&[ty, string]), None, None)
            }
            None => Ok(self.tables.intrinsics.string),
        }
    }

    /// tsc-port: getIndexTypeOrString @6.0.3
    /// tsc-hash: c8ecc6e8763470e9dc210f694a20ab557776d7b467a7ebbebb853f15537f3b8f
    /// tsc-span: _tsc.js:62024-62027
    pub(crate) fn get_index_type_or_string(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let index_type = self.get_index_type(ty, IndexFlags::NONE)?;
        let extracted = self.get_extract_string_type(index_type)?;
        Ok(
            if self.tables.flags_of(extracted).intersects(TypeFlags::NEVER) {
                self.tables.intrinsics.string
            } else {
                extracted
            },
        )
    }

    /// tsc-port: getIndexType @6.0.3
    /// tsc-hash: af5867f7723c495d086366e08501a86c4c76b02f70bf5b94dae8f524f2ef51ac
    /// tsc-span: _tsc.js:62016-62019
    ///
    /// isNoInferType is constant false (Substitution types are
    /// unconstructible before 9.6); mapped objects use the 9.5b key
    /// expansion path.
    pub fn get_index_type(&mut self, ty: TypeId, index_flags: IndexFlags) -> CheckResult2<TypeId> {
        let ty = self.get_reduced_type(ty)?;
        if self.should_defer_index_type(ty, index_flags)? {
            return Ok(self.get_index_type_for_generic_type(ty, index_flags));
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION) {
            let members = self.union_members_or_self(ty);
            let mut index_types = Vec::with_capacity(members.len());
            for member in members {
                index_types.push(self.get_index_type(member, index_flags)?);
            }
            return self.get_intersection_type(&index_types, IntersectionFlags::NONE);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            let mut index_types = Vec::with_capacity(members.len());
            for member in members {
                index_types.push(self.get_index_type(member, index_flags)?);
            }
            return self.get_union_type_ex(&index_types, UnionReduction::Literal);
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
        {
            return self.get_index_type_for_mapped_type(ty, index_flags);
        }
        if ty == self.tables.intrinsics.wildcard {
            return Ok(self.tables.intrinsics.wildcard);
        }
        if flags.intersects(TypeFlags::UNKNOWN) {
            return Ok(self.tables.intrinsics.never);
        }
        if flags.intersects(TypeFlags::ANY | TypeFlags::NEVER) {
            return Ok(self.tables.intrinsics.string_number_symbol);
        }
        let include = TypeFlags::from_bits(
            (if index_flags.intersects(IndexFlags::NO_INDEX_SIGNATURES) {
                TypeFlags::STRING_LITERAL.bits()
            } else {
                TypeFlags::STRING_LIKE.bits()
            }) | (if index_flags.intersects(IndexFlags::STRINGS_ONLY) {
                0
            } else {
                TypeFlags::NUMBER_LIKE.bits() | TypeFlags::ES_SYMBOL_LIKE.bits()
            }),
        );
        self.get_literal_type_from_properties(ty, include, index_flags == IndexFlags::NONE)
    }

    /// tsc-port: shouldDeferIndexType @6.0.3
    /// tsc-hash: da7fa1b903bc78d060f67772b13b9c45bec2c78f40fe0ea25bb0e47691bd763e
    /// tsc-span: _tsc.js:62013-62015
    ///
    /// The generic-mapped-with-nameType disjunct stays behind the named
    /// getIndexTypeForMappedType 9.5b boundary below.
    pub(crate) fn should_defer_index_type(
        &mut self,
        ty: TypeId,
        index_flags: IndexFlags,
    ) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
            || self.tables.is_generic_tuple_type(ty)
        {
            return Ok(true);
        }
        if flags.intersects(TypeFlags::UNION)
            && !index_flags.intersects(IndexFlags::NO_REDUCIBLE_CHECK)
            && self.is_generic_reducible_type(ty)?
        {
            return Ok(true);
        }
        if flags.intersects(TypeFlags::INTERSECTION)
            && self.maybe_type_of_kind(ty, TypeFlags::INSTANTIABLE)
        {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            for &member in &members {
                if self.is_empty_anonymous_object_type(member)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: isGenericReducibleType @6.0.3
    /// tsc-hash: 9b73fef19992062e1a6fe2349eef1af06ef1e0a697d04b591abe099a8ebde7a3
    /// tsc-span: _tsc.js:59318-59320
    ///
    /// tsc-port: isReducibleIntersection @6.0.3
    /// tsc-hash: 3aecd44e7a52b5337e106837a97753d45348309fb3ec2d448aa93e488b231ccf
    /// tsc-span: _tsc.js:59321-59324
    pub(crate) fn is_generic_reducible_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION)
            && self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::CONTAINS_INTERSECTIONS)
        {
            let members = self.union_members_or_self(ty);
            for member in members {
                if self.is_generic_reducible_type(member)? {
                    return Ok(true);
                }
            }
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            return self.is_reducible_intersection(ty);
        }
        Ok(false)
    }

    fn is_reducible_intersection(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let unique_filled = match self
            .links
            .ty(ty)
            .unique_literal_filled_instantiation
            .resolved()
        {
            Some(cached) => cached,
            None => {
                let mapper = self.unique_literal_mapper;
                let filled = self.instantiate_type(ty, Some(mapper))?;
                self.links.set_type_unique_literal_filled_instantiation(
                    self.speculation_depth,
                    ty,
                    filled,
                );
                filled
            }
        };
        Ok(self.get_reduced_type(unique_filled)? != unique_filled)
    }

    /// tsc-port: getIndexTypeForGenericType @6.0.3
    /// tsc-hash: dd50d78b15b0cf1329a53a526c20a55ce1ffd277d902b105da86d535f2f30a9b
    /// tsc-span: _tsc.js:61932-61934
    pub(crate) fn get_index_type_for_generic_type(
        &mut self,
        ty: TypeId,
        index_flags: IndexFlags,
    ) -> TypeId {
        let strings_only = index_flags.intersects(IndexFlags::STRINGS_ONLY);
        let cached = if strings_only {
            self.links.ty(ty).resolved_string_index_type.resolved()
        } else {
            self.links.ty(ty).resolved_index_type.resolved()
        };
        if let Some(cached) = cached {
            return cached;
        }
        let created = self.tables.create_type(
            TypeFlags::INDEX,
            TypeData::Index {
                ty,
                index_flags: if strings_only {
                    IndexFlags::STRINGS_ONLY
                } else {
                    IndexFlags::NONE
                },
            },
        );
        if strings_only {
            self.links
                .set_type_resolved_string_index_type(self.speculation_depth, ty, created);
        } else {
            self.links
                .set_type_resolved_index_type(self.speculation_depth, ty, created);
        }
        created
    }

    /// tsc-port: getLiteralTypeFromProperties @6.0.3
    /// tsc-hash: 6a5c07650014a8132dd7320001039548749a26888d4f112fe898f13f8388e793
    /// tsc-span: _tsc.js:61999-62012
    ///
    /// The `info !== enumNumberIndexInfo` exclusion keeps the enum
    /// reverse-mapping `[number]: string` row out of `keyof typeof E`
    /// (the synthesis is resolve-enum's is_enum_number_index_info
    /// marker — the port has no singleton to compare against).
    fn get_literal_type_from_properties(
        &mut self,
        ty: TypeId,
        include: TypeFlags,
        include_origin: bool,
    ) -> CheckResult2<TypeId> {
        let origin = if include_origin
            && (self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::CLASS_OR_INTERFACE | ObjectFlags::REFERENCE)
                || self.tables.type_of(ty).alias_symbol.is_some())
        {
            Some(self.tables.create_type(
                TypeFlags::INDEX,
                TypeData::Index {
                    ty,
                    index_flags: IndexFlags::NONE,
                },
            ))
        } else {
            None
        };
        let properties = self.get_properties_of_type_full(ty)?;
        let mut key_types: Vec<TypeId> = Vec::with_capacity(properties.len());
        for property in properties {
            key_types.push(self.get_literal_type_from_property(
                property, include, /*include_non_public*/ false,
            )?);
        }
        for info in self.get_index_infos_of_type(ty)? {
            let included = !info.is_enum_number_index_info
                && self.is_key_type_included(info.key_type, include);
            key_types.push(if included {
                if info.key_type == self.tables.intrinsics.string
                    && include.intersects(TypeFlags::NUMBER)
                {
                    self.tables.intrinsics.string_or_number
                } else {
                    info.key_type
                }
            } else {
                self.tables.intrinsics.never
            });
        }
        self.get_union_type_ex_with_origin(&key_types, UnionReduction::Literal, None, None, origin)
    }

    /// tsc-port: getLiteralTypeFromProperty @6.0.3
    /// tsc-hash: 9d83d97c724868ed6e750a37582b0daf8d46a73c7602cf31f7be051742d8ab22
    /// tsc-span: _tsc.js:61985-61995
    ///
    /// getLateBoundSymbol is the identity (late binding is 5.3) and
    /// links.nameType is unset for every constructible symbol — the
    /// declaration-name path carries all traffic.
    pub(crate) fn get_literal_type_from_property(
        &mut self,
        property: SymbolId,
        include: TypeFlags,
        include_non_public: bool,
    ) -> CheckResult2<TypeId> {
        let non_public = !include_non_public
            && self
                .binder
                .symbol(property)
                .value_declaration
                .is_some_and(|declaration| {
                    let source = self.binder.source_of_node(declaration);
                    tsrs2_binder::node_util::has_syntactic_modifier(
                        source,
                        declaration,
                        tsrs2_types::ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER,
                    )
                });
        if non_public {
            return Ok(self.tables.intrinsics.never);
        }
        let name_type = self.links.symbol(property).name_type;
        let ty = match name_type {
            Some(name_type) => Some(name_type),
            None => {
                if self.binder.symbol(property).escaped_name == "default" {
                    Some(self.tables.get_string_literal_type("default"))
                } else {
                    let name = self
                        .binder
                        .symbol(property)
                        .value_declaration
                        .and_then(|declaration| self.name_of_node(declaration));
                    match name {
                        Some(name) => Some(self.get_literal_type_from_property_name(name)?),
                        None => {
                            let display = self.symbol_display_name(property);
                            Some(self.tables.get_string_literal_type(&display))
                        }
                    }
                }
            }
        };
        Ok(match ty {
            Some(ty) if self.tables.flags_of(ty).intersects(include) => ty,
            _ => self.tables.intrinsics.never,
        })
    }

    /// tsc-port: getLiteralTypeFromPropertyName @6.0.3
    /// tsc-hash: fa7862491ef349a197dec7f1b3d009c68b657365739393d52ea2257b31745549
    /// tsc-span: _tsc.js:61964-61984
    ///
    /// Numeric names take the literal directly (checkExpression on a
    /// NumericLiteral reduces to it); computed names and expression
    /// names are 5.5 rows.
    pub(crate) fn get_literal_type_from_property_name(
        &mut self,
        name: NodeId,
    ) -> CheckResult2<TypeId> {
        match self.data_of(name) {
            NodeData::PrivateIdentifier(_) => Ok(self.tables.intrinsics.never),
            NodeData::NumericLiteral(data) => {
                let value = crate::annotate::parse_numeric_literal_text(&data.text)?;
                Ok(self.tables.get_number_literal_type(value))
            }
            NodeData::ComputedPropertyName(_) => {
                let ty = self.check_computed_property_name(name)?;
                Ok(self.tables.get_regular_type_of_literal_type(ty))
            }
            NodeData::Identifier(data) => {
                Ok(self
                    .tables
                    .get_string_literal_type(tsrs2_binder::unescape_leading_underscores(
                        &data.escaped_text,
                    )))
            }
            NodeData::StringLiteral(data) => {
                let text = data.text.clone();
                Ok(self.tables.get_string_literal_type(&text))
            }
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                let text = data.text.clone();
                Ok(self.tables.get_string_literal_type(&text))
            }
            // isExpression(name) tail (61979-61981): remaining name
            // kinds (BigIntLiteral et al) check as expressions; true
            // non-expressions bottom out in checkExpression's own
            // escape rather than tsc's neverType tail (no such name
            // kind parses today).
            _ => {
                let ty = self.check_expression(name, tsrs2_types::CheckMode::NORMAL)?;
                Ok(self.tables.get_regular_type_of_literal_type(ty))
            }
        }
    }

    /// tsc-port: isKeyTypeIncluded @6.0.3
    /// tsc-hash: 627ab52c9396f88c5869a787e23e979231a503ecaab483b1cb8f8e2fa472d686
    /// tsc-span: _tsc.js:61996-61998
    fn is_key_type_included(&self, key_type: TypeId, include: TypeFlags) -> bool {
        if self.tables.flags_of(key_type).intersects(include) {
            return true;
        }
        if self
            .tables
            .flags_of(key_type)
            .intersects(TypeFlags::INTERSECTION)
        {
            if let TypeData::Intersection { types } = &self.tables.type_of(key_type).data {
                return types
                    .iter()
                    .any(|&member| self.is_key_type_included(member, include));
            }
        }
        false
    }

    /// tsc-port: maybeTypeOfKind @6.0.3
    /// tsc-hash: f741e7c78ead28c1af6f8f4db9c6784b186cf4c525a2c8b26dc8a7eb8921abc6
    /// tsc-span: _tsc.js:79514-79527
    pub(crate) fn maybe_type_of_kind(&self, ty: TypeId, kind: TypeFlags) -> bool {
        if self.tables.flags_of(ty).intersects(kind) {
            return true;
        }
        if self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::UNION_OR_INTERSECTION)
        {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("union/intersection flag implies member data"),
            };
            return members
                .into_iter()
                .any(|member| self.maybe_type_of_kind(member, kind));
        }
        false
    }

    /// tsc-port: isTypeAssignableToKind @6.0.3
    /// tsc-hash: 9947e108a8d8346b18e05ddde0d062a6301a51bdd539b7f9608e852afdec5f4a
    /// tsc-span: _tsc.js:79528-79533
    pub(crate) fn is_type_assignable_to_kind(
        &mut self,
        source: TypeId,
        kind: TypeFlags,
        strict: bool,
    ) -> CheckResult2<bool> {
        if self.tables.flags_of(source).intersects(kind) {
            return Ok(true);
        }
        if strict
            && self.tables.flags_of(source).intersects(
                TypeFlags::ANY_OR_UNKNOWN
                    | TypeFlags::VOID
                    | TypeFlags::UNDEFINED
                    | TypeFlags::NULL,
            )
        {
            return Ok(false);
        }
        let checks: [(TypeFlags, TypeId); 10] = [
            (TypeFlags::NUMBER_LIKE, self.tables.intrinsics.number),
            (TypeFlags::BIG_INT_LIKE, self.tables.intrinsics.bigint),
            (TypeFlags::STRING_LIKE, self.tables.intrinsics.string),
            (TypeFlags::BOOLEAN_LIKE, self.tables.intrinsics.boolean),
            (TypeFlags::VOID, self.tables.intrinsics.void),
            (TypeFlags::NEVER, self.tables.intrinsics.never),
            (TypeFlags::NULL, self.tables.intrinsics.null),
            (TypeFlags::UNDEFINED, self.tables.intrinsics.undefined),
            (TypeFlags::ES_SYMBOL, self.tables.intrinsics.es_symbol),
            (
                TypeFlags::NON_PRIMITIVE,
                self.tables.intrinsics.non_primitive,
            ),
        ];
        for (flag, target) in checks {
            if kind.intersects(flag) && self.is_type_assignable_to(source, target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ---- indexed access ----

    /// tsc-port: getIndexedAccessType @6.0.3
    /// tsc-hash: f8acbbaa1df7a3ef914d26d5603ecf0be0fb2bd3902e694c8b699fcbb8c094ec
    /// tsc-span: _tsc.js:62552-62554
    pub fn get_indexed_access_type(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        access_flags: AccessFlags,
        access_node: Option<NodeId>,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let resolved = self.get_indexed_access_type_or_undefined(
            object_type,
            index_type,
            access_flags,
            access_node,
            alias_symbol,
            alias_type_arguments,
        )?;
        Ok(resolved.unwrap_or(if access_node.is_some() {
            self.tables.intrinsics.error
        } else {
            self.tables.intrinsics.unknown
        }))
    }

    /// tsc-port: getActualTypeVariable @6.0.3
    /// tsc-hash: 194568789a48a4a0e08ba0b934c9acafdf81244913ba01c0b5e42a0ca99c5983
    /// tsc-span: _tsc.js:62631-62639
    ///
    pub(crate) fn get_actual_type_variable(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::SUBSTITUTION) {
            let TypeData::Substitution(data) = self.tables.type_of(ty).data.clone() else {
                unreachable!("Substitution flag implies substitution data");
            };
            return self.get_actual_type_variable(data.base_type);
        }
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            let TypeData::IndexedAccess {
                object_type,
                index_type,
                ..
            } = self.tables.type_of(ty).data
            else {
                unreachable!("IndexedAccess flag implies data");
            };
            if self
                .tables
                .flags_of(object_type)
                .intersects(TypeFlags::SUBSTITUTION)
                || self
                    .tables
                    .flags_of(index_type)
                    .intersects(TypeFlags::SUBSTITUTION)
            {
                let actual_object = self.get_actual_type_variable(object_type)?;
                let actual_index = self.get_actual_type_variable(index_type)?;
                return self.get_indexed_access_type(
                    actual_object,
                    actual_index,
                    AccessFlags::NONE,
                    None,
                    None,
                    None,
                );
            }
        }
        Ok(ty)
    }

    /// tsc-port: isGenericType @6.0.3
    /// tsc-hash: 24d5fea9e8bd61b2e658f4c5d9423ee7926ff364e97370617a604cd6e4b0f5c0
    /// tsc-span: _tsc.js:62431-62433
    pub(crate) fn is_generic_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        Ok(!self.get_generic_object_flags(ty)?.is_empty())
    }

    /// tsrs-native: fallible checker-owned projection of
    /// getGenericObjectFlags' object half.
    pub(crate) fn is_generic_object_type_state(&mut self, ty: TypeId) -> CheckResult2<bool> {
        Ok(self
            .get_generic_object_flags(ty)?
            .intersects(ObjectFlags::IS_GENERIC_OBJECT_TYPE))
    }

    /// tsrs-native: fallible checker-owned projection of
    /// getGenericObjectFlags' index half.
    pub(crate) fn is_generic_index_type_state(&mut self, ty: TypeId) -> CheckResult2<bool> {
        Ok(self
            .get_generic_object_flags(ty)?
            .intersects(ObjectFlags::IS_GENERIC_INDEX_TYPE))
    }

    /// tsc-port: getGenericObjectFlags @6.0.3
    /// tsc-hash: 7ea070b17fceb8ba8275f5641dccb325aeff6b32c33b499e1de70b696192d94f
    /// tsc-span: _tsc.js:62440-62454
    ///
    /// The IsGenericTypeComputed memo (62442/62448) is elided: the
    /// port's type tables are append-only, so the composite reduction
    /// recomputes per query — cache-only deviation, verdict-identical.
    pub(crate) fn get_generic_object_flags(&mut self, ty: TypeId) -> CheckResult2<ObjectFlags> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION | TypeFlags::INTERSECTION) {
            let (TypeData::Union { types, .. } | TypeData::Intersection { types }) =
                &self.tables.type_of(ty).data
            else {
                unreachable!("UnionOrIntersection flag implies member data");
            };
            let types = types.to_vec();
            let mut reduced = ObjectFlags::NONE;
            for member in types {
                reduced |= self.get_generic_object_flags(member)?;
            }
            return Ok(reduced & ObjectFlags::IS_GENERIC_TYPE);
        }
        if flags.intersects(TypeFlags::SUBSTITUTION) {
            let TypeData::Substitution(data) = self.tables.type_of(ty).data.clone() else {
                unreachable!("Substitution flag implies substitution data");
            };
            return Ok((self.get_generic_object_flags(data.base_type)?
                | self.get_generic_object_flags(data.constraint)?)
                & ObjectFlags::IS_GENERIC_TYPE);
        }
        let object_half = if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
            || self.is_generic_mapped_type_state(ty)?
            || self.is_generic_tuple_type(ty)
        {
            ObjectFlags::IS_GENERIC_OBJECT_TYPE
        } else {
            ObjectFlags::NONE
        };
        let index_half = if flags
            .intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE | TypeFlags::INDEX)
            || self.is_generic_string_like_type(ty)
        {
            ObjectFlags::IS_GENERIC_INDEX_TYPE
        } else {
            ObjectFlags::NONE
        };
        Ok(object_half | index_half)
    }

    /// tsc-port: getSimplifiedType @6.0.3
    /// tsc-hash: ac9c7ff358d22383776d3bf0d841ff1a331b67e77b87198ef19ad9398418adc5
    /// tsc-span: _tsc.js:62455-62457
    ///
    /// The Conditional arm is M8 (those TypeFlags are unconstructible
    /// before conditional type nodes land) — the escape fires if one
    /// ever appears rather than mis-simplifying.
    pub(crate) fn get_simplified_type(
        &mut self,
        ty: TypeId,
        writing: bool,
    ) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            self.get_simplified_indexed_access_type(ty, writing)
        } else if flags.intersects(TypeFlags::CONDITIONAL) {
            // tsc-dormant: canary=conditional_resolution; owner=9.6c
            Err(Unsupported::new("getSimplifiedConditionalType (9.6c)"))
        } else if flags.intersects(TypeFlags::INDEX) {
            self.get_simplified_index_type(ty)
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: getSimplifiedIndexType @6.0.3
    /// tsc-hash: b26e70997196efd7e1f1d3d114b5ff87f5ea045e5856d5925477022d8f4c120c
    /// tsc-span: _tsc.js:62527-62532
    ///
    fn get_simplified_index_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let TypeData::Index { ty: inner, .. } = self.tables.type_of(ty).data else {
            unreachable!("index flag implies index data");
        };
        if self.is_generic_mapped_type_state(inner)?
            && self.get_name_type_from_mapped_type(inner)?.is_some()
            && !self.is_mapped_type_with_keyof_constraint_declaration(inner)
        {
            return self.get_index_type_for_mapped_type(inner, IndexFlags::NONE);
        }
        Ok(ty)
    }

    /// tsc-port: distributeIndexOverObjectType @6.0.3
    /// tsc-hash: c1b5b66a9950b720634f5c0aed06b0c81a123fc583dab949f49b160b7ee679dc
    /// tsc-span: _tsc.js:62458-62463
    pub(crate) fn distribute_index_over_object_type(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        writing: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let object_flags = self.tables.flags_of(object_type);
        let distributes = object_flags.intersects(TypeFlags::UNION)
            || (object_flags.intersects(TypeFlags::INTERSECTION)
                && !self.should_defer_index_type(object_type, IndexFlags::NONE)?);
        if !distributes {
            return Ok(None);
        }
        let members: Vec<TypeId> = match &self.tables.type_of(object_type).data {
            TypeData::Union { types, .. } => types.to_vec(),
            TypeData::Intersection { types } => types.to_vec(),
            _ => unreachable!("union/intersection flag implies member data"),
        };
        let mut types = Vec::with_capacity(members.len());
        for member in members {
            let access = self.get_indexed_access_type(
                member,
                index_type,
                AccessFlags::NONE,
                None,
                None,
                None,
            )?;
            types.push(self.get_simplified_type(access, writing)?);
        }
        Ok(Some(
            if object_flags.intersects(TypeFlags::INTERSECTION) || writing {
                self.get_intersection_type(&types, IntersectionFlags::NONE)?
            } else {
                self.get_union_type_ex(&types, UnionReduction::Literal)?
            },
        ))
    }

    /// tsc-port: distributeObjectOverIndexType @6.0.3
    /// tsc-hash: 3d3e019d985056c1232c73944bbfae00247b8d401992815f6960bcb9b9180155
    /// tsc-span: _tsc.js:62464-62469
    fn distribute_object_over_index_type(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        writing: bool,
    ) -> CheckResult2<Option<TypeId>> {
        if !self
            .tables
            .flags_of(index_type)
            .intersects(TypeFlags::UNION)
        {
            return Ok(None);
        }
        let members: Vec<TypeId> = match &self.tables.type_of(index_type).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies union data"),
        };
        let mut types = Vec::with_capacity(members.len());
        for member in members {
            let access = self.get_indexed_access_type(
                object_type,
                member,
                AccessFlags::NONE,
                None,
                None,
                None,
            )?;
            types.push(self.get_simplified_type(access, writing)?);
        }
        Ok(Some(if writing {
            self.get_intersection_type(&types, IntersectionFlags::NONE)?
        } else {
            self.get_union_type_ex(&types, UnionReduction::Literal)?
        }))
    }

    /// tsc-port: getSimplifiedIndexedAccessType @6.0.3
    /// tsc-hash: 7945a007d8d874e90e9a46305d0a6b9e0240ecad9e9bf00838d64f878c61adb3
    /// tsc-span: _tsc.js:62470-62506
    fn get_simplified_indexed_access_type(
        &mut self,
        ty: TypeId,
        writing: bool,
    ) -> CheckResult2<TypeId> {
        {
            let links = self.links.ty(ty);
            let slot = if writing {
                &links.simplified_for_writing
            } else {
                &links.simplified_for_reading
            };
            if slot.is_resolving() {
                // The circular sentinel: mid-flight re-entry reads the
                // type unsimplified (62472-62474).
                return Ok(ty);
            }
            if let Some(cached) = slot.resolved() {
                return Ok(cached);
            }
        }
        self.links
            .set_type_simplified(self.speculation_depth, ty, writing, LinkSlot::Resolving);
        match self.simplified_indexed_access_worker(ty, writing) {
            Ok(simplified) => {
                self.links.set_type_simplified(
                    self.speculation_depth,
                    ty,
                    writing,
                    LinkSlot::Resolved(simplified),
                );
                Ok(simplified)
            }
            Err(err) => {
                self.links.revert_type_simplified(ty, writing);
                Err(err)
            }
        }
    }

    /// getSimplifiedIndexedAccessType's body (62476-62505), split out
    /// so the cache wrapper can park/revert the sentinel around it.
    fn simplified_indexed_access_worker(
        &mut self,
        ty: TypeId,
        writing: bool,
    ) -> CheckResult2<TypeId> {
        let TypeData::IndexedAccess {
            object_type,
            index_type,
            ..
        } = self.tables.type_of(ty).data
        else {
            unreachable!("indexed-access flag implies indexed-access data");
        };
        let object_type = self.get_simplified_type(object_type, writing)?;
        let index_type = self.get_simplified_type(index_type, writing)?;
        if let Some(distributed) =
            self.distribute_object_over_index_type(object_type, index_type, writing)?
        {
            return Ok(distributed);
        }
        if !self
            .tables
            .flags_of(index_type)
            .intersects(TypeFlags::INSTANTIABLE)
        {
            if let Some(distributed) =
                self.distribute_index_over_object_type(object_type, index_type, writing)?
            {
                return Ok(distributed);
            }
        }
        if self.is_generic_tuple_type(object_type)
            && self
                .tables
                .flags_of(index_type)
                .intersects(TypeFlags::NUMBER_LIKE)
        {
            let start = if self
                .tables
                .flags_of(index_type)
                .intersects(TypeFlags::NUMBER)
            {
                0
            } else {
                let target = self.tables.reference_target(object_type);
                match &self.tables.type_of(target).data {
                    TypeData::TupleTarget(data) => data.fixed_length,
                    _ => unreachable!("tuple type targets a tuple target"),
                }
            };
            if let Some(element) =
                self.get_element_type_of_slice_of_tuple_type(object_type, start, 0, writing, false)?
            {
                return Ok(element);
            }
        }
        if self.is_generic_mapped_type_state(object_type)?
            && self.get_mapped_type_name_type_kind(object_type)?
                != crate::mapped::MappedTypeNameTypeKind::Remapping
        {
            let substituted = self.substitute_indexed_mapped_type(object_type, index_type)?;
            let mapped = self.map_type(
                substituted,
                &mut |state, member| Ok(Some(state.get_simplified_type(member, writing)?)),
                /*no_reductions*/ false,
            )?;
            return Ok(mapped.expect("mapped indexed substitution never drops a member"));
        }
        Ok(ty)
    }

    /// tsc-port: indexTypeLessThan @6.0.3
    /// tsc-hash: ae6f29f3745e3a4682f73162a107c56d078200a413c7145d832743792f287388
    /// tsc-span: _tsc.js:62555-62566
    fn index_type_less_than(&self, index_type: TypeId, limit: usize) -> bool {
        self.every_type(index_type, |state, t| {
            if state
                .tables
                .flags_of(t)
                .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL)
            {
                if let Some(name) = state.property_name_from_type(t) {
                    if is_numeric_literal_name(&name) {
                        let index: f64 = name.parse().unwrap_or(-1.0);
                        return index >= 0.0 && (index as usize) < limit;
                    }
                }
            }
            false
        })
    }

    /// tsc-port: getIndexedAccessTypeOrUndefined @6.0.3
    /// tsc-hash: 844cc355aa4db18d1b78368c76ff6c5ab181b0c47bded309986ffc1bcb4073cd
    /// tsc-span: _tsc.js:62567-62611
    ///
    /// 5.5d: expression access nodes are live — the ExpressionPosition
    /// → IncludeUndefined promotion (62575) and the non-type-node
    /// generic-tuple gate branch transcribe in full.
    pub fn get_indexed_access_type_or_undefined(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        access_flags: AccessFlags,
        access_node: Option<NodeId>,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<Option<TypeId>> {
        self.get_indexed_access_type_or_undefined_ex(
            object_type,
            index_type,
            access_flags,
            access_node,
            alias_symbol,
            alias_type_arguments,
            /*synthetic_access*/ false,
        )
    }

    /// The synthetic_access flavor: tsc threads a SyntheticExpression
    /// (createSyntheticExpression 76289) as the access node from the
    /// destructuring band — its kind matches NONE of the access-node
    /// probes, so the access-expression band (mark-referenced,
    /// readonly, flow tail) and the getIndexNodeForAccessExpression
    /// unwrap stay off while spans still point at the element.
    #[allow(clippy::too_many_arguments)]
    pub fn get_indexed_access_type_or_undefined_ex(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        access_flags: AccessFlags,
        access_node: Option<NodeId>,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
        synthetic_access: bool,
    ) -> CheckResult2<Option<TypeId>> {
        if object_type == self.tables.intrinsics.wildcard
            || index_type == self.tables.intrinsics.wildcard
        {
            return Ok(Some(self.tables.intrinsics.wildcard));
        }
        let object_type = self.get_reduced_type(object_type)?;
        let mut access_flags = access_flags;
        let mut index_type = index_type;
        if self.is_string_index_signature_only_type(object_type)?
            && !self
                .tables
                .flags_of(index_type)
                .intersects(TypeFlags::NULLABLE)
            && self.is_type_assignable_to_kind(
                index_type,
                TypeFlags::from_bits(TypeFlags::STRING.bits() | TypeFlags::NUMBER.bits()),
                /*strict*/ false,
            )?
        {
            index_type = self.tables.intrinsics.string;
        }
        if self.options.no_unchecked_indexed_access == Some(true)
            && access_flags.intersects(AccessFlags::EXPRESSION_POSITION)
        {
            access_flags =
                AccessFlags::from_bits(access_flags.bits() | AccessFlags::INCLUDE_UNDEFINED.bits());
        }
        // 62576 deferral asymmetry: EXPRESSION access nodes defer only
        // generic TUPLES; type nodes defer any generic object.
        let generic = self.is_generic_index_type_state(index_type)? || {
            let expression_access =
                access_node.is_some_and(|node| self.kind_of(node) != SyntaxKind::IndexedAccessType);
            let tuple_fixed_access = self.tables.is_tuple_type(object_type) && {
                let target = self.tables.reference_target(object_type);
                let limit = self.get_total_fixed_element_count(target);
                self.index_type_less_than(index_type, limit)
            };
            if expression_access {
                self.tables.is_generic_tuple_type(object_type) && !tuple_fixed_access
            } else {
                let object_generic = self.is_generic_object_type_state(object_type)?;
                (object_generic && !tuple_fixed_access)
                    || self.is_generic_reducible_type(object_type)?
            }
        };
        if generic {
            if self
                .tables
                .flags_of(object_type)
                .intersects(TypeFlags::ANY_OR_UNKNOWN)
            {
                return Ok(Some(object_type));
            }
            let persistent =
                AccessFlags::from_bits(access_flags.bits() & AccessFlags::PERSISTENT.bits());
            return Ok(Some(self.tables.get_indexed_access_type_interned(
                object_type,
                index_type,
                persistent,
                alias_symbol,
                alias_type_arguments,
            )));
        }
        let apparent_object_type = self.get_reduced_apparent_type(object_type)?;
        if self
            .tables
            .flags_of(index_type)
            .intersects(TypeFlags::UNION)
            && !self
                .tables
                .flags_of(index_type)
                .intersects(TypeFlags::BOOLEAN)
        {
            let members = self.union_members_or_self(index_type);
            let mut property_types: Vec<TypeId> = Vec::with_capacity(members.len());
            let mut was_missing_property = false;
            for member in members {
                let member_flags = AccessFlags::from_bits(
                    access_flags.bits()
                        | if was_missing_property {
                            AccessFlags::SUPPRESS_NO_IMPLICIT_ANY_ERROR.bits()
                        } else {
                            0
                        },
                );
                let property_type = self.get_property_type_for_index_type_ex(
                    object_type,
                    apparent_object_type,
                    member,
                    index_type,
                    access_node,
                    member_flags,
                    synthetic_access,
                )?;
                match property_type {
                    Some(property_type) => property_types.push(property_type),
                    None if access_node.is_none() => return Ok(None),
                    None => was_missing_property = true,
                }
            }
            if was_missing_property {
                return Ok(None);
            }
            return Ok(Some(if access_flags.intersects(AccessFlags::WRITING) {
                self.get_intersection_type_ex(
                    &property_types,
                    IntersectionFlags::NONE,
                    alias_symbol,
                    alias_type_arguments,
                )?
            } else {
                self.get_union_type_ex_with_origin(
                    &property_types,
                    UnionReduction::Literal,
                    alias_symbol,
                    alias_type_arguments,
                    None,
                )?
            }));
        }
        let full_flags = AccessFlags::from_bits(
            access_flags.bits()
                | AccessFlags::CACHE_SYMBOL.bits()
                | AccessFlags::REPORT_DEPRECATED.bits(),
        );
        self.get_property_type_for_index_type_ex(
            object_type,
            apparent_object_type,
            index_type,
            index_type,
            access_node,
            full_flags,
            synthetic_access,
        )
    }

    /// tsc-port: getPropertyTypeForIndexType @6.0.3
    /// tsc-hash: 4014bff2f311f9e1f8746a10a48831bc25867f25d8548e320ea8ead8c5afa786
    /// tsc-span: _tsc.js:62211-62410
    ///
    /// Full transcription (5.5d): expression access nodes drive the
    /// noImplicitAny ladder (2576/7015/2551/7052/7053+7054 — the
    /// LADDER ORDER is the observable, risk #1), the write rows, and
    /// the flow tail. Escapes, each named: displays outside the T2
    /// slice (tuples/anonymous shapes render through nodeBuilder),
    /// deprecation suggestions (JSDoc band), the Contextual arm's miss
    /// fallback (anyType — M6 wires the flag), autoType flow ([FLOW
    /// M5] via the caller's flow tail), unique-symbol index heads
    /// (unique symbol types unconstructible until their annotate arm).
    #[allow(clippy::too_many_arguments)]
    fn get_property_type_for_index_type_ex(
        &mut self,
        original_object_type: TypeId,
        object_type: TypeId,
        index_type: TypeId,
        full_index_type: TypeId,
        access_node: Option<NodeId>,
        access_flags: AccessFlags,
        synthetic_access: bool,
    ) -> CheckResult2<Option<TypeId>> {
        // A synthetic access node (tsc SyntheticExpression) matches no
        // access-node kind probe: the element-access band stays off
        // and index-node unwraps answer the node itself.
        let access_expression = if synthetic_access {
            None
        } else {
            access_node.filter(|&node| self.kind_of(node) == SyntaxKind::ElementAccessExpression)
        };
        let property_name = if access_node
            .is_some_and(|node| self.kind_of(node) == SyntaxKind::PrivateIdentifier)
        {
            None
        } else {
            // getPropertyNameFromIndex: the isPropertyName accessNode
            // fallback serves computed-name/late-bound positions whose
            // producers resolve names before reaching here.
            self.property_name_from_type_usable(index_type)
        };
        if let Some(property_name) = &property_name {
            if access_flags.intersects(AccessFlags::CONTEXTUAL) {
                let contextual =
                    self.get_type_of_property_of_contextual_type(object_type, property_name, None)?;
                return Ok(Some(contextual.unwrap_or(self.tables.intrinsics.any)));
            }
            let property = self.get_property_of_type_full(object_type, property_name)?;
            if let Some(property) = property {
                // ReportDeprecated suggestions elided (JSDoc band).
                if let Some(access_expression) = access_expression {
                    let receiver = match self.data_of(access_expression) {
                        NodeData::ElementAccessExpression(data) => data.expression,
                        _ => None,
                    };
                    let object_symbol = self.tables.type_of(object_type).symbol;
                    let self_access = match receiver {
                        Some(receiver) => self.is_self_type_access(receiver, object_symbol)?,
                        None => false,
                    };
                    self.mark_property_as_referenced(
                        property,
                        Some(access_expression),
                        self_access,
                    );
                    let assignment_kind = self.get_assignment_target_kind(access_expression);
                    if self.is_assignment_to_readonly_entity(
                        access_expression,
                        property,
                        assignment_kind,
                    )? {
                        let argument = match self.data_of(access_expression) {
                            NodeData::ElementAccessExpression(data) => data.argument_expression,
                            _ => None,
                        };
                        let display = self.symbol_display_name(property);
                        self.error_at(
                            argument.or(Some(access_expression)),
                            &diagnostics::Cannot_assign_to_0_because_it_is_a_read_only_property,
                            &[&display],
                        );
                        return Ok(None);
                    }
                    if access_flags.intersects(AccessFlags::CACHE_SYMBOL) {
                        self.links.set_node_resolved_symbol(
                            self.speculation_depth,
                            access_node.expect("access expression implies node"),
                            property,
                        );
                    }
                    if self.is_this_property_access_in_constructor(access_expression, property)? {
                        return Ok(Some(self.tables.intrinsics.auto));
                    }
                }
                let property_type = if access_flags.intersects(AccessFlags::WRITING) {
                    self.get_write_type_of_symbol(property)?
                } else {
                    self.get_type_of_symbol(property)?
                };
                return Ok(Some(match access_expression {
                    Some(access_expression)
                        if self.get_assignment_target_kind(access_expression)
                            != crate::expr::AssignmentKind::Definite =>
                    {
                        self.get_flow_type_of_reference(
                            access_expression,
                            property_type,
                            property_type,
                            None,
                        )?
                    }
                    _ => {
                        let type_node_missing =
                            access_node.is_some_and(|node| {
                                self.kind_of(node) == SyntaxKind::IndexedAccessType
                            }) && self.tables.contains_missing_type(property_type);
                        if type_node_missing {
                            let undefined = self.tables.intrinsics.undefined;
                            self.get_union_type_ex(
                                &[property_type, undefined],
                                UnionReduction::Literal,
                            )?
                        } else {
                            property_type
                        }
                    }
                }));
            }
            if self.every_type(object_type, |state, t| state.tables.is_tuple_type(t))
                && is_numeric_literal_name(property_name)
            {
                let index: f64 = property_name.parse().unwrap_or(-1.0);
                let all_fixed = self.every_type(object_type, |state, t| {
                    let target = state.tables.reference_target(t);
                    match &state.tables.type_of(target).data {
                        TypeData::TupleTarget(data) => !data
                            .combined_flags
                            .intersects(tsrs2_types::ElementFlags::VARIABLE),
                        _ => false,
                    }
                });
                if let Some(node) = access_node {
                    if all_fixed && !access_flags.intersects(AccessFlags::ALLOW_MISSING) {
                        let index_node = if synthetic_access {
                            node
                        } else {
                            self.get_index_node_for_access_expression(node)
                        };
                        if self.tables.is_tuple_type(object_type) {
                            if index < 0.0 {
                                self.error_at(
                                    Some(index_node),
                                    &diagnostics::A_tuple_type_cannot_be_indexed_with_a_negative_value,
                                    &[],
                                );
                                return Ok(Some(self.tables.intrinsics.undefined));
                            }
                            let tuple_display = self.type_to_string_slice(object_type)?;
                            let target = self.tables.reference_target(object_type);
                            let arity = match &self.tables.type_of(target).data {
                                TypeData::TupleTarget(data) => data.element_flags.len(),
                                _ => 0,
                            };
                            self.error_at(
                                Some(index_node),
                                &diagnostics::Tuple_type_0_of_length_1_has_no_element_at_index_2,
                                &[&tuple_display, &arity.to_string(), property_name],
                            );
                        } else {
                            let object_display = self.type_to_string_slice(object_type)?;
                            self.error_at(
                                Some(index_node),
                                &diagnostics::Property_0_does_not_exist_on_type_1,
                                &[property_name, &object_display],
                            );
                        }
                    }
                }
                if index >= 0.0 {
                    let number = self.tables.intrinsics.number;
                    let number_info = self
                        .get_index_infos_of_type(object_type)?
                        .into_iter()
                        .find(|info| info.key_type == number);
                    self.error_if_writing_to_readonly_index(
                        number_info.as_ref(),
                        object_type,
                        access_expression,
                    )?;
                    return Ok(Some(
                        self.get_tuple_element_type_out_of_start_count(
                            object_type,
                            index as usize,
                            access_flags
                                .intersects(AccessFlags::INCLUDE_UNDEFINED)
                                .then_some(self.tables.intrinsics.missing),
                        )?,
                    ));
                }
            }
        }
        if !self
            .tables
            .flags_of(index_type)
            .intersects(TypeFlags::NULLABLE)
            && self.is_type_assignable_to_kind(
                index_type,
                TypeFlags::from_bits(
                    TypeFlags::STRING_LIKE.bits()
                        | TypeFlags::NUMBER_LIKE.bits()
                        | TypeFlags::ES_SYMBOL_LIKE.bits(),
                ),
                /*strict*/ false,
            )?
        {
            if self
                .tables
                .flags_of(object_type)
                .intersects(TypeFlags::ANY | TypeFlags::NEVER)
            {
                return Ok(Some(object_type));
            }
            let index_info = match self.get_applicable_index_info(object_type, index_type)? {
                Some(info) => Some(info),
                None => {
                    let string = self.tables.intrinsics.string;
                    self.get_index_infos_of_type(object_type)?
                        .into_iter()
                        .find(|info| info.key_type == string)
                }
            };
            if let Some(index_info) = index_info {
                if access_flags.intersects(AccessFlags::NO_INDEX_SIGNATURES)
                    && index_info.key_type != self.tables.intrinsics.number
                {
                    if let Some(access_expression) = access_expression {
                        if access_flags.intersects(AccessFlags::WRITING) {
                            let display = self.type_to_string_slice(original_object_type)?;
                            self.error_at(
                                Some(access_expression),
                                &diagnostics::Type_0_is_generic_and_can_only_be_indexed_for_reading,
                                &[&display],
                            );
                        } else {
                            let index_display = self.type_to_string_slice(index_type)?;
                            let object_display = self.type_to_string_slice(original_object_type)?;
                            self.error_at(
                                Some(access_expression),
                                &diagnostics::Type_0_cannot_be_used_to_index_type_1,
                                &[&index_display, &object_display],
                            );
                        }
                    }
                    return Ok(None);
                }
                if let Some(node) = access_node {
                    if index_info.key_type == self.tables.intrinsics.string
                        && !self.is_type_assignable_to_kind(
                            index_type,
                            TypeFlags::from_bits(
                                TypeFlags::STRING.bits() | TypeFlags::NUMBER.bits(),
                            ),
                            /*strict*/ false,
                        )?
                    {
                        let index_node = if synthetic_access {
                            node
                        } else {
                            self.get_index_node_for_access_expression(node)
                        };
                        let index_display = self.type_to_string_slice(index_type)?;
                        self.error_at(
                            Some(index_node),
                            &diagnostics::Type_0_cannot_be_used_as_an_index_type,
                            &[&index_display],
                        );
                        return Ok(Some(
                            if access_flags.intersects(AccessFlags::INCLUDE_UNDEFINED) {
                                let missing = self.tables.intrinsics.missing;
                                self.get_union_type_ex(
                                    &[index_info.value_type, missing],
                                    UnionReduction::Literal,
                                )?
                            } else {
                                index_info.value_type
                            },
                        ));
                    }
                }
                self.error_if_writing_to_readonly_index(
                    Some(&index_info),
                    object_type,
                    access_expression,
                )?;
                if access_flags.intersects(AccessFlags::INCLUDE_UNDEFINED) {
                    // The enum-keyed exemption (62284-62287).
                    let enum_keyed =
                        self.tables
                            .type_of(object_type)
                            .symbol
                            .is_some_and(|object_symbol| {
                                self.binder
                                    .symbol(object_symbol)
                                    .flags
                                    .intersects(SymbolFlags::REGULAR_ENUM | SymbolFlags::CONST_ENUM)
                                    && self
                                        .tables
                                        .flags_of(index_type)
                                        .intersects(TypeFlags::ENUM_LITERAL)
                                    && self.tables.type_of(index_type).symbol.is_some_and(
                                        |index_symbol| {
                                            self.get_parent_of_symbol(index_symbol)
                                                == Some(object_symbol)
                                        },
                                    )
                            });
                    if !enum_keyed {
                        let missing = self.tables.intrinsics.missing;
                        return Ok(Some(self.get_union_type_ex(
                            &[index_info.value_type, missing],
                            UnionReduction::Literal,
                        )?));
                    }
                }
                return Ok(Some(index_info.value_type));
            }
            if self
                .tables
                .flags_of(index_type)
                .intersects(TypeFlags::NEVER)
            {
                return Ok(Some(self.tables.intrinsics.never));
            }
            if self.is_js_literal_type(object_type)? {
                return Ok(Some(self.tables.intrinsics.any));
            }
            if let Some(access_expression) = access_expression {
                if !self.is_const_enum_object_type(object_type) {
                    return self.element_access_error_ladder(
                        original_object_type,
                        object_type,
                        index_type,
                        full_index_type,
                        access_expression,
                        access_flags,
                        property_name.as_deref(),
                    );
                }
            }
        }
        if access_flags.intersects(AccessFlags::ALLOW_MISSING)
            && self
                .tables
                .object_flags_of(object_type)
                .intersects(ObjectFlags::OBJECT_LITERAL)
        {
            return Ok(Some(self.tables.intrinsics.undefined));
        }
        if self.is_js_literal_type(object_type)? {
            return Ok(Some(self.tables.intrinsics.any));
        }
        if let Some(node) = access_node {
            let index_node = if synthetic_access {
                node
            } else {
                self.get_index_node_for_access_expression(node)
            };
            let index_flags = self.tables.flags_of(index_type);
            if self.kind_of(index_node) != SyntaxKind::BigIntLiteral
                && index_flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL)
            {
                let value = self
                    .index_literal_value_display(index_type)
                    .unwrap_or_default();
                let object_display = self.type_to_string_slice(object_type)?;
                self.error_at(
                    Some(index_node),
                    &diagnostics::Property_0_does_not_exist_on_type_1,
                    &[&value, &object_display],
                );
            } else if index_flags.intersects(TypeFlags::STRING | TypeFlags::NUMBER) {
                let object_display = self.type_to_string_slice(object_type)?;
                let index_display = self.type_to_string_slice(index_type)?;
                self.error_at(
                    Some(index_node),
                    &diagnostics::Type_0_has_no_matching_index_signature_for_type_1,
                    &[&object_display, &index_display],
                );
            } else {
                let type_string = if self.kind_of(index_node) == SyntaxKind::BigIntLiteral {
                    "bigint".to_owned()
                } else {
                    self.type_to_string_slice(index_type)?
                };
                self.error_at(
                    Some(index_node),
                    &diagnostics::Type_0_cannot_be_used_as_an_index_type,
                    &[&type_string],
                );
            }
        }
        if self.tables.flags_of(index_type).intersects(TypeFlags::ANY) {
            return Ok(Some(index_type));
        }
        Ok(None)
    }

    /// The accessExpression noImplicitAny ladder (62296-62379) — the
    /// order 2339-value → props-union → globalThis → 2576 → 7015 →
    /// 2551 → 7052 → 7053(+per-kind head) is the risk-#1 observable.
    #[allow(clippy::too_many_arguments)]
    fn element_access_error_ladder(
        &mut self,
        _original_object_type: TypeId,
        object_type: TypeId,
        index_type: TypeId,
        full_index_type: TypeId,
        access_expression: NodeId,
        access_flags: AccessFlags,
        property_name: Option<&str>,
    ) -> CheckResult2<Option<TypeId>> {
        // 6.6f: the syntax-probe ladder gate retired; the residual
        // containment is FLAG-EXACT — a seam-reverted receiver or
        // index answer (an unported M6/M8 dependency crossed its
        // walk) makes every failed ladder verdict undecidable.
        let ladder_operands = match self.data_of(access_expression) {
            NodeData::ElementAccessExpression(data) => (data.expression, data.argument_expression),
            _ => (None, None),
        };
        for operand in [ladder_operands.0, ladder_operands.1].into_iter().flatten() {
            if self.flow_answer_is_seam_reverted(operand) {
                return Err(Unsupported::new(
                    "element-access ladder over a seam-reverted flow answer \
                     (unported narrowing dependency, M6/M8 seam)",
                ));
            }
        }
        let no_implicit_any = self
            .options
            .strict_option_value(self.options.no_implicit_any);
        let index_flags = self.tables.flags_of(index_type);
        if self
            .tables
            .object_flags_of(object_type)
            .intersects(ObjectFlags::OBJECT_LITERAL)
        {
            if no_implicit_any
                && index_flags.intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL)
            {
                let value = self
                    .index_literal_value_display(index_type)
                    .unwrap_or_default();
                let object_display = self.type_to_string_slice(object_type)?;
                self.error_at(
                    Some(access_expression),
                    &diagnostics::Property_0_does_not_exist_on_type_1,
                    &[&value, &object_display],
                );
                return Ok(Some(self.tables.intrinsics.undefined));
            }
            if index_flags.intersects(TypeFlags::NUMBER | TypeFlags::STRING) {
                let resolved = self.resolve_structured_type_members(object_type)?;
                let properties: Vec<SymbolId> = self
                    .members_of(resolved)
                    .members
                    .values()
                    .copied()
                    .collect();
                let mut types = Vec::with_capacity(properties.len() + 1);
                for property in properties {
                    types.push(self.get_type_of_symbol(property)?);
                }
                types.push(self.tables.intrinsics.undefined);
                return Ok(Some(
                    self.get_union_type_ex(&types, UnionReduction::Literal)?,
                ));
            }
        }
        let object_symbol = self.tables.type_of(object_type).symbol;
        // Expando-member suppression, the element-access flavor
        // (`F["prop"] = 3`): tsc's binder declares the member on the
        // function symbol (bindable static name), so the port's
        // member-less lookup would fabricate the 7053/2339 faces —
        // same disposition as report_nonexistent_property (errorType
        // continues via the caller), and NAME-PRECISE like it:
        // `foo["z"]` with no `z` assignment misses in tsc too and
        // keeps its 7053. Both faces consult: the object TYPE's
        // symbol (fn declarations) and the receiver's resolved symbol
        // (`const f = function () {}` flags the VARIABLE — the type's
        // symbol is the anonymous expression).
        if let Some(name) = property_name {
            let receiver_symbol = match self.data_of(access_expression) {
                NodeData::ElementAccessExpression(data) => data
                    .expression
                    .and_then(|receiver| self.links.node(receiver).resolved_symbol.resolved()),
                _ => None,
            };
            if [object_symbol, receiver_symbol]
                .into_iter()
                .flatten()
                .any(|symbol| self.symbol_expando_covers_merged(symbol, name))
            {
                return Ok(None);
            }
        }
        let global_this_block_scoped = object_symbol == Some(self.global_this_symbol)
            && property_name.is_some_and(|name| {
                self.globals.get(name).copied().is_some_and(|exported| {
                    self.binder
                        .symbol(exported)
                        .flags
                        .intersects(SymbolFlags::BLOCK_SCOPED)
                })
            });
        if global_this_block_scoped {
            let display =
                tsrs2_binder::unescape_leading_underscores(property_name.expect("checked above"));
            let object_display = self.type_to_string_slice(object_type)?;
            self.error_at(
                Some(access_expression),
                &diagnostics::Property_0_does_not_exist_on_type_1,
                &[display, &object_display],
            );
        } else if no_implicit_any
            && !access_flags.intersects(AccessFlags::SUPPRESS_NO_IMPLICIT_ANY_ERROR)
        {
            let static_property = match property_name {
                Some(name) => self.type_has_static_property(name, object_type)?,
                None => false,
            };
            if static_property {
                let name = property_name.expect("checked above");
                let type_name = self.type_to_string_slice(object_type)?;
                let argument = match self.data_of(access_expression) {
                    NodeData::ElementAccessExpression(data) => data.argument_expression,
                    _ => None,
                };
                let argument_text = match argument {
                    Some(argument) => {
                        let source = self.binder.source_of_node(argument);
                        let raw = source.arena.node(argument);
                        let start = tsrs2_syntax::skip_trivia(&source.text, raw.pos as usize);
                        source.text[start..raw.end as usize].to_owned()
                    }
                    None => String::new(),
                };
                let suggestion = format!("{type_name}[{argument_text}]");
                self.error_at(
                    Some(access_expression),
                    &diagnostics::Property_0_does_not_exist_on_type_1_Did_you_mean_to_access_the_static_member_2_instead,
                    &[name, &type_name, &suggestion],
                );
            } else {
                let number = self.tables.intrinsics.number;
                let has_number_index = self
                    .get_index_infos_of_type(object_type)?
                    .iter()
                    .any(|info| info.key_type == number);
                let argument = match self.data_of(access_expression) {
                    NodeData::ElementAccessExpression(data) => data.argument_expression,
                    _ => None,
                };
                if has_number_index {
                    self.error_at(
                        argument.or(Some(access_expression)),
                        &diagnostics::Element_implicitly_has_an_any_type_because_index_expression_is_not_of_type_number,
                        &[],
                    );
                } else {
                    let suggestion = match property_name {
                        Some(name) => self.get_suggestion_for_nonexistent_property(
                            /*name_node*/ None,
                            name,
                            object_type,
                        )?,
                        None => None,
                    };
                    if let Some(suggestion) = suggestion {
                        // NO related 2728 on the element-access flavor
                        // (oracle-pinned asymmetry).
                        let name = property_name.expect("suggestion implies name");
                        let object_display = self.type_to_string_slice(object_type)?;
                        self.error_at(
                            argument.or(Some(access_expression)),
                            &diagnostics::Property_0_does_not_exist_on_type_1_Did_you_mean_2,
                            &[name, &object_display, &suggestion],
                        );
                    } else {
                        let index_suggestion = self
                            .get_suggestion_for_nonexistent_index_signature(
                                object_type,
                                access_expression,
                                index_type,
                            )?;
                        if let Some(index_suggestion) = index_suggestion {
                            let object_display = self.type_to_string_slice(object_type)?;
                            self.error_at(
                                Some(access_expression),
                                &diagnostics::Element_implicitly_has_an_any_type_because_type_0_has_no_index_signature_Did_you_mean_to_call_1,
                                &[&object_display, &index_suggestion],
                            );
                        } else {
                            let mut tail: Vec<tsrs2_diags::MessageChain> = Vec::new();
                            if index_flags.intersects(TypeFlags::ENUM_LITERAL) {
                                let index_display = self.type_to_string_slice(index_type)?;
                                let object_display = self.type_to_string_slice(object_type)?;
                                tail.push(tsrs2_diags::MessageChain::new(
                                    &diagnostics::Property_0_does_not_exist_on_type_1,
                                    &[format!("[{index_display}]"), object_display],
                                ));
                            } else if index_flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
                                // 62319-62321: `[<fully-qualified>]`
                                // over the unique symbol's symbol (the
                                // containingLocation-relative
                                // qualification is a T2 nuance — the
                                // full chain renders).
                                let symbol = self
                                    .tables
                                    .type_of(index_type)
                                    .symbol
                                    .expect("unique symbol types carry their symbol");
                                let symbol_name = self.get_fully_qualified_name(symbol);
                                let object_display = self.type_to_string_slice(object_type)?;
                                tail.push(tsrs2_diags::MessageChain::new(
                                    &diagnostics::Property_0_does_not_exist_on_type_1,
                                    &[format!("[{symbol_name}]"), object_display],
                                ));
                            } else if index_flags
                                .intersects(TypeFlags::STRING_LITERAL | TypeFlags::NUMBER_LITERAL)
                            {
                                let value = self
                                    .index_literal_value_display(index_type)
                                    .unwrap_or_default();
                                let object_display = self.type_to_string_slice(object_type)?;
                                tail.push(tsrs2_diags::MessageChain::new(
                                    &diagnostics::Property_0_does_not_exist_on_type_1,
                                    &[value, object_display],
                                ));
                            } else if index_flags.intersects(TypeFlags::NUMBER | TypeFlags::STRING)
                            {
                                let index_display = self.type_to_string_slice(index_type)?;
                                let object_display = self.type_to_string_slice(object_type)?;
                                tail.push(tsrs2_diags::MessageChain::new(
                                    &diagnostics::No_index_signature_with_a_parameter_of_type_0_was_found_on_type_1,
                                    &[index_display, object_display],
                                ));
                            }
                            let full_display = self.type_to_string_slice(full_index_type)?;
                            let object_display = self.type_to_string_slice(object_type)?;
                            let head = tsrs2_diags::MessageChain::new(
                                &diagnostics::Element_implicitly_has_an_any_type_because_expression_of_type_0_can_t_be_used_to_index_type_1,
                                &[full_display, object_display],
                            );
                            let mut diagnostic = self.diagnostic_for_node(
                                access_expression,
                                &diagnostics::Element_implicitly_has_an_any_type_because_expression_of_type_0_can_t_be_used_to_index_type_1,
                                &[],
                            );
                            diagnostic.message = head.with_next(tail);
                            self.push_error_diagnostic(diagnostic);
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: getIndexNodeForAccessExpression @6.0.3
    /// tsc-hash: ce391770881fc0c1e83445fdbd202d82c932407368884e652b2215e1fd9f6bc3
    /// tsc-span: _tsc.js:62408-62410
    fn get_index_node_for_access_expression(&self, access_node: NodeId) -> NodeId {
        match self.data_of(access_node) {
            NodeData::ElementAccessExpression(data) => {
                data.argument_expression.unwrap_or(access_node)
            }
            NodeData::IndexedAccessType(data) => data.index_type.unwrap_or(access_node),
            NodeData::ComputedPropertyName(data) => data.expression.unwrap_or(access_node),
            _ => access_node,
        }
    }

    /// errorIfWritingToReadonlyIndex (62399-62403).
    fn error_if_writing_to_readonly_index(
        &mut self,
        index_info: Option<&crate::state::IndexInfo>,
        object_type: TypeId,
        access_expression: Option<NodeId>,
    ) -> CheckResult2<()> {
        let Some(index_info) = index_info else {
            return Ok(());
        };
        let Some(access_expression) = access_expression else {
            return Ok(());
        };
        if !index_info.is_readonly {
            return Ok(());
        }
        let source = self.binder.source_of_node(access_expression);
        if tsrs2_binder::node_util::is_assignment_target(source, access_expression)
            || self.is_delete_target(access_expression)
        {
            let object_display = self.type_to_string_slice(object_type)?;
            self.error_at(
                Some(access_expression),
                &diagnostics::Index_signature_in_type_0_only_permits_reading,
                &[&object_display],
            );
        }
        Ok(())
    }

    /// tsc-port: isJSLiteralType @6.0.3
    /// tsc-hash: 1f0a81d7d87b210f2f134fdc42636ffba58be1f415ce4a08574c4b43a3ff0f29
    /// tsc-span: _tsc.js:62176-62194
    ///
    /// JSLiteral object flags are never produced (JS literal widening
    /// is 5.6), so only the flag-shape is live.
    pub(crate) fn is_js_literal_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self
            .options
            .strict_option_value(self.options.no_implicit_any)
        {
            return Ok(false);
        }
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::JS_LITERAL)
        {
            return Ok(true);
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::UNION) {
            let members = self.union_members_or_self(ty);
            for member in &members {
                if !self.is_js_literal_type(*member)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies intersection data"),
            };
            for member in members {
                if self.is_js_literal_type(member)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if flags.intersects(TypeFlags::INSTANTIABLE) {
            let constraint = self.get_resolved_base_constraint(ty)?;
            if constraint != ty {
                return self.is_js_literal_type(constraint);
            }
        }
        Ok(false)
    }

    /// tsc-port: isStringIndexSignatureOnlyType @6.0.3
    /// tsc-hash: 6914dc03c67198d748e1f8fc4d56b1e9a19c7b5abf51787203b93f70fa428411
    /// tsc-span: _tsc.js:64670-64672
    fn is_string_index_signature_only_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::OBJECT) {
            let properties = self.get_properties_of_type_full(ty)?;
            if !properties.is_empty() {
                return Ok(false);
            }
            let infos = self.get_index_infos_of_type(ty)?;
            return Ok(infos.len() == 1 && infos[0].key_type == self.tables.intrinsics.string);
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("union/intersection flag implies member data"),
            };
            for member in members {
                if !self.is_string_index_signature_only_type(member)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// isTypeUsableAsPropertyName (19351) + getPropertyNameFromType
    /// (19354): string/number literals and unique ES symbols.
    pub(crate) fn property_name_from_type_usable(&self, ty: TypeId) -> Option<String> {
        if !self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE)
        {
            return None;
        }
        self.property_name_from_type(ty)
    }

    fn property_name_from_type(&self, ty: TypeId) -> Option<String> {
        match &self.tables.type_of(ty).data {
            // escapeLeadingUnderscores("" + type.value): the member
            // tables are escaped-keyed, so the propName leaves this
            // boundary escaped. Display rows that render the raw
            // literal value (tsc passes `indexType.value` there) use
            // index_literal_value_display instead.
            TypeData::Literal {
                value: tsrs2_types::LiteralValue::String(value),
            } => Some(tsrs2_syntax::escape_leading_underscores(value)),
            TypeData::Literal {
                value: tsrs2_types::LiteralValue::Number(value),
            } => Some(tsrs2_types::tables::js_number_to_string(*value)),
            // getPropertyNameFromType's UniqueESSymbol arm: the
            // late-bound `__@<name>@<id>` member name.
            TypeData::UniqueESSymbol { escaped_name } => Some(escaped_name.clone()),
            _ => None,
        }
    }

    /// The `"" + indexType.value` rendering the 2339/7053 display rows
    /// pass (62295/62343/62388) — the raw literal value, NOT the
    /// escaped propName.
    fn index_literal_value_display(&self, ty: TypeId) -> Option<String> {
        match &self.tables.type_of(ty).data {
            TypeData::Literal {
                value: tsrs2_types::LiteralValue::String(value),
            } => Some(value.clone()),
            TypeData::Literal {
                value: tsrs2_types::LiteralValue::Number(value),
            } => Some(tsrs2_types::tables::js_number_to_string(*value)),
            _ => None,
        }
    }

    // ---- tuple element access ----

    /// getStartElementCount/getEndElementCount/getTotalFixedElementCount
    /// (61300-61311).
    pub(crate) fn get_total_fixed_element_count(&self, target: TypeId) -> usize {
        match &self.tables.type_of(target).data {
            TypeData::TupleTarget(data) => {
                let end = data
                    .element_flags
                    .iter()
                    .rev()
                    .position(|f| !f.intersects(tsrs2_types::ElementFlags::FIXED))
                    .unwrap_or(data.element_flags.len());
                data.fixed_length + end.min(data.element_flags.len() - data.fixed_length)
            }
            _ => 0,
        }
    }

    /// tsc-port: getRestTypeOfTupleType @6.0.3
    /// tsc-hash: e1a5ac9e819103ea900bad4707f9547b51786eed8ac31d5baac42ef703c0974c
    /// tsc-span: _tsc.js:67800-67802
    pub(crate) fn get_rest_type_of_tuple_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let target = self.tables.reference_target(ty);
        let fixed_length = match &self.tables.type_of(target).data {
            TypeData::TupleTarget(data) => data.fixed_length,
            _ => return Ok(None),
        };
        self.get_element_type_of_slice_of_tuple_type(ty, fixed_length, 0, false, false)
    }

    /// tsc-port: getKnownKeysOfTupleType @6.0.3
    /// tsc-hash: feea7a608c0d34daf2e00508df8313ddbd6145e7cd375785fe15482fca9dc2c2
    /// tsc-span: _tsc.js:61299-61301
    pub(crate) fn get_known_keys_of_tuple_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let target = self.tables.reference_target(ty);
        let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
            unreachable!("tuple type targets a tuple target");
        };
        let array = if data.readonly {
            self.global_readonly_array_type()?
        } else {
            self.global_array_type()?
        };
        let mut members: Vec<TypeId> = (0..data.fixed_length)
            .map(|i| self.tables.get_string_literal_type(&i.to_string()))
            .collect();
        members.push(self.get_index_type(array, IndexFlags::NONE)?);
        self.get_union_type_ex(&members, UnionReduction::Literal)
    }

    /// tsc-port: getElementTypeOfSliceOfTupleType @6.0.3
    /// tsc-hash: 8fd16ceeda3e0823b3c56a9f0ab765a4d479ac0bd5b588a26d0acd8416dd0fd3
    /// tsc-span: _tsc.js:67820-67833
    pub(crate) fn get_element_type_of_slice_of_tuple_type(
        &mut self,
        ty: TypeId,
        index: usize,
        end_skip_count: usize,
        writing: bool,
        no_reductions: bool,
    ) -> CheckResult2<Option<TypeId>> {
        // getTypeArguments (67826) — deferred tuple references force
        // their arguments lazily here.
        let type_arguments: Vec<TypeId> = self.get_type_arguments(ty)?;
        let target = self.tables.reference_target(ty);
        let element_flags: Vec<tsrs2_types::ElementFlags> = match &self.tables.type_of(target).data
        {
            TypeData::TupleTarget(data) => data.element_flags.to_vec(),
            _ => return Ok(None),
        };
        let length = element_flags.len().saturating_sub(end_skip_count);
        if index >= length {
            return Ok(None);
        }
        let mut element_types: Vec<TypeId> = Vec::with_capacity(length - index);
        for i in index..length {
            let t = type_arguments[i];
            element_types.push(
                if element_flags[i].intersects(tsrs2_types::ElementFlags::VARIADIC) {
                    let number = self.tables.intrinsics.number;
                    self.get_indexed_access_type(t, number, AccessFlags::NONE, None, None, None)?
                } else {
                    t
                },
            );
        }
        Ok(Some(if writing {
            self.get_intersection_type(&element_types, IntersectionFlags::NONE)?
        } else {
            self.get_union_type_ex(
                &element_types,
                if no_reductions {
                    UnionReduction::None
                } else {
                    UnionReduction::Literal
                },
            )?
        }))
    }

    /// tsc-port: getTupleElementTypeOutOfStartCount @6.0.3
    /// tsc-hash: 91f5d00b1e1452d2dffe65957f2d4e845beed1bc05c26a83442e5622c16e51a7
    /// tsc-span: _tsc.js:67803-67816
    pub(crate) fn get_tuple_element_type_out_of_start_count(
        &mut self,
        ty: TypeId,
        index: usize,
        undefined_or_missing: Option<TypeId>,
    ) -> CheckResult2<TypeId> {
        let mapped = self.map_type(
            ty,
            &mut |state, t| {
                let rest_type = state.get_rest_type_of_tuple_type(t)?;
                let Some(rest_type) = rest_type else {
                    return Ok(Some(state.tables.intrinsics.undefined));
                };
                if let Some(undefined_or_missing) = undefined_or_missing {
                    let target = state.tables.reference_target(t);
                    if index >= state.get_total_fixed_element_count(target) {
                        return Ok(Some(state.get_union_type_ex(
                            &[rest_type, undefined_or_missing],
                            UnionReduction::Literal,
                        )?));
                    }
                }
                Ok(Some(rest_type))
            },
            /*no_reductions*/ false,
        )?;
        Ok(mapped.expect("tuple element mapping never drops members"))
    }
}

/// tsc-port: isNumericLiteralName @6.0.3
/// tsc-hash: 792c3a97db611b31a75c5d2ee921c6788c6a6ccbfbd6a3240b943e3f98205802
/// tsc-span: _tsc.js:19205-19207
pub(crate) fn is_numeric_literal_name(name: &str) -> bool {
    match name.parse::<f64>() {
        Ok(value) => tsrs2_types::tables::js_number_to_string(value) == name,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{AccessFlags, CompilerOptions, IndexFlags, SymbolFlags, TypeData, TypeFlags};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_type(state: &mut CheckerState, name: &str) -> tsrs2_types::TypeId {
        let annotation =
            find_probe_annotation(state.binder.source(0), name).expect("var with annotation");
        state
            .get_type_from_type_node(annotation)
            .expect("annotation resolves")
    }

    fn type_parameter(state: &mut CheckerState, name: &str) -> tsrs2_types::TypeId {
        let source = state.binder.source(0);
        let inside = source
            .arena
            .node_ids()
            .find(|&id| source.arena.node(id).kind == tsrs2_syntax::SyntaxKind::VariableDeclaration)
            .expect("var declaration");
        let symbol = state
            .resolve_name(
                Some(inside),
                name,
                SymbolFlags::TYPE_PARAMETER,
                None,
                false,
                false,
            )
            .expect("resolve_name")
            .expect("type parameter resolves");
        state.get_declared_type_of_type_parameter(symbol)
    }

    #[test]
    fn keyof_type_literal_yields_the_literal_union() {
        with_program_state(
            &[(
                "a.ts",
                "declare var v: keyof { a: string; \"1\": number };\ndeclare var w: \"a\" | \"1\";\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let keyof = annotation_type(state, "v");
                let expected = annotation_type(state, "w");
                // Oracle-pinned: string-named `"1"` keys are STRING
                // literals in keyof.
                assert_eq!(keyof, expected);
            },
        );
    }

    #[test]
    fn keyof_with_string_index_signature_widens_to_string_or_number() {
        with_program_state(
            &[(
                "a.ts",
                "declare var v: keyof { [k: string]: any };\ndeclare var w: string | number;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let keyof = annotation_type(state, "v");
                let expected = annotation_type(state, "w");
                assert_eq!(keyof, expected);
            },
        );
    }

    #[test]
    fn keyof_interface_carries_an_index_origin() {
        with_program_state(
            &[(
                "a.ts",
                "interface I { a: string; b: number }\ndeclare var v: keyof I;\ndeclare var w: \"a\" | \"b\";\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let keyof = annotation_type(state, "v");
                let plain = annotation_type(state, "w");
                // The interface keyof denormalizes through an origin
                // index type: a DISTINCT union interned under the `#`
                // key, structurally equal to the plain literal union.
                assert_ne!(keyof, plain);
                let TypeData::Union { origin, .. } = state.tables.type_of(keyof).data.clone()
                else {
                    panic!("keyof I is a union");
                };
                let origin = origin.expect("keyof I carries an origin");
                assert!(matches!(
                    state.tables.type_of(origin).data,
                    TypeData::Index { .. }
                ));
                assert_eq!(state.is_type_assignable_to(keyof, plain), Ok(true));
                assert_eq!(state.is_type_assignable_to(plain, keyof), Ok(true));
            },
        );
    }

    #[test]
    fn keyof_generic_defers_and_instantiates() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T>() { var v: keyof T; var w: { a: string }; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let keyof = annotation_type(state, "v");
                assert!(state.tables.flags_of(keyof).intersects(TypeFlags::INDEX));
                let t = type_parameter(state, "T");
                assert!(matches!(
                    state.tables.type_of(keyof).data,
                    TypeData::Index { ty, .. } if ty == t
                ));
                // The per-operand cache interns the deferred index type.
                let again = state
                    .get_index_type(t, IndexFlags::NONE)
                    .expect("index type");
                assert_eq!(again, keyof);
                // Instantiation maps through to the literal key.
                let literal_object = annotation_type(state, "w");
                let mapper = state.create_type_mapper(vec![t], Some(vec![literal_object]));
                let instantiated = state
                    .instantiate_type(keyof, Some(mapper))
                    .expect("instantiation");
                assert_eq!(instantiated, state.tables.get_string_literal_type("a"));
            },
        );
    }

    #[test]
    fn indexed_access_reads_properties_and_unions() {
        with_program_state(
            &[(
                "a.ts",
                "declare var v: { a: string; b: number }[\"a\"];\n\
                 declare var y: { a: string; b: number }[\"a\" | \"b\"];\n\
                 declare var z: string | number;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                assert_eq!(annotation_type(state, "v"), state.tables.intrinsics.string);
                let union_access = annotation_type(state, "y");
                let expected = annotation_type(state, "z");
                assert_eq!(union_access, expected);
            },
        );
    }

    #[test]
    fn tuple_indexed_access_reads_the_synthesized_members() {
        with_program_state(
            &[(
                "a.ts",
                // Array<T> feeds getTupleBaseType (the tuple target's
                // base) during member resolution.
                "interface Array<T> { length: number }\n\
                 declare var v: [string, number][1];\ndeclare var w: [string, number][\"length\"];\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // 5.3c: the property lookup consults the tuple
                // reference's synthesized per-index/length members.
                assert_eq!(annotation_type(state, "v"), state.tables.intrinsics.number);
                let length = annotation_type(state, "w");
                // Fixed [string, number]: length is the literal 2.
                assert!(state
                    .tables
                    .flags_of(length)
                    .intersects(TypeFlags::NUMBER_LITERAL));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }

    #[test]
    fn generic_indexed_access_defers_instantiates_and_constrains() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T extends { a: string }>() { var v: T[\"a\"]; var w: { a: \"x\" }; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let access = annotation_type(state, "v");
                assert!(state
                    .tables
                    .flags_of(access)
                    .intersects(TypeFlags::INDEXED_ACCESS));
                let t = type_parameter(state, "T");
                assert!(matches!(
                    state.tables.type_of(access).data,
                    TypeData::IndexedAccess { object_type, .. } if object_type == t
                ));
                // Interned per (object, index, flags).
                let index = state.tables.get_string_literal_type("a");
                let again = state
                    .get_indexed_access_type(t, index, AccessFlags::NONE, None, None, None)
                    .expect("indexed access");
                assert_eq!(again, access);
                // The base constraint re-accesses through the bounds.
                let constraint = state
                    .get_base_constraint_of_type(access)
                    .expect("constraint in slice");
                assert_eq!(constraint, Some(state.tables.intrinsics.string));
                // Instantiation maps through the concrete object.
                let concrete = annotation_type(state, "w");
                let mapper = state.create_type_mapper(vec![t], Some(vec![concrete]));
                let instantiated = state
                    .instantiate_type(access, Some(mapper))
                    .expect("instantiation");
                assert_eq!(instantiated, state.tables.get_string_literal_type("x"));
            },
        );
    }

    #[test]
    fn keyof_distributes_over_unions_and_intersections() {
        with_program_state(
            &[(
                "a.ts",
                "declare var v: keyof ({ a: string; b: number } | { b: string; c: number });\n\
                 declare var w: keyof ({ a: string } & { b: number });\n\
                 declare var u: \"a\" | \"b\";\n",
            )],
            &CompilerOptions::default(),
            |state| {
                // Union operand -> intersection of key sets = "b".
                let of_union = annotation_type(state, "v");
                assert_eq!(of_union, state.tables.get_string_literal_type("b"));
                // Intersection operand -> union of key sets.
                let of_intersection = annotation_type(state, "w");
                let expected = annotation_type(state, "u");
                assert_eq!(of_intersection, expected);
            },
        );
    }

    // ---- m4-review S1/S3 pins (oracle: vendored tsc 6.0.3, noLib,
    // strict defaults, 2026-07-19) ----

    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            state
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
                .collect()
        })
    }

    #[test]
    fn double_underscore_element_access_resolves() {
        // S1: tsc clean. Pre-fix the raw `__x` propName missed the
        // escaped-keyed member table → 7053.
        assert_eq!(
            checked_rows("interface O { __x: number }\ndeclare const o: O;\no[\"__x\"];\n"),
            []
        );
    }

    #[test]
    fn double_underscore_indexed_access_type_resolves() {
        // S1: tsc clean — V is number (pre-fix: 2339 + errorType, so
        // the assignment below would misreport).
        assert_eq!(
            checked_rows(
                "interface O { __x: number }\ntype V = O[\"__x\"];\ndeclare const v: V;\nconst n: number = v;\n"
            ),
            []
        );
    }

    #[test]
    fn double_underscore_suggestion_args_stay_escaped() {
        // S1: tsc 2551 @54 len8 with arg0 = '___helo' — the ESCAPED
        // propName VERBATIM (tsc passes the __String straight through)
        // — and the suggestion '__hello' unescaped.
        let text = "interface P { __hello: number }\ndeclare const p: P;\np[\"__helo\"];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let rows: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|diag| diag.file_name.is_some())
                .collect();
            assert_eq!(rows.len(), 1, "{rows:#?}");
            let diagnostic = rows[0];
            assert_eq!(
                (diagnostic.code(), diagnostic.start, diagnostic.length),
                (2551, Some(54), Some(8))
            );
            assert!(
                diagnostic.message.text.contains("'___helo'")
                    && diagnostic.message.text.contains("'__hello'"),
                "{}",
                diagnostic.message.text
            );
        });
    }

    #[test]
    fn keyof_typeof_enum_excludes_the_reverse_map_number() {
        // S3: tsc clean — enumNumberIndexInfo is excluded from
        // getLiteralTypeFromProperties, so K = "A" | "B" and the
        // string assignment holds.
        assert_eq!(
            checked_rows(
                "enum E { A, B }\ntype K = keyof typeof E;\ndeclare const k: K;\nconst s: string = k;\n"
            ),
            []
        );
    }

    #[test]
    fn keyof_typeof_enum_rejects_number() {
        // S3 reverse direction: tsc 2322 @72 len2 (Type 'number' is
        // not assignable to type '"A" | "B"') — pre-fix the leaked
        // number index made this assignment pass.
        assert_eq!(
            checked_rows(
                "enum E { A, B }\ntype K = keyof typeof E;\ndeclare const n: number;\nconst k2: K = n;\n"
            ),
            [(2322, 72, 2)]
        );
    }
}

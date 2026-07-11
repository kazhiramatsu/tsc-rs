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

impl<'a> CheckerState<'a> {
    /// tsc-port: getIndexType @6.0.3
    /// tsc-hash: af5867f7723c495d086366e08501a86c4c76b02f70bf5b94dae8f524f2ef51ac
    /// tsc-span: _tsc.js:62016-62019
    ///
    /// isNoInferType is constant false (Substitution types are
    /// unconstructible before M8); the Mapped arm is asserted
    /// unreachable the same way.
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
        assert!(
            !self
                .tables
                .object_flags_of(ty)
                .intersects(ObjectFlags::MAPPED),
            "mapped types are unconstructible before M8 (getIndexTypeForMappedType)"
        );
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
    /// The generic-mapped-with-nameType disjunct is constant false
    /// (mapped types unconstructible before M8).
    fn should_defer_index_type(
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
            if members
                .iter()
                .any(|&member| self.is_empty_anonymous_object_type(member))
            {
                return Ok(true);
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
        let unique_filled = match self.links.ty(ty).unique_literal_filled_instantiation.resolved()
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
    fn get_index_type_for_generic_type(&mut self, ty: TypeId, index_flags: IndexFlags) -> TypeId {
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
    /// enumNumberIndexInfo is a 5.3b enum sentinel — no index info can
    /// be it before enums construct.
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
                property,
                include,
                /*include_non_public*/ false,
            )?);
        }
        for info in self.get_index_infos_of_type(ty)? {
            let included = self.is_key_type_included(info.key_type, include);
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
    fn get_literal_type_from_property_name(&mut self, name: NodeId) -> CheckResult2<TypeId> {
        match self.data_of(name) {
            NodeData::PrivateIdentifier(_) => Ok(self.tables.intrinsics.never),
            NodeData::NumericLiteral(data) => {
                let value = crate::annotate::parse_numeric_literal_text(&data.text)?;
                Ok(self.tables.get_number_literal_type(value))
            }
            NodeData::ComputedPropertyName(_) => Err(Unsupported::new(
                "computed property names in keyof (checkComputedPropertyName, M4 5.5)",
            )),
            NodeData::Identifier(data) => Ok(self
                .tables
                .get_string_literal_type(tsrs2_binder::unescape_leading_underscores(
                    &data.escaped_text,
                ))),
            NodeData::StringLiteral(data) => {
                let text = data.text.clone();
                Ok(self.tables.get_string_literal_type(&text))
            }
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                let text = data.text.clone();
                Ok(self.tables.get_string_literal_type(&text))
            }
            _ => Err(Unsupported::new(
                "expression property names in keyof (checkExpression, M4 5.5)",
            )),
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
    /// Slice notes: noUncheckedIndexedAccess is an unmodeled option and
    /// ExpressionPosition access is 5.5, so the IncludeUndefined
    /// promotion line is dead; `access_node` here is always a type-
    /// position IndexedAccessType node (element access expressions are
    /// 5.5), so the tuple gate always takes the IndexedAccessType
    /// branch.
    pub fn get_indexed_access_type_or_undefined(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        access_flags: AccessFlags,
        access_node: Option<NodeId>,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<Option<TypeId>> {
        if object_type == self.tables.intrinsics.wildcard
            || index_type == self.tables.intrinsics.wildcard
        {
            return Ok(Some(self.tables.intrinsics.wildcard));
        }
        let object_type = self.get_reduced_type(object_type)?;
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
        let generic = self.tables.is_generic_index_type(index_type) || {
            let object_generic = self.tables.is_generic_object_type(object_type);
            let tuple_fixed_access = self.tables.is_tuple_type(object_type) && {
                let target = self.tables.reference_target(object_type);
                let limit = self.get_total_fixed_element_count(target);
                self.index_type_less_than(index_type, limit)
            };
            (object_generic && !tuple_fixed_access)
                || self.is_generic_reducible_type(object_type)?
        };
        if generic {
            if self
                .tables
                .flags_of(object_type)
                .intersects(TypeFlags::ANY_OR_UNKNOWN)
            {
                return Ok(Some(object_type));
            }
            let persistent = AccessFlags::from_bits(
                access_flags.bits() & AccessFlags::PERSISTENT.bits(),
            );
            return Ok(Some(self.tables.get_indexed_access_type_interned(
                object_type,
                index_type,
                persistent,
                alias_symbol,
                alias_type_arguments,
            )));
        }
        let apparent_object_type = {
            let apparent = self.get_apparent_type_m3(object_type)?;
            self.get_reduced_type(apparent)?
        };
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
                let property_type = self.get_property_type_for_index_type(
                    object_type,
                    apparent_object_type,
                    member,
                    index_type,
                    access_node,
                    member_flags,
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
        self.get_property_type_for_index_type(
            object_type,
            apparent_object_type,
            index_type,
            index_type,
            access_node,
            full_flags,
        )
    }

    /// tsc-port: getPropertyTypeForIndexType @6.0.3
    /// tsc-hash: 4014bff2f311f9e1f8746a10a48831bc25867f25d8548e320ea8ead8c5afa786
    /// tsc-span: _tsc.js:62211-62410
    ///
    /// TYPE-POSITION slice: accessExpression is always None (element
    /// access is 5.5), so the flow/write/deprecation/referenced-marking
    /// arms and every accessExpression diagnostic are structurally
    /// skipped. Error paths that render types through typeToString's
    /// nodeBuilder (tuple index errors 2493/2339/2537, 2538
    /// Type_0_cannot_be_used_as_an_index_type) unwind as Unsupported —
    /// display is T2/M8. The Contextual/AllowMissing arms belong to
    /// contextual typing (M6).
    fn get_property_type_for_index_type(
        &mut self,
        _original_object_type: TypeId,
        object_type: TypeId,
        index_type: TypeId,
        _full_index_type: TypeId,
        access_node: Option<NodeId>,
        access_flags: AccessFlags,
    ) -> CheckResult2<Option<TypeId>> {
        if access_flags.intersects(AccessFlags::WRITING) {
            return Err(Unsupported::new(
                "write-position indexed access (getWriteTypeOfSymbol, M4 5.5)",
            ));
        }
        let property_name = self.property_name_from_type_usable(index_type);
        if let Some(property_name) = &property_name {
            let property = self.get_property_of_type_full(object_type, property_name)?;
            if let Some(property) = property {
                // isDeprecatedSymbol suggestions are M7 diagnostics.
                let property_type = self.get_type_of_symbol(property)?;
                let missing = access_node.is_some_and(|node| {
                    self.kind_of(node) == SyntaxKind::IndexedAccessType
                }) && self.tables.contains_missing_type(property_type);
                return Ok(Some(if missing {
                    let undefined = self.tables.intrinsics.undefined;
                    self.get_union_type_ex(&[property_type, undefined], UnionReduction::Literal)?
                } else {
                    property_type
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
                if access_node.is_some()
                    && all_fixed
                    && !access_flags.intersects(AccessFlags::ALLOW_MISSING)
                {
                    // 62243-62256: the out-of-bounds tuple index errors
                    // (2493/negative/2339) render the tuple through
                    // typeToString — display is T2/M8.
                    return Err(Unsupported::new(
                        "tuple index diagnostics (typeToString display, T2/M8)",
                    ));
                }
                if index >= 0.0 {
                    // errorIfWritingToReadonlyIndex: Writing escapes
                    // above.
                    return Ok(Some(self.get_tuple_element_type_out_of_start_count(
                        object_type,
                        index as usize,
                        access_flags
                            .intersects(AccessFlags::INCLUDE_UNDEFINED)
                            .then_some(self.tables.intrinsics.missing),
                    )?));
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
                    // getIndexInfoOfType(objectType, stringType): the
                    // exact string-keyed info.
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
                    // The accessExpression diagnostics (2862/2536) are
                    // 5.5; without one tsc returns undefined.
                    return Ok(None);
                }
                if access_node.is_some() && index_info.key_type == self.tables.intrinsics.string {
                    let string_or_number = TypeFlags::from_bits(
                        TypeFlags::STRING.bits() | TypeFlags::NUMBER.bits(),
                    );
                    if !self.is_type_assignable_to_kind(
                        index_type,
                        string_or_number,
                        /*strict*/ false,
                    )? {
                        // 62278-62281: 2538 renders the index type —
                        // display is T2/M8.
                        return Err(Unsupported::new(
                            "index-type diagnostics (typeToString display, T2/M8)",
                        ));
                    }
                }
                // errorIfWritingToReadonlyIndex: Writing escapes above.
                // The IncludeUndefined enum-literal exclusion
                // (62284-62287) is dead: the flag is never set in type
                // position and enums are unconstructible.
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
            // The accessExpression object-literal/noImplicitAny
            // diagnostic ladder (62296-62379) is 5.5.
            if access_node.is_some() {
                // 62386-62396: the misses render both types — display
                // is T2/M8.
                return Err(Unsupported::new(
                    "indexed-access miss diagnostics (typeToString display, T2/M8)",
                ));
            }
        }
        // AllowMissing object-literal arm is M6 contextual typing.
        if self.is_js_literal_type(object_type)? {
            return Ok(Some(self.tables.intrinsics.any));
        }
        if let Some(_node) = access_node {
            // 62387-62397 (the final accessNode error ladder) renders
            // types — display is T2/M8.
            return Err(Unsupported::new(
                "indexed-access miss diagnostics (typeToString display, T2/M8)",
            ));
        }
        if self.tables.flags_of(index_type).intersects(TypeFlags::ANY) {
            return Ok(Some(index_type));
        }
        Ok(None)
    }

    /// tsc-port: isJSLiteralType @6.0.3
    /// tsc-hash: 1f0a81d7d87b210f2f134fdc42636ffba58be1f415ce4a08574c4b43a3ff0f29
    /// tsc-span: _tsc.js:62176-62194
    ///
    /// JSLiteral object flags are never produced (JS literal widening
    /// is 5.6), so only the flag-shape is live.
    fn is_js_literal_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
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
    /// (19354): string/number literals; unique ES symbols are
    /// unconstructible before 5.3b.
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
    pub(crate) fn get_rest_type_of_tuple_type(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        let target = self.tables.reference_target(ty);
        let fixed_length = match &self.tables.type_of(target).data {
            TypeData::TupleTarget(data) => data.fixed_length,
            _ => return Ok(None),
        };
        self.get_element_type_of_slice_of_tuple_type(ty, fixed_length, 0, false, false)
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
        let element_flags: Vec<tsrs2_types::ElementFlags> =
            match &self.tables.type_of(target).data {
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
        let annotation = find_probe_annotation(state.binder.source(0), name)
            .expect("var with annotation");
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
    fn tuple_indexed_access_escapes_to_tuple_member_synthesis() {
        with_program_state(
            &[("a.ts", "declare var v: [string, number][1];\n")],
            &CompilerOptions::default(),
            |state| {
                // The property lookup consults the tuple reference's
                // synthesized members — 5.3 tuple member synthesis;
                // getTupleElementTypeOutOfStartCount unblocks with it.
                let annotation = find_probe_annotation(state.binder.source(0), "v")
                    .expect("var with annotation");
                let reason = state
                    .get_type_from_type_node(annotation)
                    .expect_err("tuple member reads are a 5.3 row")
                    .reason;
                assert!(reason.contains("M4 5.3"), "{reason}");
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
}

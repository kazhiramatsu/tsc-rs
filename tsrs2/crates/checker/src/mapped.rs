//! Mapped-type member materialization (phase 9.5b).
//!
//! The immutable mapped payload lives in `tsrs2-types`; this module
//! owns the checker-side lazy modifier source, finite key expansion,
//! synthesized mapped properties/index infos, and property-type
//! instantiation.

use tsrs2_binder::{SymbolId, SymbolTable};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckFlags, IndexFlags, MappedTypeModifiers, ObjectFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, TypeSystemPropertyName, UnionReduction,
};

use crate::links::LinkSlot;
use crate::state::{
    CheckResult2, CheckerState, IndexInfo, MembersId, ResolutionTarget, ResolvedMembers,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MappedTypeNameTypeKind {
    None,
    Filtering,
    Remapping,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: isGenericMappedType @6.0.3
    /// tsc-hash: 0de4059bb2606e0ea8b724e86d10d096d4e9fba0de0e0ee4b01cdd51d92b09a1
    /// tsc-span: _tsc.js:58659-58671
    pub(crate) fn is_generic_mapped_type_state(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
        {
            return Ok(false);
        }
        let constraint = self.get_constraint_type_from_mapped_type(ty)?;
        if self
            .get_generic_object_flags(constraint)?
            .intersects(ObjectFlags::IS_GENERIC_INDEX_TYPE)
        {
            return Ok(true);
        }
        let Some(name_type) = self.get_name_type_from_mapped_type(ty)? else {
            return Ok(false);
        };
        let type_parameter = self.get_type_parameter_from_mapped_type(ty)?;
        let mapper = self.make_unary_type_mapper(type_parameter, constraint);
        let instantiated_name = self.instantiate_type(name_type, Some(mapper))?;
        Ok(self
            .get_generic_object_flags(instantiated_name)?
            .intersects(ObjectFlags::IS_GENERIC_INDEX_TYPE))
    }

    /// tsc-port: getMappedTypeOptionality @6.0.3
    /// tsc-hash: f2ff51c93f2b27afb3a02a30de4d6fda4499f79aca0382e2d6abb0a8d4824987
    /// tsc-span: _tsc.js:58642-58645
    pub(crate) fn get_mapped_type_optionality(&self, ty: TypeId) -> i8 {
        let modifiers = self.get_mapped_type_modifiers(ty);
        if modifiers.intersects(MappedTypeModifiers::EXCLUDE_OPTIONAL) {
            -1
        } else if modifiers.intersects(MappedTypeModifiers::INCLUDE_OPTIONAL) {
            1
        } else {
            0
        }
    }

    /// tsc-port: getCombinedMappedTypeOptionality @6.0.3
    /// tsc-hash: 1ecb4167362e790cb5a669db93477c0f13ebaa79454f26830f5d7f2157e71cbf
    /// tsc-span: _tsc.js:58646-58655
    pub(crate) fn get_combined_mapped_type_optionality(&mut self, ty: TypeId) -> CheckResult2<i8> {
        if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
        {
            let direct = self.get_mapped_type_optionality(ty);
            return if direct != 0 {
                Ok(direct)
            } else {
                let modifiers = self.get_modifiers_type_from_mapped_type(ty)?;
                self.get_combined_mapped_type_optionality(modifiers)
            };
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = &self.tables.type_of(ty).data else {
                unreachable!("intersection flag implies intersection payload");
            };
            let types = types.to_vec();
            let Some((&first, rest)) = types.split_first() else {
                return Ok(0);
            };
            let optionality = self.get_combined_mapped_type_optionality(first)?;
            for &member in rest {
                if self.get_combined_mapped_type_optionality(member)? != optionality {
                    return Ok(0);
                }
            }
            return Ok(optionality);
        }
        Ok(0)
    }

    /// tsc-port: isPartialMappedType @6.0.3
    /// tsc-hash: 85f48737a839d61c088c792e89dedb86340523a69587fde73d6c615a1697744e
    /// tsc-span: _tsc.js:58656-58658
    pub(crate) fn is_partial_mapped_type(&self, ty: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::MAPPED)
            && self
                .get_mapped_type_modifiers(ty)
                .intersects(MappedTypeModifiers::INCLUDE_OPTIONAL)
    }

    /// tsc-port: getConstraintDeclarationForMappedType @6.0.3
    /// tsc-hash: 2f1f4f5927df5e92dde6840178b5e9aea7a781a8d4c3fed38ea75823e8e90d2f
    /// tsc-span: _tsc.js:58618-58620
    fn get_constraint_declaration_for_mapped_type(&self, ty: TypeId) -> NodeId {
        let declaration = self.mapped_type_declaration(ty);
        let NodeData::MappedType(mapped) = self.data_of(declaration) else {
            unreachable!("mapped payload declaration has MappedType syntax kind");
        };
        let parameter = mapped
            .type_parameter
            .expect("parser invariant: mapped type has a type parameter");
        let NodeData::TypeParameter(parameter) = self.data_of(parameter) else {
            unreachable!("mapped type_parameter has TypeParameter data");
        };
        parameter
            .constraint
            .expect("checker-created mapped type has a constrained type parameter")
    }

    /// tsc-port: isMappedTypeWithKeyofConstraintDeclaration @6.0.3
    /// tsc-hash: f95faf3d1bf6affdbf92e192b4aec8de73c270e01d4819f54a2d9da28d5b43ef
    /// tsc-span: _tsc.js:58621-58624
    pub(crate) fn is_mapped_type_with_keyof_constraint_declaration(&self, ty: TypeId) -> bool {
        let constraint = self.get_constraint_declaration_for_mapped_type(ty);
        matches!(
            self.data_of(constraint),
            NodeData::TypeOperator(data)
                if data.operator == SyntaxKind::KeyOfKeyword
        )
    }

    /// tsc-port: getModifiersTypeFromMappedType @6.0.3
    /// tsc-hash: 0ecad4eda8ea5b1f794948c9035c31edd971212571922c96c716f671f4a3ca5a
    /// tsc-span: _tsc.js:58625-58637
    pub(crate) fn get_modifiers_type_from_mapped_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).mapped_modifiers_type.resolved() {
            return Ok(cached);
        }
        let mapped = self.mapped_type_data(ty);
        let resolved = if self.is_mapped_type_with_keyof_constraint_declaration(ty) {
            let constraint = self.get_constraint_declaration_for_mapped_type(ty);
            let NodeData::TypeOperator(operator) = self.data_of(constraint) else {
                unreachable!("keyof constraint has TypeOperator data");
            };
            let operand = operator
                .r#type
                .expect("parser invariant: keyof has an operand");
            let operand_type = self.get_type_from_type_node(operand)?;
            self.instantiate_type(operand_type, mapped.mapper)?
        } else {
            let declared = self.get_type_from_type_node(NodeId(mapped.declaration))?;
            let constraint = self.get_constraint_type_from_mapped_type(declared)?;
            let extended_constraint = if self
                .tables
                .flags_of(constraint)
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                self.get_constraint_of_type_parameter(constraint)?
            } else {
                Some(constraint)
            };
            match extended_constraint {
                Some(extended) if self.tables.flags_of(extended).intersects(TypeFlags::INDEX) => {
                    let TypeData::Index { ty: operand, .. } = self.tables.type_of(extended).data
                    else {
                        unreachable!("Index flag implies Index payload");
                    };
                    self.instantiate_type(operand, mapped.mapper)?
                }
                _ => self.tables.intrinsics.unknown,
            }
        };
        if let Some(cached) = self.links.ty(ty).mapped_modifiers_type.resolved() {
            return Ok(cached);
        }
        self.links
            .set_mapped_modifiers_type(self.speculation_depth, ty, resolved);
        Ok(resolved)
    }

    /// tsc-port: getMappedTypeNameTypeKind @6.0.3
    /// tsc-hash: 9dea04b3321844f615e94bc2b55d8c666bb11ac4638391437b1d4ef833d69170
    /// tsc-span: _tsc.js:58672-58678
    pub(crate) fn get_mapped_type_name_type_kind(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<MappedTypeNameTypeKind> {
        let Some(name_type) = self.get_name_type_from_mapped_type(ty)? else {
            return Ok(MappedTypeNameTypeKind::None);
        };
        let parameter = self.get_type_parameter_from_mapped_type(ty)?;
        Ok(if self.is_type_assignable_to(name_type, parameter)? {
            MappedTypeNameTypeKind::Filtering
        } else {
            MappedTypeNameTypeKind::Remapping
        })
    }

    /// tsrs-native: syntax-domain projection of `type.declaration.nameType`
    /// for the relation and indexed-access ports.
    pub(crate) fn mapped_type_declaration_has_name_type(&self, ty: TypeId) -> bool {
        let declaration = self.mapped_type_declaration(ty);
        matches!(
            self.data_of(declaration),
            NodeData::MappedType(mapped) if mapped.name_type.is_some()
        )
    }

    /// tsc-port: isMappedTypeGenericIndexedAccess @6.0.3
    /// tsc-hash: 47fcca6fc630c1206068c295d4d5b695e4927d0f58e88b3fb8f67dd43f842300
    /// tsc-span: _tsc.js:59089-59092
    pub(crate) fn is_mapped_type_generic_indexed_access(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<bool> {
        let TypeData::IndexedAccess {
            object_type,
            index_type,
            ..
        } = self.tables.type_of(ty).data
        else {
            return Ok(false);
        };
        if !self
            .tables
            .object_flags_of(object_type)
            .intersects(ObjectFlags::MAPPED)
            || self.is_generic_mapped_type_state(object_type)?
            || !self.is_generic_index_type_state(index_type)?
            || self
                .get_mapped_type_modifiers(object_type)
                .intersects(MappedTypeModifiers::EXCLUDE_OPTIONAL)
            || self.mapped_type_declaration_has_name_type(object_type)
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// tsc-port: substituteIndexedMappedType @6.0.3
    /// tsc-hash: fa7b0179f7b50aaa394482eedd579ac86c71e27631cffde2e0cb08a83ab94fac
    /// tsc-span: _tsc.js:62536-62547
    pub(crate) fn substitute_indexed_mapped_type(
        &mut self,
        object_type: TypeId,
        index: TypeId,
    ) -> CheckResult2<TypeId> {
        let type_parameter = self.get_type_parameter_from_mapped_type(object_type)?;
        let mapper = self.create_type_mapper(vec![type_parameter], Some(vec![index]));
        let mapped = self.mapped_type_data(object_type);
        let template_mapper = self.combine_type_mappers(mapped.mapper, mapper);
        let target = mapped.target.unwrap_or(object_type);
        let template = self.get_template_type_from_mapped_type(target)?;
        let instantiated_template = self.instantiate_type(template, Some(template_mapper))?;
        let is_optional = self.get_mapped_type_optionality(object_type) > 0
            || if self.is_generic_type(object_type)? {
                let modifiers = self.get_modifiers_type_from_mapped_type(object_type)?;
                self.get_combined_mapped_type_optionality(modifiers)? > 0
            } else {
                self.could_access_optional_property(object_type, index)?
            };
        Ok(self.tables.add_optionality(
            instantiated_template,
            /*is_property*/ true,
            is_optional,
        ))
    }

    /// tsc-port: couldAccessOptionalProperty @6.0.3
    /// tsc-hash: 9e1466e47e95dc90a1c56812d70ce53bf6ad15c0e6a81495439193421daf6625
    /// tsc-span: _tsc.js:62548-62551
    fn could_access_optional_property(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
    ) -> CheckResult2<bool> {
        let Some(index_constraint) = self.get_base_constraint_of_type(index_type)? else {
            return Ok(false);
        };
        for property in self.get_properties_of_type(object_type)? {
            if !self
                .symbol_flags(property)
                .intersects(SymbolFlags::OPTIONAL)
            {
                continue;
            }
            let literal = self.get_literal_type_from_property(
                property,
                TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
                /*include_non_public*/ true,
            )?;
            if self.is_type_assignable_to(literal, index_constraint)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsc-port: getApparentMappedTypeKeys @6.0.3
    /// tsc-hash: 1e0ba86eae6fe2bbb459888507d27b63a11680301841c47a5d9d5a660e4993ed
    /// tsc-span: _tsc.js:65930-65941
    pub(crate) fn get_apparent_mapped_type_keys(
        &mut self,
        name_type: TypeId,
        target_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let modifiers = self.get_modifiers_type_from_mapped_type(target_type)?;
        let apparent_modifiers = self.get_apparent_type(modifiers)?;
        let keys = self.mapped_property_and_index_key_types(
            apparent_modifiers,
            TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
            /*strings_only*/ false,
        )?;
        let parameter = self.get_type_parameter_from_mapped_type(target_type)?;
        let mapped = self.mapped_type_data(target_type);
        let mut mapped_keys = Vec::with_capacity(keys.len());
        for key in keys {
            let mapper = self.append_type_mapping(mapped.mapper, parameter, key);
            mapped_keys.push(self.instantiate_type(name_type, Some(mapper))?);
        }
        self.get_union_type_ex(&mapped_keys, UnionReduction::Literal)
    }

    /// tsc-port: getLowerBoundOfKeyType @6.0.3
    /// tsc-hash: 2c9ed9c229f1b32a41101ab7544eb0869d9c5be62b138a7d9e7af56599933032
    /// tsc-span: _tsc.js:58456-58492
    pub(crate) fn get_lower_bound_of_key_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INDEX) {
            let TypeData::Index { ty: operand, .. } = self.tables.type_of(ty).data else {
                unreachable!("Index flag implies Index payload");
            };
            let apparent = self.get_apparent_type(operand)?;
            return if self.tables.is_generic_tuple_type(apparent) {
                self.get_known_keys_of_tuple_type(apparent)
            } else {
                self.get_index_type(apparent, IndexFlags::NONE)
            };
        }
        if flags.intersects(TypeFlags::CONDITIONAL) {
            let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
                unreachable!("Conditional flag implies conditional data");
            };
            let root = self.tables.conditional_root(data.root).clone();
            if root.is_distributive {
                let constraint = self.get_lower_bound_of_key_type(data.check_type)?;
                if constraint != data.check_type {
                    let mapper =
                        self.prepend_type_mapping(root.check_type, constraint, data.mapper);
                    return self.get_conditional_type_instantiation(
                        ty, mapper, /*for_constraint*/ false, None, None,
                    );
                }
            }
            return Ok(ty);
        }
        if flags.intersects(TypeFlags::UNION) {
            let mapped = self.map_type(
                ty,
                &mut |state, member| state.get_lower_bound_of_key_type(member).map(Some),
                /*no_reductions*/ true,
            )?;
            return Ok(mapped.expect("lower-bound mapping never drops a union member"));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = &self.tables.type_of(ty).data else {
                unreachable!("Intersection flag implies Intersection payload");
            };
            let types = types.to_vec();
            if types.len() == 2
                && self
                    .tables
                    .flags_of(types[0])
                    .intersects(TypeFlags::STRING | TypeFlags::NUMBER | TypeFlags::BIG_INT)
                && types[1] == self.empty_type_literal_type
            {
                return Ok(ty);
            }
            let mut lowered = Vec::with_capacity(types.len());
            for member in types {
                lowered.push(self.get_lower_bound_of_key_type(member)?);
            }
            return self.get_intersection_type(&lowered, tsrs2_types::IntersectionFlags::NONE);
        }
        Ok(ty)
    }

    /// tsc-port: forEachMappedTypePropertyKeyTypeAndIndexSignatureKeyType @6.0.3
    /// tsc-hash: d26b11b1f4063192dfdb51fac637644ae93f61000b462f544d041031b3f4ee96
    /// tsc-span: _tsc.js:58496-58509
    fn mapped_property_and_index_key_types(
        &mut self,
        ty: TypeId,
        include: TypeFlags,
        strings_only: bool,
    ) -> CheckResult2<Vec<TypeId>> {
        let properties = self.get_properties_of_type_full(ty)?;
        let mut keys = Vec::with_capacity(properties.len());
        for property in properties {
            keys.push(self.get_literal_type_from_property(
                property, include, /*include_non_public*/ false,
            )?);
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            keys.push(self.tables.intrinsics.string);
        } else {
            for info in self.get_index_infos_of_type(ty)? {
                if !strings_only
                    || self
                        .tables
                        .flags_of(info.key_type)
                        .intersects(TypeFlags::STRING | TypeFlags::TEMPLATE_LITERAL)
                {
                    keys.push(info.key_type);
                }
            }
        }
        Ok(keys)
    }

    /// tsc-port: resolveMappedTypeMembers @6.0.3
    /// tsc-hash: 24d56cfe94497835f00ce92b68c33b908ce35213aed2242621b2c7479dd3c159
    /// tsc-span: _tsc.js:58510-58576
    pub(crate) fn resolve_mapped_type_members(&mut self, ty: TypeId) -> CheckResult2<MembersId> {
        if let Some(cached) = self.links.ty(ty).resolved_members.resolved() {
            return Ok(cached);
        }

        // tsc publishes an empty resolved shell before walking the key
        // domain so recursive readers terminate. Rust Unsupported
        // unwinds retract it, preserving the repository cache protocol.
        let members_id = self.alloc_members(ResolvedMembers::default());
        self.links
            .set_type_members(self.speculation_depth, ty, LinkSlot::Resolved(members_id));
        let filled = (|state: &mut Self| -> CheckResult2<ResolvedMembers> {
            let type_parameter = state.get_type_parameter_from_mapped_type(ty)?;
            let constraint_type = state.get_constraint_type_from_mapped_type(ty)?;
            let mapped_data = state.mapped_type_data(ty);
            let mapped_type = mapped_data.target.unwrap_or(ty);
            let name_type = state.get_name_type_from_mapped_type(mapped_type)?;
            let should_link_prop_declarations = state
                .get_mapped_type_name_type_kind(mapped_type)?
                != MappedTypeNameTypeKind::Remapping;
            let template_type = state.get_template_type_from_mapped_type(mapped_type)?;
            let modifiers_raw = state.get_modifiers_type_from_mapped_type(ty)?;
            let modifiers_type = state.get_apparent_type(modifiers_raw)?;
            let template_modifiers = state.get_mapped_type_modifiers(ty);
            let include = TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE;
            let keys = if state.is_mapped_type_with_keyof_constraint_declaration(ty) {
                state.mapped_property_and_index_key_types(
                    modifiers_type,
                    include,
                    /*strings_only*/ false,
                )?
            } else {
                let lower = state.get_lower_bound_of_key_type(constraint_type)?;
                state.union_members_or_self(lower)
            };

            let mut members = SymbolTable::default();
            let mut index_infos = Vec::new();
            for key_type in keys {
                let prop_name_type = match name_type {
                    Some(name_type) => {
                        let mapper =
                            state.append_type_mapping(mapped_data.mapper, type_parameter, key_type);
                        state.instantiate_type(name_type, Some(mapper))?
                    }
                    None => key_type,
                };
                for member_name_type in state.union_members_or_self(prop_name_type) {
                    state.add_mapped_member_for_key_type(
                        ty,
                        type_parameter,
                        key_type,
                        member_name_type,
                        template_type,
                        modifiers_type,
                        template_modifiers,
                        should_link_prop_declarations,
                        mapped_data.mapper,
                        &mut members,
                        &mut index_infos,
                    )?;
                }
            }
            let properties = members.values().copied().collect();
            Ok(ResolvedMembers {
                members,
                properties,
                call_signatures: Vec::new(),
                construct_signatures: Vec::new(),
                index_infos,
            })
        })(self);
        match filled {
            Ok(resolved) => {
                *self.members_mut(members_id) = resolved;
                Ok(members_id)
            }
            Err(error) => {
                self.links.retract_type_members(ty);
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_mapped_member_for_key_type(
        &mut self,
        mapped_type: TypeId,
        type_parameter: TypeId,
        key_type: TypeId,
        prop_name_type: TypeId,
        template_type: TypeId,
        modifiers_type: TypeId,
        template_modifiers: MappedTypeModifiers,
        should_link_prop_declarations: bool,
        mapper: Option<tsrs2_types::MapperId>,
        members: &mut SymbolTable,
        index_infos: &mut Vec<IndexInfo>,
    ) -> CheckResult2<()> {
        if self.is_type_usable_as_property_name(prop_name_type) {
            let prop_name = self
                .get_property_name_from_type(prop_name_type)
                .expect("usable property-name type has a property name");
            if let Some(existing) = members.get(&prop_name).copied() {
                let existing_name = self
                    .links
                    .symbol(existing)
                    .name_type
                    .expect("mapped property has nameType");
                let existing_key = self
                    .links
                    .symbol(existing)
                    .key_type
                    .expect("mapped property has keyType");
                let union_name = self
                    .get_union_type_ex(&[existing_name, prop_name_type], UnionReduction::Literal)?;
                let union_key =
                    self.get_union_type_ex(&[existing_key, key_type], UnionReduction::Literal)?;
                self.links.update_symbol_mapped_name_and_key(
                    self.speculation_depth,
                    existing,
                    union_name,
                    union_key,
                );
                return Ok(());
            }

            let modifiers_prop = if self.is_type_usable_as_property_name(key_type) {
                let modifier_name = self
                    .get_property_name_from_type(key_type)
                    .expect("usable property-name type has a property name");
                self.get_property_of_type_full(modifiers_type, &modifier_name)?
            } else {
                None
            };
            let is_optional = template_modifiers.intersects(MappedTypeModifiers::INCLUDE_OPTIONAL)
                || !template_modifiers.intersects(MappedTypeModifiers::EXCLUDE_OPTIONAL)
                    && modifiers_prop.is_some_and(|property| {
                        self.symbol_flags(property)
                            .intersects(SymbolFlags::OPTIONAL)
                    });
            let is_readonly = template_modifiers.intersects(MappedTypeModifiers::INCLUDE_READONLY)
                || !template_modifiers.intersects(MappedTypeModifiers::EXCLUDE_READONLY)
                    && modifiers_prop.is_some_and(|property| self.is_readonly_symbol(property));
            let strip_optional = self.tables.strict_null_checks
                && !is_optional
                && modifiers_prop.is_some_and(|property| {
                    self.symbol_flags(property)
                        .intersects(SymbolFlags::OPTIONAL)
                });
            let late = modifiers_prop
                .map(|property| self.get_check_flags(property))
                .unwrap_or(CheckFlags::NONE);
            let late = CheckFlags::from_bits(late.bits() & CheckFlags::LATE.bits());
            let symbol_flags = SymbolFlags::PROPERTY
                | if is_optional {
                    SymbolFlags::OPTIONAL
                } else {
                    SymbolFlags::NONE
                };
            let property = self.binder.create_symbol(symbol_flags, prop_name.clone());
            let check_flags = CheckFlags::from_bits(
                late.bits()
                    | CheckFlags::MAPPED.bits()
                    | if is_readonly {
                        CheckFlags::READONLY.bits()
                    } else {
                        0
                    }
                    | if strip_optional {
                        CheckFlags::STRIP_OPTIONAL.bits()
                    } else {
                        0
                    },
            );
            self.links
                .set_symbol_check_flags(self.speculation_depth, property, check_flags);
            self.links.set_symbol_mapped_links(
                self.speculation_depth,
                property,
                mapped_type,
                prop_name_type,
                key_type,
            );
            if let Some(modifiers_prop) = modifiers_prop {
                self.links.set_symbol_synthetic_origin(
                    self.speculation_depth,
                    property,
                    modifiers_prop,
                );
                if should_link_prop_declarations {
                    self.binder.symbol_mut(property).declarations =
                        self.binder.symbol(modifiers_prop).declarations.clone();
                }
            }
            members.insert(prop_name, property);
            return Ok(());
        }

        let prop_name_flags = self.tables.flags_of(prop_name_type);
        if self.is_valid_index_key_type(prop_name_type)?
            || prop_name_flags.intersects(TypeFlags::ANY | TypeFlags::ENUM)
        {
            let index_key_type = if prop_name_flags.intersects(TypeFlags::ANY | TypeFlags::STRING) {
                self.tables.intrinsics.string
            } else if prop_name_flags.intersects(TypeFlags::NUMBER | TypeFlags::ENUM) {
                self.tables.intrinsics.number
            } else {
                prop_name_type
            };
            let template_mapper = self.append_type_mapping(mapper, type_parameter, key_type);
            let prop_type = self.instantiate_type(template_type, Some(template_mapper))?;
            let modifiers_index_info =
                self.get_applicable_index_info(modifiers_type, prop_name_type)?;
            let is_readonly = template_modifiers.intersects(MappedTypeModifiers::INCLUDE_READONLY)
                || !template_modifiers.intersects(MappedTypeModifiers::EXCLUDE_READONLY)
                    && modifiers_index_info.is_some_and(|info| info.is_readonly);
            self.append_index_info(
                index_infos,
                IndexInfo {
                    key_type: index_key_type,
                    value_type: prop_type,
                    is_readonly,
                    declaration: None,
                    components: None,
                    is_enum_number_index_info: false,
                },
                /*union*/ true,
            )?;
        }
        Ok(())
    }

    /// tsc-port: getTypeOfMappedSymbol @6.0.3
    /// tsc-hash: 85d45ee7090072ed14a4923ad21f7d37bf580d8ea2078dc0a06c428c85364c5c
    /// tsc-span: _tsc.js:58577-58600
    pub(crate) fn get_type_of_mapped_symbol(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        let mapped_type = self
            .links
            .symbol(symbol)
            .mapped_type
            .expect("Mapped check flag implies links.mappedType");
        if !self.push_type_resolution(
            ResolutionTarget::Symbol(symbol),
            TypeSystemPropertyName::TYPE,
        ) {
            self.links
                .set_mapped_contains_error(self.speculation_depth, mapped_type);
            return Ok(self.tables.intrinsics.error);
        }
        let computed = (|state: &mut Self| -> CheckResult2<TypeId> {
            let mapped = state.mapped_type_data(mapped_type);
            let target = mapped.target.unwrap_or(mapped_type);
            let template_type = state.get_template_type_from_mapped_type(target)?;
            let key_type = state
                .links
                .symbol(symbol)
                .key_type
                .expect("mapped property has keyType");
            let type_parameter = state.get_type_parameter_from_mapped_type(mapped_type)?;
            let mapper = state.append_type_mapping(mapped.mapper, type_parameter, key_type);
            let prop_type = state.instantiate_type(template_type, Some(mapper))?;
            let optional = state.tables.strict_null_checks
                && state.symbol_flags(symbol).intersects(SymbolFlags::OPTIONAL)
                && !state.maybe_type_of_kind(prop_type, TypeFlags::UNDEFINED | TypeFlags::VOID);
            if optional {
                state.get_optional_type(prop_type, /*is_property*/ true)
            } else if state
                .get_check_flags(symbol)
                .intersects(CheckFlags::STRIP_OPTIONAL)
            {
                state.remove_missing_or_undefined_type(prop_type)
            } else {
                Ok(prop_type)
            }
        })(self);
        let mut computed = match computed {
            Ok(computed) => computed,
            Err(error) => {
                self.pop_type_resolution();
                return Err(error);
            }
        };
        if !self.pop_type_resolution() {
            self.links
                .set_mapped_contains_error(self.speculation_depth, mapped_type);
            let property_name = self.symbol_display_name(symbol);
            let mapped_text = self.type_to_string_slice(mapped_type)?;
            self.error_at(
                self.current_node,
                &diagnostics::Type_of_property_0_circularly_references_itself_in_mapped_type_1,
                &[&property_name, &mapped_text],
            );
            computed = self.tables.intrinsics.error;
        }
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        self.links
            .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(computed));
        Ok(computed)
    }

    /// tsc-port: getApparentTypeOfMappedType @6.0.3
    /// tsc-hash: 63dc25b3158fd6cad94c5b30d6a17744e81d9dd7ec6dc18d1978535d1de9bee6
    /// tsc-span: _tsc.js:59071-59073
    pub(crate) fn get_apparent_type_of_mapped_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).mapped_apparent_type.resolved() {
            return Ok(cached);
        }
        let resolved = self.get_resolved_apparent_type_of_mapped_type(ty)?;
        if let Some(cached) = self.links.ty(ty).mapped_apparent_type.resolved() {
            return Ok(cached);
        }
        self.links
            .set_mapped_apparent_type(self.speculation_depth, ty, resolved);
        Ok(resolved)
    }

    /// tsc-port: getResolvedApparentTypeOfMappedType @6.0.3
    /// tsc-hash: 53d4ca8bed39f305d47dd19d644781edab281f036b776704cd1ecb745328aab9
    /// tsc-span: _tsc.js:59074-59085
    fn get_resolved_apparent_type_of_mapped_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let mapped = self.mapped_type_data(ty);
        let target = mapped.target.unwrap_or(ty);
        let Some(type_variable) = self.get_homomorphic_type_variable(target)? else {
            return Ok(ty);
        };
        let declaration = self.mapped_type_declaration(target);
        let NodeData::MappedType(declaration) = self.data_of(declaration) else {
            unreachable!("mapped declaration has MappedType data");
        };
        if declaration.name_type.is_some() {
            return Ok(ty);
        }
        let modifiers_type = self.get_modifiers_type_from_mapped_type(ty)?;
        let base_constraint = if self.is_generic_mapped_type_state(modifiers_type)? {
            Some(self.get_apparent_type_of_mapped_type(modifiers_type)?)
        } else {
            self.get_base_constraint_of_type(modifiers_type)?
        };
        let Some(base_constraint) = base_constraint else {
            return Ok(ty);
        };
        let mut array_or_tuple_domain = true;
        for member in self.union_members_or_self(base_constraint) {
            if !self.is_array_or_tuple_or_intersection(member)? {
                array_or_tuple_domain = false;
                break;
            }
        }
        if array_or_tuple_domain {
            let mapper = self.prepend_type_mapping(type_variable, base_constraint, mapped.mapper);
            return self.instantiate_type(target, Some(mapper));
        }
        Ok(ty)
    }

    fn is_array_or_tuple_or_intersection(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if self.is_array_type(ty)? || self.tables.is_tuple_type(ty) {
            return Ok(true);
        }
        if !self.tables.flags_of(ty).intersects(TypeFlags::INTERSECTION) {
            return Ok(false);
        }
        let TypeData::Intersection { types } = &self.tables.type_of(ty).data else {
            unreachable!("intersection flag implies intersection payload");
        };
        let types = types.to_vec();
        for member in types {
            if !self.is_array_type(member)? && !self.tables.is_tuple_type(member) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: getIndexTypeForMappedType @6.0.3
    /// tsc-hash: 8823e4b8b63be7e1b18d57facd6840364590cc4af2c04b0c77fe82859e8d08d7
    /// tsc-span: _tsc.js:61935-61963
    pub(crate) fn get_index_type_for_mapped_type(
        &mut self,
        ty: TypeId,
        index_flags: IndexFlags,
    ) -> CheckResult2<TypeId> {
        let type_parameter = self.get_type_parameter_from_mapped_type(ty)?;
        let constraint_type = self.get_constraint_type_from_mapped_type(ty)?;
        let mapped = self.mapped_type_data(ty);
        let target = mapped.target.unwrap_or(ty);
        let name_type = self.get_name_type_from_mapped_type(target)?;
        if name_type.is_none() && !index_flags.intersects(IndexFlags::NO_INDEX_SIGNATURES) {
            return Ok(constraint_type);
        }

        if self.is_generic_index_type_state(constraint_type)?
            && self.is_mapped_type_with_keyof_constraint_declaration(ty)
        {
            return Ok(self.get_index_type_for_generic_type(ty, index_flags));
        }
        let keys = if self.is_generic_index_type_state(constraint_type)? {
            self.union_members_or_self(constraint_type)
        } else if self.is_mapped_type_with_keyof_constraint_declaration(ty) {
            let modifiers_raw = self.get_modifiers_type_from_mapped_type(ty)?;
            let modifiers_type = self.get_apparent_type(modifiers_raw)?;
            self.mapped_property_and_index_key_types(
                modifiers_type,
                TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
                index_flags.intersects(IndexFlags::STRINGS_ONLY),
            )?
        } else {
            let lower = self.get_lower_bound_of_key_type(constraint_type)?;
            self.union_members_or_self(lower)
        };
        let mut key_types = Vec::with_capacity(keys.len());
        for key_type in keys {
            let prop_name_type = match name_type {
                Some(name_type) => {
                    let mapper = self.append_type_mapping(mapped.mapper, type_parameter, key_type);
                    self.instantiate_type(name_type, Some(mapper))?
                }
                None => key_type,
            };
            key_types.push(if prop_name_type == self.tables.intrinsics.string {
                self.tables.intrinsics.string_or_number
            } else {
                prop_name_type
            });
        }
        let mut result = self.get_union_type_ex(&key_types, UnionReduction::Literal)?;
        if index_flags.intersects(IndexFlags::NO_INDEX_SIGNATURES) {
            result = self.tables.filter_type(result, |tables, member| {
                !tables
                    .flags_of(member)
                    .intersects(TypeFlags::ANY | TypeFlags::STRING)
            });
        }
        if self.tables.flags_of(result).intersects(TypeFlags::UNION)
            && self
                .tables
                .flags_of(constraint_type)
                .intersects(TypeFlags::UNION)
        {
            let result_members = self.union_members_or_self(result);
            let constraint_members = self.union_members_or_self(constraint_type);
            if result_members == constraint_members {
                return Ok(constraint_type);
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_syntax::NodeData;
    use tsrs2_types::{
        CompilerOptions, ElementFlags, IndexFlags, SymbolFlags, TypeData, TypeFlags, TypeId,
    };

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
        let annotation =
            find_probe_annotation(state.binder.source(0), name).expect("fixture annotation");
        state
            .get_type_from_type_node(annotation)
            .expect("mapped annotation resolves")
    }

    fn property(state: &mut CheckerState, ty: TypeId, name: &str) -> tsrs2_binder::SymbolId {
        state
            .get_property_of_type_full(ty, name)
            .expect("mapped members resolve")
            .expect("mapped property exists")
    }

    fn parameter_annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
        let annotation = {
            let source = state.binder.source(0);
            (0..source.arena.len())
                .find_map(|index| {
                    let NodeData::Parameter(parameter) =
                        &source.arena.node(tsrs2_syntax::NodeId(index as u32)).data
                    else {
                        return None;
                    };
                    let declared_name = parameter.name?;
                    let NodeData::Identifier(identifier) = &source.arena.node(declared_name).data
                    else {
                        return None;
                    };
                    (identifier.text == name)
                        .then_some(parameter.r#type)
                        .flatten()
                })
                .expect("fixture parameter annotation")
        };
        state
            .get_type_from_type_node(annotation)
            .expect("parameter annotation resolves")
    }

    #[test]
    fn finite_mapped_members_remap_duplicate_keys_and_instantiate_values() {
        with_program_state(
            &[(
                "a.ts",
                "declare let finite: { [K in \"a\" | \"b\"]?: K };\n\
                 declare let remapped: { [K in \"a\" | \"b\" as `x${K}`]-?: K };\n\
                 declare let duplicate: { [K in \"a\" | \"b\" as \"x\"]: K };\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let finite = annotation_type(state, "finite");
                assert!(!state
                    .is_generic_mapped_type_state(finite)
                    .expect("finite mapped classifier"));
                let names: Vec<_> = state
                    .get_properties_of_type_full(finite)
                    .expect("finite properties")
                    .into_iter()
                    .map(|symbol| state.binder.symbol(symbol).escaped_name.clone())
                    .collect();
                assert_eq!(names, ["a", "b"]);
                let a = property(state, finite, "a");
                assert!(state.symbol_flags(a).intersects(SymbolFlags::OPTIONAL));
                let a_type = state.get_type_of_symbol(a).expect("mapped value types");
                let a_text = state.type_to_string_slice(a_type).expect("value renders");
                assert!(a_text.contains("\"a\""), "{a_text}");
                assert!(a_text.contains("undefined"), "{a_text}");

                let remapped = annotation_type(state, "remapped");
                let remapped_names: Vec<_> = state
                    .get_properties_of_type_full(remapped)
                    .expect("remapped properties")
                    .into_iter()
                    .map(|symbol| state.binder.symbol(symbol).escaped_name.clone())
                    .collect();
                assert_eq!(remapped_names, ["xa", "xb"]);
                let xa = property(state, remapped, "xa");
                assert!(!state.symbol_flags(xa).intersects(SymbolFlags::OPTIONAL));
                let xa_type = state.get_type_of_symbol(xa).expect("xa type");
                assert_eq!(
                    state.type_to_string_slice(xa_type).expect("xa renders"),
                    "\"a\""
                );

                let duplicate = annotation_type(state, "duplicate");
                let x = property(state, duplicate, "x");
                let x_type = state.get_type_of_symbol(x).expect("duplicate value union");
                assert!(state.tables.flags_of(x_type).intersects(TypeFlags::UNION));
                let TypeData::Union { types, .. } = &state.tables.type_of(x_type).data else {
                    panic!("duplicate key value is a union");
                };
                assert_eq!(types.len(), 2);
            },
        );
    }

    #[test]
    fn mapped_members_copy_modifiers_create_index_info_and_report_keyof() {
        with_program_state(
            &[(
                "a.ts",
                "declare let copied: { [K in keyof { readonly a?: number; b: string }]-?: K };\n\
                 declare let indexed: { readonly [K in string]: number };\n\
                 declare let remapped: { [K in \"a\" | \"b\" as `x${K}`]: K };\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let copied = annotation_type(state, "copied");
                let a = property(state, copied, "a");
                let b = property(state, copied, "b");
                assert!(state.is_readonly_symbol(a));
                assert!(!state.is_readonly_symbol(b));
                assert!(!state.symbol_flags(a).intersects(SymbolFlags::OPTIONAL));

                let indexed = annotation_type(state, "indexed");
                let infos = state
                    .get_index_infos_of_type(indexed)
                    .expect("mapped index info");
                assert_eq!(infos.len(), 1);
                assert_eq!(infos[0].key_type, state.tables.intrinsics.string);
                assert_eq!(infos[0].value_type, state.tables.intrinsics.number);
                assert!(infos[0].is_readonly);

                let remapped = annotation_type(state, "remapped");
                let keys = state
                    .get_index_type(remapped, IndexFlags::NONE)
                    .expect("keyof remapped mapped type");
                let key_text = state.type_to_string_slice(keys).expect("key union renders");
                assert!(key_text.contains("\"xa\""), "{key_text}");
                assert!(key_text.contains("\"xb\""), "{key_text}");
            },
        );
    }

    #[test]
    fn homomorphic_mapped_instantiation_preserves_array_and_tuple_shapes() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { [n: number]: T }\n\
                 interface ReadonlyArray<T> { readonly [n: number]: T }\n\
                 type Identity<T> = { [K in keyof T]: T[K] };\n\
                 type Mutable<T> = { -readonly [K in keyof T]: T[K] };\n\
                 type RequiredTuple<T> = { [K in keyof T]-?: T[K] };\n\
                 declare let tuple: Identity<readonly [number, string?]>;\n\
                 declare let mutable: Mutable<readonly number[]>;\n\
                 declare let required: RequiredTuple<[number, string?]>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let tuple = annotation_type(state, "tuple");
                assert!(state.tables.is_tuple_type(tuple));
                let tuple_target = state.tables.reference_target(tuple);
                let TypeData::TupleTarget(tuple_data) =
                    state.tables.type_of(tuple_target).data.clone()
                else {
                    panic!("tuple instantiation retains a tuple target");
                };
                assert!(tuple_data.readonly);
                assert!(tuple_data.element_flags[1].intersects(ElementFlags::OPTIONAL));
                let tuple_arguments = state.get_type_arguments(tuple).expect("tuple elements");
                assert_eq!(tuple_arguments[0], state.tables.intrinsics.number);

                let mutable = annotation_type(state, "mutable");
                let mutable_text = state
                    .type_to_string_slice(mutable)
                    .expect("mutable renders");
                assert!(
                    state.is_array_type(mutable).expect("array predicate"),
                    "{mutable_text}: {:?}",
                    state.tables.type_of(mutable).data
                );
                assert!(!state
                    .is_readonly_array_type(mutable)
                    .expect("readonly predicate"));
                assert_eq!(
                    state
                        .get_element_type_of_array_type(mutable)
                        .expect("array element"),
                    Some(state.tables.intrinsics.number)
                );

                let required = annotation_type(state, "required");
                assert!(state.tables.is_tuple_type(required));
                let required_target = state.tables.reference_target(required);
                let TypeData::TupleTarget(required_data) =
                    state.tables.type_of(required_target).data.clone()
                else {
                    panic!("required mapped tuple retains a tuple target");
                };
                assert!(required_data.element_flags[1].intersects(ElementFlags::REQUIRED));
            },
        );
    }

    #[test]
    fn apparent_homomorphic_mapped_type_uses_array_base_constraint() {
        with_program_state(
            &[(
                "a.ts",
                "interface Array<T> { [n: number]: T }\n\
                 interface ReadonlyArray<T> { readonly [n: number]: T }\n\
                 function f<T extends readonly string[]>(value: { [K in keyof T]: number }) {}\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let mapped = parameter_annotation_type(state, "value");
                assert!(state
                    .is_generic_mapped_type_state(mapped)
                    .expect("generic mapped classifier"));
                let apparent = state
                    .get_apparent_type(mapped)
                    .expect("mapped apparent type resolves");
                let apparent_text = state
                    .type_to_string_slice(apparent)
                    .expect("apparent renders");
                assert!(
                    state
                        .is_readonly_array_type(apparent)
                        .expect("apparent readonly array"),
                    "{apparent_text}: {:?}",
                    state.tables.type_of(apparent).data
                );
                assert_eq!(
                    state
                        .get_element_type_of_array_type(apparent)
                        .expect("apparent array element"),
                    Some(state.tables.intrinsics.number)
                );
            },
        );
    }

    #[test]
    fn generic_indexed_mapped_substitution_preserves_template_and_optionality() {
        with_program_state(
            &[(
                "a.ts",
                "function f<K extends \"a\" | \"b\">(\n\
                   value: { [P in \"a\" | \"b\"]: P }[K],\n\
                   optional: { [P in \"a\" | \"b\"]?: P }[K]\n\
                 ) {}\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let value = parameter_annotation_type(state, "value");
                let TypeData::IndexedAccess {
                    object_type,
                    index_type,
                    ..
                } = state.tables.type_of(value).data
                else {
                    panic!("generic mapped access remains deferred");
                };
                assert!(state
                    .is_mapped_type_generic_indexed_access(value)
                    .expect("mapped generic indexed classifier"));
                let substituted = state
                    .substitute_indexed_mapped_type(object_type, index_type)
                    .expect("mapped template substitutes");
                assert_eq!(
                    state
                        .type_to_string_slice(substituted)
                        .expect("substitution renders"),
                    "K"
                );
                assert_eq!(
                    state
                        .get_constraint_of_indexed_access(value)
                        .expect("constraint resolves"),
                    Some(substituted)
                );

                let optional = parameter_annotation_type(state, "optional");
                let TypeData::IndexedAccess {
                    object_type,
                    index_type,
                    ..
                } = state.tables.type_of(optional).data
                else {
                    panic!("optional generic mapped access remains deferred");
                };
                let substituted = state
                    .substitute_indexed_mapped_type(object_type, index_type)
                    .expect("optional mapped template substitutes");
                let rendered = state
                    .type_to_string_slice(substituted)
                    .expect("optional substitution renders");
                assert!(rendered.contains('K'), "{rendered}");
                assert!(rendered.contains("undefined"), "{rendered}");
            },
        );
    }

    #[test]
    fn generic_mapped_relations_compare_constraint_template_and_optionality() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T>(\n\
                   required: { [P in keyof T]: T[P] },\n\
                   same: { [Q in keyof T]: T[Q] },\n\
                   optional: { [P in keyof T]?: T[P] },\n\
                   empty: {}\n\
                 ) {}\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let required = parameter_annotation_type(state, "required");
                let same = parameter_annotation_type(state, "same");
                let optional = parameter_annotation_type(state, "optional");
                let empty = parameter_annotation_type(state, "empty");
                assert!(state
                    .is_type_assignable_to(required, same)
                    .expect("equivalent mapped relation"));
                assert!(state
                    .is_type_assignable_to(required, optional)
                    .expect("required maps to optional"));
                assert!(!state
                    .is_type_assignable_to(optional, required)
                    .expect("optional does not map to required"));
                assert!(state
                    .is_type_assignable_to(empty, optional)
                    .expect("empty object maps to a partial mapped target"));
            },
        );
    }
}

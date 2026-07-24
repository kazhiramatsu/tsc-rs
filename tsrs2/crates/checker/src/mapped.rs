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
    CheckFlags, IndexFlags, MappedTypeModifiers, SymbolFlags, TypeData, TypeFlags, TypeId,
    TypeSystemPropertyName, UnionReduction,
};

use crate::links::LinkSlot;
use crate::state::{
    CheckResult2, CheckerState, IndexInfo, MembersId, ResolutionTarget, ResolvedMembers,
    Unsupported,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MappedTypeNameTypeKind {
    None,
    Filtering,
    Remapping,
}

impl<'a> CheckerState<'a> {
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
    fn get_mapped_type_name_type_kind(
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
            // tsc's distributive branch requires the phase-9.6
            // conditional/substitution model. Do not turn the
            // conditional into an arbitrary key domain.
            return Err(Unsupported::new(
                "getLowerBoundOfKeyType over conditional keys (9.6/M8)",
            ));
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
        if self.is_valid_index_key_type(prop_name_type)
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

    /// 9.5b1's finite face of getResolvedApparentTypeOfMappedType.
    /// The homomorphic array/tuple transformation is owned with mapped
    /// instantiation in 9.5b2; every other mapped apparent type is
    /// identity in tsc.
    fn get_resolved_apparent_type_of_mapped_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let mapped = self.mapped_type_data(ty);
        let target = mapped.target.unwrap_or(ty);
        let homomorphic = self.get_homomorphic_type_variable(target)?;
        let declaration = self.mapped_type_declaration(target);
        let NodeData::MappedType(declaration) = self.data_of(declaration) else {
            unreachable!("mapped declaration has MappedType data");
        };
        if homomorphic.is_some() && declaration.name_type.is_none() {
            return Err(Unsupported::new(
                "homomorphic mapped apparent array/tuple transformation (9.5b2/M8)",
            ));
        }
        Ok(ty)
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

        if self.tables.is_generic_index_type(constraint_type)
            && self.is_mapped_type_with_keyof_constraint_declaration(ty)
        {
            return Ok(self.get_index_type_for_generic_type(ty, index_flags));
        }
        let keys = if self.tables.is_generic_index_type(constraint_type) {
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
    use tsrs2_types::{CompilerOptions, IndexFlags, SymbolFlags, TypeData, TypeFlags, TypeId};

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
}

//! M4 class band (§6) — seeded at 5.8a with the index-constraint and
//! duplicate-member workers that checkTypeLiteral's lazy block pulls
//! forward (m4-58 §11); checkClassLikeDeclaration and the member
//! override bands land at 5.8c.

use tsrs2_binder::SymbolId;
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{InternalSymbolName, ModifierFlags, ObjectFlags, SymbolFlags, TypeFlags, TypeId};

use crate::state::{CheckResult2, CheckerState, IndexInfo};

impl<'a> CheckerState<'a> {
    /// tsc-port: checkIndexConstraints @6.0.3
    /// tsc-hash: caf4507093d400ddc13209bd8f6aa7dfaa09b62b7fca9d9b3388369817ac81f5
    /// tsc-span: _tsc.js:84705-84734
    ///
    /// The classLike valueDeclaration arm (computed non-bindable
    /// member names) is transcribed for the 5.8c callers; 5.8a's
    /// TypeLiteral callers never take it.
    pub(crate) fn check_index_constraints(
        &mut self,
        ty: TypeId,
        symbol: SymbolId,
        is_static_index: bool,
    ) -> CheckResult2<()> {
        let index_infos = self.get_index_infos_of_type(ty)?;
        if index_infos.is_empty() {
            return Ok(());
        }
        for prop in self.get_properties_of_object_type_owned(ty)? {
            if !(is_static_index
                && self
                    .binder
                    .symbol(prop)
                    .flags
                    .intersects(SymbolFlags::PROTOTYPE))
            {
                let prop_name_type = self.get_literal_type_from_property(
                    prop,
                    TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
                    /*include_non_public*/ true,
                )?;
                let prop_type = self.get_non_missing_type_of_symbol(prop)?;
                self.check_index_constraint_for_property(ty, prop, prop_name_type, prop_type)?;
            }
        }
        let type_declaration = self.binder.symbol(symbol).value_declaration;
        if let Some(type_declaration) = type_declaration {
            if matches!(
                self.kind_of(type_declaration),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) {
                let members = match self.data_of(type_declaration) {
                    NodeData::ClassDeclaration(data) => data.members,
                    NodeData::ClassExpression(data) => data.members,
                    _ => None,
                };
                for member in self.nodes_of(members) {
                    let member_is_static = tsrs2_binder::node_util::has_syntactic_modifier(
                        self.binder.source_of_node(member),
                        member,
                        ModifierFlags::STATIC,
                    );
                    // hasBindableName = !hasDynamicName ||
                    // hasLateBindableName — the late-bindable refinement
                    // rides the 5.8c class callers (this arm is dead for
                    // 5.8a's TypeLiteral callers).
                    let has_bindable_name = !tsrs2_binder::node_util::has_dynamic_name(
                        self.binder.source_of_node(member),
                        member,
                    );
                    if (is_static_index == member_is_static) && !has_bindable_name {
                        let member_symbol = self.get_symbol_of_declaration(member)?;
                        let name_expression = self
                            .name_of_node(member)
                            .and_then(|name| match self.data_of(name) {
                                NodeData::ComputedPropertyName(data) => data.expression,
                                _ => None,
                            });
                        let Some(name_expression) = name_expression else {
                            continue;
                        };
                        let prop_name_type = self.get_type_of_expression(name_expression)?;
                        let prop_type = self.get_non_missing_type_of_symbol(member_symbol)?;
                        self.check_index_constraint_for_property(
                            ty,
                            member_symbol,
                            prop_name_type,
                            prop_type,
                        )?;
                    }
                }
            }
        }
        if index_infos.len() > 1 {
            for info in &index_infos {
                self.check_index_constraint_for_index_signature(ty, info)?;
            }
        }
        Ok(())
    }

    /// tsc-port: getApplicableIndexInfos @6.0.3
    /// tsc-hash: a4db6e0cc48a2bac1fcdc015fbb18b8eb611da09d0b23e6bcf15ae597c0c4ef1
    /// tsc-span: _tsc.js:59473-59475
    fn get_applicable_index_infos(
        &mut self,
        ty: TypeId,
        key_type: TypeId,
    ) -> CheckResult2<Vec<IndexInfo>> {
        let infos = self.get_index_infos_of_type(ty)?;
        let mut applicable = Vec::new();
        for info in infos {
            if self.is_applicable_index_type(key_type, info.key_type)? {
                applicable.push(info);
            }
        }
        Ok(applicable)
    }

    /// tsc-port: checkIndexConstraintForProperty @6.0.3
    /// tsc-hash: f8520780f00ee44b88b16706ff8a30afdb49ae0774daff2d7aa615c45b4f8508
    /// tsc-span: _tsc.js:84735-84756
    ///
    /// Display band (risk §14.4): the 2411 report renders four types/
    /// symbols — an unrenderable display unwinds Unsupported and the
    /// report escapes whole.
    fn check_index_constraint_for_property(
        &mut self,
        ty: TypeId,
        prop: SymbolId,
        prop_name_type: TypeId,
        prop_type: TypeId,
    ) -> CheckResult2<()> {
        let declaration = self.binder.symbol(prop).value_declaration;
        let name = declaration.and_then(|declaration| self.name_of_node(declaration));
        if name.is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier) {
            return Ok(());
        }
        let index_infos = self.get_applicable_index_infos(ty, prop_name_type)?;
        let interface_declaration = if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::INTERFACE)
        {
            self.tables.type_of(ty).symbol.and_then(|symbol| {
                self.get_declaration_of_kind(symbol, SyntaxKind::InterfaceDeclaration)
            })
        } else {
            None
        };
        let prop_declaration = declaration.filter(|&declaration| {
            self.kind_of(declaration) == SyntaxKind::BinaryExpression
                || name.is_some_and(|name| self.kind_of(name) == SyntaxKind::ComputedPropertyName)
        });
        let local_prop_declaration = declaration.filter(|_| {
            self.get_parent_of_symbol(prop) == self.tables.type_of(ty).symbol
                && self.tables.type_of(ty).symbol.is_some()
        });
        for info in index_infos {
            let local_index_declaration = info.declaration.filter(|&index_declaration| {
                self.get_symbol_of_declaration(index_declaration)
                    .ok()
                    .and_then(|symbol| self.get_parent_of_symbol(symbol))
                    == self.tables.type_of(ty).symbol
                    && self.tables.type_of(ty).symbol.is_some()
            });
            let error_node = match local_prop_declaration.or(local_index_declaration) {
                Some(node) => Some(node),
                None => match interface_declaration {
                    Some(interface_declaration) => {
                        let bases = self.get_base_types(ty)?;
                        let mut base_covers = false;
                        for base in bases {
                            let base_prop = self.get_property_of_object_type(
                                base,
                                &self.binder.symbol(prop).escaped_name.clone(),
                            )?;
                            let base_index = self.get_index_type_of_type(base, info.key_type)?;
                            if base_prop.is_some() && base_index.is_some() {
                                base_covers = true;
                                break;
                            }
                        }
                        if base_covers {
                            None
                        } else {
                            Some(interface_declaration)
                        }
                    }
                    None => None,
                },
            };
            if let Some(error_node) = error_node {
                if !self.is_type_assignable_to(prop_type, info.value_type)? {
                    let prop_display = self.symbol_display_name(prop);
                    let prop_type_display = self.type_to_string_slice(prop_type)?;
                    let key_display = self.type_to_string_slice(info.key_type)?;
                    let value_display = self.type_to_string_slice(info.value_type)?;
                    let related = prop_declaration
                        .filter(|&prop_declaration| error_node != prop_declaration)
                        .map(|prop_declaration| {
                            self.related_info_for_node(
                                prop_declaration,
                                &diagnostics::_0_is_declared_here,
                                &[&prop_display],
                            )
                        })
                        .into_iter()
                        .collect();
                    self.error_at_with_related(
                        Some(error_node),
                        &diagnostics::Property_0_of_type_1_is_not_assignable_to_2_index_type_3,
                        &[&prop_display, &prop_type_display, &key_display, &value_display],
                        related,
                    );
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkIndexConstraintForIndexSignature @6.0.3
    /// tsc-hash: 732c15de3351b11fa397e845ff8ab101beb1383ff2dae671eeeab8859e5403b1
    /// tsc-span: _tsc.js:84757-84770
    fn check_index_constraint_for_index_signature(
        &mut self,
        ty: TypeId,
        check_info: &IndexInfo,
    ) -> CheckResult2<()> {
        let declaration = check_info.declaration;
        let index_infos = self.get_applicable_index_infos(ty, check_info.key_type)?;
        let interface_declaration = if self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::INTERFACE)
        {
            self.tables.type_of(ty).symbol.and_then(|symbol| {
                self.get_declaration_of_kind(symbol, SyntaxKind::InterfaceDeclaration)
            })
        } else {
            None
        };
        let local_check_declaration = declaration.filter(|&declaration| {
            self.get_symbol_of_declaration(declaration)
                .ok()
                .and_then(|symbol| self.get_parent_of_symbol(symbol))
                == self.tables.type_of(ty).symbol
                && self.tables.type_of(ty).symbol.is_some()
        });
        for info in index_infos {
            // Same-key identity: (key, value, readonly, declaration)
            // — IndexInfo is re-materialized per call, so compare the
            // payload like tsc compares the object.
            if info.key_type == check_info.key_type
                && info.value_type == check_info.value_type
                && info.declaration == check_info.declaration
            {
                continue;
            }
            let local_index_declaration = info.declaration.filter(|&index_declaration| {
                self.get_symbol_of_declaration(index_declaration)
                    .ok()
                    .and_then(|symbol| self.get_parent_of_symbol(symbol))
                    == self.tables.type_of(ty).symbol
                    && self.tables.type_of(ty).symbol.is_some()
            });
            let error_node = match local_check_declaration.or(local_index_declaration) {
                Some(node) => Some(node),
                None => match interface_declaration {
                    Some(interface_declaration) => {
                        let bases = self.get_base_types(ty)?;
                        let mut base_covers = false;
                        for base in bases {
                            let base_check =
                                self.get_index_info_of_type(base, check_info.key_type)?;
                            let base_index = self.get_index_type_of_type(base, info.key_type)?;
                            if base_check.is_some() && base_index.is_some() {
                                base_covers = true;
                                break;
                            }
                        }
                        if base_covers {
                            None
                        } else {
                            Some(interface_declaration)
                        }
                    }
                    None => None,
                },
            };
            if let Some(error_node) = error_node {
                if !self.is_type_assignable_to(check_info.value_type, info.value_type)? {
                    let check_key_display = self.type_to_string_slice(check_info.key_type)?;
                    let check_value_display = self.type_to_string_slice(check_info.value_type)?;
                    let key_display = self.type_to_string_slice(info.key_type)?;
                    let value_display = self.type_to_string_slice(info.value_type)?;
                    self.error_at(
                        Some(error_node),
                        &diagnostics::_0_index_type_1_is_not_assignable_to_2_index_type_3,
                        &[
                            &check_key_display,
                            &check_value_display,
                            &key_display,
                            &value_display,
                        ],
                    );
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkTypeForDuplicateIndexSignatures @6.0.3
    /// tsc-hash: 444d7a4d31372d50bfecb781e5ccfdcf6d7f5d68c92d0dc606d0c4e06d0fdca2
    /// tsc-span: _tsc.js:81475-81507
    pub(crate) fn check_type_for_duplicate_index_signatures(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        if self.kind_of(node) == SyntaxKind::InterfaceDeclaration {
            let node_symbol = self.get_symbol_of_declaration(node)?;
            let declarations = &self.binder.symbol(node_symbol).declarations;
            if declarations.first().copied() != Some(node) {
                return Ok(());
            }
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        let index_symbol = self
            .get_members_of_symbol(symbol)?
            .get(InternalSymbolName::INDEX)
            .copied();
        let Some(index_symbol) = index_symbol else {
            return Ok(());
        };
        let declarations = self.binder.symbol(index_symbol).declarations.clone();
        // entry identity is the TYPE id (getTypeId keying); insertion
        // order preserved for the per-entry declaration lists.
        let mut map: indexmap::IndexMap<TypeId, (TypeId, Vec<NodeId>)> = indexmap::IndexMap::new();
        for declaration in declarations {
            if self.kind_of(declaration) != SyntaxKind::IndexSignature {
                continue;
            }
            let parameters = match self.data_of(declaration) {
                NodeData::IndexSignature(data) => self.nodes_of(data.parameters),
                _ => Vec::new(),
            };
            if parameters.len() != 1 {
                continue;
            }
            let Some(parameter_type_node) = self.type_annotation_of(parameters[0]) else {
                continue;
            };
            let key_type = self.get_type_from_type_node(parameter_type_node)?;
            for constituent in self.union_members_or_self(key_type) {
                map.entry(constituent)
                    .or_insert_with(|| (constituent, Vec::new()))
                    .1
                    .push(declaration);
            }
        }
        for (_, (entry_type, entry_declarations)) in map {
            if entry_declarations.len() > 1 {
                let display = self.type_to_string_slice(entry_type)?;
                for declaration in entry_declarations {
                    self.error_at(
                        Some(declaration),
                        &diagnostics::Duplicate_index_signature_for_type_0,
                        &[&display],
                    );
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkObjectTypeForDuplicateDeclarations @6.0.3
    /// tsc-hash: ba5ee7b242949d34efce258e3c7127820130f32f9b8b7e7d594ff5ea22b96375
    /// tsc-span: _tsc.js:81449-81474
    ///
    /// BOTH spans report: the first declaration's name (through the
    /// member symbol's valueDeclaration) and the duplicate's own name.
    pub(crate) fn check_object_type_for_duplicate_declarations(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        let members = match self.data_of(node) {
            NodeData::TypeLiteral(data) => data.members,
            NodeData::InterfaceDeclaration(data) => data.members,
            _ => None,
        };
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for member in self.nodes_of(members) {
            if self.kind_of(member) != SyntaxKind::PropertySignature {
                continue;
            }
            let Some(name) = self.name_of_node(member) else {
                continue;
            };
            let member_name = match self.data_of(name) {
                NodeData::StringLiteral(data) => data.text.clone(),
                NodeData::NumericLiteral(data) => data.text.clone(),
                NodeData::Identifier(data) => {
                    tsrs2_binder::unescape_leading_underscores(&data.escaped_text).to_owned()
                }
                _ => continue,
            };
            if names.contains(&member_name) {
                let value_declaration = self
                    .get_symbol_of_declaration(member)
                    .ok()
                    .and_then(|symbol| self.binder.symbol(symbol).value_declaration);
                let first_name =
                    value_declaration.and_then(|declaration| self.name_of_node(declaration));
                self.error_at(
                    first_name,
                    &diagnostics::Duplicate_identifier_0,
                    &[&member_name],
                );
                self.error_at(Some(name), &diagnostics::Duplicate_identifier_0, &[&member_name]);
            } else {
                names.insert(member_name);
            }
        }
        Ok(())
    }
}

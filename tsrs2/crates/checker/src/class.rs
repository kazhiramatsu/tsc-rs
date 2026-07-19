//! M4 class band (§6) — seeded at 5.8a with the index-constraint and
//! duplicate-member workers that checkTypeLiteral's lazy block pulls
//! forward (m4-58 §11); checkClassLikeDeclaration and the member
//! override bands land at 5.8c.

use tsrs2_binder::SymbolId;
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    InternalSymbolName, ModifierFlags, NodeFlags, ObjectFlags, SymbolFlags, TypeFlags, TypeId,
};

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
                        let name_expression = self.name_of_node(member).and_then(|name| match self
                            .data_of(name)
                        {
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
                        &[
                            &prop_display,
                            &prop_type_display,
                            &key_display,
                            &value_display,
                        ],
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
                self.error_at(
                    Some(name),
                    &diagnostics::Duplicate_identifier_0,
                    &[&member_name],
                );
            } else {
                names.insert(member_name);
            }
        }
        Ok(())
    }

    // ---- §6 drivers (5.8c) ----

    /// tsc-port: checkClassDeclaration @6.0.3
    /// tsc-hash: 3b07c1829619db8554a666700209aa994ea32f0c7371e513ab4e6005bfaa7e88
    /// tsc-span: _tsc.js:84982-84993
    ///
    /// The static-private grammar row is LIVE only under
    /// experimental_decorators=true (§10 dual mode: legacyDecorators ==
    /// the option); registerForUnusedIdentifiersCheck is inert until
    /// M7.
    pub(crate) fn check_class_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::ClassDeclaration(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (modifiers, name, members) = (data.modifiers, data.name, data.members);
        if self.options.experimental_decorators {
            let first_decorator = self
                .nodes_of(modifiers)
                .into_iter()
                .find(|&modifier| self.kind_of(modifier) == SyntaxKind::Decorator);
            if let Some(first_decorator) = first_decorator {
                let has_static_private_element = self.nodes_of(members).iter().any(|&member| {
                    self.is_static_element(member)
                        && self
                            .name_of_node(member)
                            .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
                });
                if has_static_private_element {
                    self.grammar_error_on_node(
                        first_decorator,
                        &diagnostics::Class_decorators_can_t_be_used_with_static_private_identifier_Consider_removing_the_experimental_decorator,
                        &[],
                    );
                }
            }
        }
        if name.is_none()
            && !tsrs2_binder::node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::DEFAULT,
            )
        {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::A_class_declaration_without_the_default_modifier_must_have_a_name,
                &[],
            );
        }
        self.check_class_like_declaration(node)?;
        for member in self.nodes_of(members) {
            self.check_source_element(Some(member));
        }
        Ok(())
    }

    /// tsc-port: checkClassExpression @6.0.3
    /// tsc-hash: a02caab9f1df7c6c3d4005cd5a084050bf147ce03943cc383d0c007ecf59827b
    /// tsc-span: _tsc.js:84972-84977
    ///
    /// checkClassExpressionExternalHelpers is an emit-helper no-op.
    pub(crate) fn check_class_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_class_like_declaration(node)?;
        self.check_node_deferred(node);
        let symbol = self.get_symbol_of_declaration(node)?;
        self.get_type_of_symbol(symbol)
    }

    /// tsc-port: checkClassExpressionDeferred @6.0.3
    /// tsc-hash: a08eeee9ff34a3e1ebf619a911642f1265f37dfcb5e7523fdd112d4d2f4794d7
    /// tsc-span: _tsc.js:84978-84981
    pub(crate) fn check_class_expression_deferred(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::ClassExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        for member in self.nodes_of(data.members) {
            self.check_source_element(Some(member));
        }
        Ok(())
    }

    /// tsc-port: checkClassLikeDeclaration @6.0.3
    /// tsc-hash: c6d19b4f07ff5e371ca23c7525da95a00bbd334e745ba9ae039197957eeb7330
    /// tsc-span: _tsc.js:84994-85111
    ///
    /// Order is the spec (m4-58 §6). addLazyDiagnostic = eager
    /// identity for the base-type block, the implements diagnostics,
    /// and the final index-constraint/property-initialization block.
    /// Elisions: the ES5 Extends emit-helper probe (no-op), the
    /// getClassExtendsHeritageElement ≠ getEffectiveBaseTypeNode
    /// divergence (JS @augments only), and isJSConstructor (constant
    /// false in TS files).
    fn check_class_like_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_class_like_declaration(node);
        self.check_decorators(node)?;
        let (name, type_parameters, _members) = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => (data.name, data.type_parameters, data.members),
            NodeData::ClassExpression(data) => (data.name, data.type_parameters, data.members),
            _ => unreachable!("class-like kinds route here"),
        };
        self.check_collisions_for_declaration_name(node, name);
        let type_parameter_nodes = self.nodes_of(type_parameters);
        self.check_type_parameters(&type_parameter_nodes)?;
        self.check_exports_on_merged_declarations(node)?;
        let symbol = self.get_symbol_of_declaration(node)?;
        let ty = self.get_declared_type_of_symbol_slice(symbol)?;
        let type_with_this = self.get_type_with_this_argument(ty, None, false)?;
        let static_type = self.get_type_of_symbol(symbol)?;
        self.check_type_parameter_lists_identical(symbol)?;
        self.check_function_or_constructor_symbol(symbol)?;
        self.check_class_for_duplicate_declarations(node)?;
        // The parser does not stamp NodeFlags::AMBIENT from a `declare`
        // modifier — read the modifier alongside (5.8b precedent).
        let node_in_ambient_context = self.node_flags(node) & NodeFlags::AMBIENT.bits() != 0
            || tsrs2_binder::node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::AMBIENT,
            );
        if !node_in_ambient_context {
            self.check_class_for_static_property_name_conflicts(node)?;
        }
        if let Some(base_type_node) = self.get_class_extends_heritage_element(node) {
            let NodeData::ExpressionWithTypeArguments(base_data) = self.data_of(base_type_node)
            else {
                unreachable!("extends heritage elements are ExpressionWithTypeArguments");
            };
            let (base_expression, base_type_arguments) =
                (base_data.expression, base_data.type_arguments);
            for argument in self.nodes_of(base_type_arguments) {
                self.check_source_element(Some(argument));
            }
            let base_types = self.get_base_types(ty)?;
            if !base_types.is_empty() {
                let base_type = base_types[0];
                let base_constructor_type = self.get_base_constructor_type_of_class(ty)?;
                let static_base_type = self.get_apparent_type(base_constructor_type)?;
                self.check_base_type_accessibility(static_base_type, base_type_node)?;
                self.check_source_element(base_expression);
                if !self.nodes_of(base_type_arguments).is_empty() {
                    for argument in self.nodes_of(base_type_arguments) {
                        self.check_source_element(Some(argument));
                    }
                    for constructor in
                        self.get_constructors_for_type_arguments(static_base_type, base_type_node)?
                    {
                        let constructor_type_parameters =
                            self.signature_of(constructor).type_parameters.clone();
                        if !self.check_type_argument_constraints(
                            base_type_node,
                            constructor_type_parameters.as_deref().unwrap_or(&[]),
                        )? {
                            break;
                        }
                    }
                }
                let this_type = self.this_type_of_class_or_interface(ty);
                let base_with_this =
                    self.get_type_with_this_argument(base_type, this_type, false)?;
                let instance_side_assignable = self.check_type_assignable_to(
                    type_with_this,
                    base_with_this,
                    /*error_node*/ None,
                    &diagnostics::Class_0_incorrectly_extends_base_class_1,
                )?;
                if !instance_side_assignable {
                    self.issue_member_specific_error(
                        node,
                        type_with_this,
                        base_with_this,
                        &diagnostics::Class_0_incorrectly_extends_base_class_1,
                    )?;
                } else {
                    let static_base_without_signatures =
                        self.get_type_without_signatures(static_base_type)?;
                    self.check_type_assignable_to(
                        static_type,
                        static_base_without_signatures,
                        name.or(Some(node)),
                        &diagnostics::Class_static_side_0_incorrectly_extends_base_class_static_side_1,
                    )?;
                }
                if self
                    .tables
                    .flags_of(base_constructor_type)
                    .intersects(TypeFlags::TYPE_VARIABLE)
                {
                    if !self.is_mixin_constructor_type(static_type)? {
                        self.error_at(
                            name.or(Some(node)),
                            &diagnostics::A_mixin_class_must_have_a_constructor_with_a_single_rest_parameter_of_type_any,
                            &[],
                        );
                    } else {
                        let construct_signatures = self.get_signatures_of_type(
                            base_constructor_type,
                            crate::structural::SignatureKind::Construct,
                        )?;
                        let has_abstract_signature = construct_signatures.iter().any(|&sig| {
                            self.signature_of(sig)
                                .flags
                                .intersects(tsrs2_types::SignatureFlags::ABSTRACT)
                        });
                        if has_abstract_signature
                            && !tsrs2_binder::node_util::has_syntactic_modifier(
                                self.binder.source_of_node(node),
                                node,
                                ModifierFlags::ABSTRACT,
                            )
                        {
                            self.error_at(
                                name.or(Some(node)),
                                &diagnostics::A_mixin_class_that_extends_from_a_type_variable_containing_an_abstract_construct_signature_must_also_be_declared_abstract,
                                &[],
                            );
                        }
                    }
                }
                let static_base_is_class = self
                    .tables
                    .type_of(static_base_type)
                    .symbol
                    .is_some_and(|base_symbol| {
                        self.binder
                            .symbol(base_symbol)
                            .flags
                            .intersects(SymbolFlags::CLASS)
                    });
                if !static_base_is_class
                    && !self
                        .tables
                        .flags_of(base_constructor_type)
                        .intersects(TypeFlags::TYPE_VARIABLE)
                {
                    let constructors = self.get_instantiated_constructors_for_type_arguments(
                        static_base_type,
                        base_type_node,
                    )?;
                    let mut return_type_mismatch = false;
                    for signature in constructors {
                        let return_type = self.get_return_type_of_signature(signature)?;
                        if !self.is_type_identical_to(return_type, base_type)? {
                            return_type_mismatch = true;
                            break;
                        }
                    }
                    if return_type_mismatch {
                        self.error_at(
                            base_expression,
                            &diagnostics::Base_constructors_must_all_have_the_same_return_type,
                            &[],
                        );
                    }
                }
                self.check_kinds_of_property_member_overrides(ty, base_type)?;
            }
        }
        self.check_members_for_override_modifier(node, ty, type_with_this, static_type)?;
        for type_ref_node in self.get_effective_implements_type_nodes(node) {
            let NodeData::ExpressionWithTypeArguments(ref_data) = self.data_of(type_ref_node)
            else {
                unreachable!("implements heritage elements are ExpressionWithTypeArguments");
            };
            let ref_expression = ref_data.expression.ok_or_else(|| {
                crate::state::Unsupported::new(
                    "implements heritage element without an expression (parse recovery)",
                )
            })?;
            let expression_is_entity = {
                let source = self.binder.source_of_node(ref_expression);
                tsrs2_binder::node_util::is_entity_name_expression(source, ref_expression)
                    && !tsrs2_binder::node_util::is_optional_chain(source, ref_expression)
            };
            if !expression_is_entity {
                self.error_at(
                    Some(ref_expression),
                    &diagnostics::A_class_can_only_implement_an_identifier_qualified_name_with_optional_type_arguments,
                    &[],
                );
            }
            self.check_type_reference_node(type_ref_node)?;
            self.check_implements_diagnostics(node, type_ref_node, ty, type_with_this)?;
        }
        self.check_index_constraints(ty, symbol, /*is_static_index*/ false)?;
        self.check_index_constraints(static_type, symbol, /*is_static_index*/ true)?;
        self.check_type_for_duplicate_index_signatures(node)?;
        self.check_property_initialization(node)?;
        Ok(())
    }

    /// createImplementsDiagnostics (85090-85110, the lazy closure of
    /// checkClassLikeDeclaration) — hash/span carried by the owner.
    fn check_implements_diagnostics(
        &mut self,
        node: NodeId,
        type_ref_node: NodeId,
        ty: TypeId,
        type_with_this: TypeId,
    ) -> CheckResult2<()> {
        let node_type = self.get_type_from_type_node(type_ref_node)?;
        let t = self.get_reduced_type(node_type)?;
        if t == self.tables.intrinsics.error {
            return Ok(());
        }
        if self.is_valid_base_type(t)? {
            let t_is_class = self.tables.type_of(t).symbol.is_some_and(|t_symbol| {
                self.binder
                    .symbol(t_symbol)
                    .flags
                    .intersects(SymbolFlags::CLASS)
            });
            let generic_diag: &'static tsrs2_diags::DiagnosticMessage = if t_is_class {
                &diagnostics::Class_0_incorrectly_implements_class_1_Did_you_mean_to_extend_1_and_inherit_its_members_as_a_subclass
            } else {
                &diagnostics::Class_0_incorrectly_implements_interface_1
            };
            let this_type = self.this_type_of_class_or_interface(ty);
            let base_with_this = self.get_type_with_this_argument(t, this_type, false)?;
            let assignable = self.check_type_assignable_to(
                type_with_this,
                base_with_this,
                /*error_node*/ None,
                generic_diag,
            )?;
            if !assignable {
                self.issue_member_specific_error(
                    node,
                    type_with_this,
                    base_with_this,
                    generic_diag,
                )?;
            }
        } else {
            self.error_at(
                Some(type_ref_node),
                &diagnostics::A_class_can_only_implement_an_object_type_or_intersection_of_object_types_with_statically_known_members,
                &[],
            );
        }
        Ok(())
    }

    /// tsc getEffectiveImplementsTypeNodes reduced to TS files: the
    /// FIRST implements clause's type list (getHeritageClause returns
    /// the first matching clause — a recovery tree's second implements
    /// clause never resolves; parserClassDeclaration2 pins the 2304
    /// silence). The JS @implements arm is dead.
    fn get_effective_implements_type_nodes(&self, node: NodeId) -> Vec<NodeId> {
        let heritage = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => None,
        };
        for clause in self.nodes_of(heritage) {
            if !self.heritage_clause_is_extends(clause) {
                if let NodeData::HeritageClause(data) = self.data_of(clause) {
                    return self.nodes_of(data.types);
                }
            }
        }
        Vec::new()
    }

    /// tsrs-native: tsc reads `type.thisType` off the InterfaceType
    /// object directly — this accessor unpacks the GenericType data
    /// twin (None for this-less interface Object data).
    pub(crate) fn this_type_of_class_or_interface(&self, ty: TypeId) -> Option<TypeId> {
        match &self.tables.type_of(ty).data {
            tsrs2_types::TypeData::GenericType { this_type, .. } => Some(*this_type),
            _ => None,
        }
    }

    // ---- §6 grammar workers (5.8c; m4-58 §12 checklist) ----

    /// tsc-port: checkGrammarClassLikeDeclaration @6.0.3
    /// tsc-hash: 4ed1934b252ad9f0205a4ae8d5316c4d1602af811184091ed9dbc5b8c9877321
    /// tsc-span: _tsc.js:89470-89473
    fn check_grammar_class_like_declaration(&mut self, node: NodeId) -> bool {
        let type_parameters = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.type_parameters,
            NodeData::ClassExpression(data) => data.type_parameters,
            _ => None,
        };
        self.check_grammar_class_declaration_heritage_clauses(node)
            || self.check_grammar_type_parameter_list(node, type_parameters)
    }

    /// tsc-port: checkGrammarClassDeclarationHeritageClauses @6.0.3
    /// tsc-hash: a0759d7c8c63919858e1b9c255d6cc22fe5f49d9c205c5c86812e43655ac0765
    /// tsc-span: _tsc.js:89563-89589
    ///
    /// The modifier gate consults the would-report skeleton
    /// (check_grammar_modifiers_would_report): a modifier grammar
    /// error suppresses the heritage walk in tsc exactly like this
    /// `if` (the modifier row itself stays the M7 FN). NOTE tsc's
    /// suppression covers ONLY the walk — the fn returns undefined
    /// (falsy) either way, so checkGrammarTypeParameterList still
    /// runs after a modifier error.
    fn check_grammar_class_declaration_heritage_clauses(&mut self, node: NodeId) -> bool {
        let mut seen_extends_clause = false;
        let mut seen_implements_clause = false;
        let heritage_clauses = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => None,
        };
        if !self.check_grammar_modifiers_would_report(node) {
            for clause in self.nodes_of(heritage_clauses) {
                let NodeData::HeritageClause(clause_data) = self.data_of(clause) else {
                    continue;
                };
                let types = clause_data.types;
                if self.heritage_clause_is_extends(clause) {
                    if seen_extends_clause {
                        return self.grammar_error_on_first_token(
                            clause,
                            &diagnostics::extends_clause_already_seen,
                            &[],
                        );
                    }
                    if seen_implements_clause {
                        return self.grammar_error_on_first_token(
                            clause,
                            &diagnostics::extends_clause_must_precede_implements_clause,
                            &[],
                        );
                    }
                    let type_nodes = self.nodes_of(types);
                    if type_nodes.len() > 1 {
                        return self.grammar_error_on_first_token(
                            type_nodes[1],
                            &diagnostics::Classes_can_only_extend_a_single_class,
                            &[],
                        );
                    }
                    seen_extends_clause = true;
                } else {
                    if seen_implements_clause {
                        return self.grammar_error_on_first_token(
                            clause,
                            &diagnostics::implements_clause_already_seen,
                            &[],
                        );
                    }
                    seen_implements_clause = true;
                }
                self.check_grammar_heritage_clause(clause);
            }
        }
        false
    }

    /// tsc-port: checkGrammarInterfaceDeclaration @6.0.3
    /// tsc-hash: 38bcdfd34f946b09e0927672db63b8520dd1d65a78d1446182e2f8e1352ab871
    /// tsc-span: _tsc.js:89590-89607
    pub(crate) fn check_grammar_interface_declaration(&mut self, node: NodeId) -> bool {
        let mut seen_extends_clause = false;
        let heritage_clauses = match self.data_of(node) {
            NodeData::InterfaceDeclaration(data) => data.heritage_clauses,
            _ => None,
        };
        for clause in self.nodes_of(heritage_clauses) {
            if self.heritage_clause_is_extends(clause) {
                if seen_extends_clause {
                    return self.grammar_error_on_first_token(
                        clause,
                        &diagnostics::extends_clause_already_seen,
                        &[],
                    );
                }
                seen_extends_clause = true;
            } else {
                return self.grammar_error_on_first_token(
                    clause,
                    &diagnostics::Interface_declaration_cannot_have_implements_clause,
                    &[],
                );
            }
            self.check_grammar_heritage_clause(clause);
        }
        false
    }

    /// tsc-port: checkGrammarHeritageClause @6.0.3
    /// tsc-hash: 47db57db2fd94b03c829f384efc76d2428e9e6af083822e9bf61d5e6f5a61156
    /// tsc-span: _tsc.js:89546-89556
    fn check_grammar_heritage_clause(&mut self, node: NodeId) -> bool {
        let NodeData::HeritageClause(data) = self.data_of(node) else {
            return false;
        };
        let types = data.types;
        if self.check_grammar_for_disallowed_trailing_comma(
            types,
            &diagnostics::Trailing_comma_not_allowed,
        ) {
            return true;
        }
        let type_nodes = self.nodes_of(types);
        if let Some(types) = types {
            if type_nodes.is_empty() {
                let list_type = if self.heritage_clause_is_extends(node) {
                    "extends"
                } else {
                    "implements"
                };
                let pos = {
                    let source = self.binder.source_of_node(node);
                    let byte_pos = source.arena.node_array(types).pos;
                    source
                        .line_map
                        .byte_to_utf16
                        .get(byte_pos as usize)
                        .copied()
                        .unwrap_or(byte_pos)
                };
                return self.grammar_error_at_pos(
                    node,
                    pos,
                    0,
                    &diagnostics::_0_list_cannot_be_empty,
                    &[list_type],
                );
            }
        }
        type_nodes
            .into_iter()
            .any(|type_node| self.check_grammar_expression_with_type_arguments(type_node))
    }

    // ---- §6 symbol-shape checks (5.8c) ----

    /// tsc-port: checkTypeParameterListsIdentical @6.0.3
    /// tsc-hash: ec1052e02b1bc50c5225c5fa1ec82402a19098cbe912a0905ceefb453c6a24c5
    /// tsc-span: _tsc.js:84871-84890
    pub(crate) fn check_type_parameter_lists_identical(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<()> {
        if self.binder.symbol(symbol).declarations.len() == 1 {
            return Ok(());
        }
        if self.links.symbol(symbol).type_parameters_checked {
            return Ok(());
        }
        self.links
            .set_symbol_type_parameters_checked(self.speculation_depth, symbol);
        let declarations = self.get_class_or_interface_declarations_of_symbol(symbol);
        if declarations.len() <= 1 {
            return Ok(());
        }
        let ty = self.get_declared_type_of_symbol_slice(symbol)?;
        let local_type_parameters: Vec<TypeId> = match &self.tables.type_of(ty).data {
            tsrs2_types::TypeData::GenericType {
                type_parameters,
                outer_type_parameter_count,
                ..
            } => type_parameters[*outer_type_parameter_count..].to_vec(),
            _ => Vec::new(),
        };
        if !self.are_type_parameters_identical(&declarations, &local_type_parameters)? {
            let name = self.symbol_display_name(symbol);
            for declaration in declarations {
                let declaration_name = self.name_of_node(declaration);
                self.error_at(
                    declaration_name,
                    &diagnostics::All_declarations_of_0_must_have_identical_type_parameters,
                    &[&name],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: areTypeParametersIdentical @6.0.3
    /// tsc-hash: d32883771008135db0659e692753206de6c7d8d54765f6ec01fc2defb07f0c8e
    /// tsc-span: _tsc.js:84891-84920
    ///
    /// getTypeParameterDeclarations reduces to node.typeParameters in
    /// TS files (the JSDoc template arm is dead).
    pub(crate) fn are_type_parameters_identical(
        &mut self,
        declarations: &[NodeId],
        target_parameters: &[TypeId],
    ) -> CheckResult2<bool> {
        let max_type_argument_count = target_parameters.len();
        let min_type_argument_count = self.get_min_type_argument_count(Some(target_parameters));
        for &declaration in declarations {
            let source_parameters = match self.data_of(declaration) {
                NodeData::ClassDeclaration(data) => self.nodes_of(data.type_parameters),
                NodeData::InterfaceDeclaration(data) => self.nodes_of(data.type_parameters),
                // checkInferType passes the TypeParameter declarations
                // themselves (getTypeParameterDeclarations = decl =>
                // [decl], 81969).
                NodeData::TypeParameter(_) => vec![declaration],
                _ => Vec::new(),
            };
            let num_type_parameters = source_parameters.len();
            if num_type_parameters < min_type_argument_count
                || num_type_parameters > max_type_argument_count
            {
                return Ok(false);
            }
            for (i, &source) in source_parameters.iter().enumerate() {
                let target = target_parameters[i];
                let NodeData::TypeParameter(source_data) = self.data_of(source) else {
                    continue;
                };
                let (source_name, source_constraint_node, source_default_node) = (
                    source_data.name,
                    source_data.constraint,
                    source_data.default,
                );
                let source_text = source_name.and_then(|name| match self.data_of(name) {
                    NodeData::Identifier(data) => Some(data.escaped_text.clone()),
                    _ => None,
                });
                let target_name =
                    self.tables.type_of(target).symbol.map(|target_symbol| {
                        self.binder.symbol(target_symbol).escaped_name.clone()
                    });
                if source_text != target_name {
                    return Ok(false);
                }
                if let Some(constraint_node) = source_constraint_node {
                    let source_constraint = self.get_type_from_type_node(constraint_node)?;
                    if let Some(target_constraint) =
                        self.get_constraint_of_type_parameter(target)?
                    {
                        if !self.is_type_identical_to(source_constraint, target_constraint)? {
                            return Ok(false);
                        }
                    }
                }
                if let Some(default_node) = source_default_node {
                    let source_default = self.get_type_from_type_node(default_node)?;
                    if let Some(target_default) = self.get_default_from_type_parameter(target)? {
                        if !self.is_type_identical_to(source_default, target_default)? {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    /// tsc-port: getClassOrInterfaceDeclarationsOfSymbol @6.0.3
    /// tsc-hash: d69c3d9b598ae66c91f639f3821842f8a7310274ec8ad4342396d03ca477af78
    /// tsc-span: _tsc.js:85312-85314
    fn get_class_or_interface_declarations_of_symbol(&self, symbol: SymbolId) -> Vec<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .filter(|&declaration| {
                matches!(
                    self.kind_of(declaration),
                    SyntaxKind::ClassDeclaration | SyntaxKind::InterfaceDeclaration
                )
            })
            .collect()
    }

    /// tsc-port: checkClassForDuplicateDeclarations @6.0.3
    /// tsc-hash: 203191affdef6230598a72198c7c99cfa972dc0a6be8205f0a1149e2f2c7df70
    /// tsc-span: _tsc.js:81363-81424
    pub(crate) fn check_class_for_duplicate_declarations(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        const GET_ACCESSOR: u32 = 1;
        const SET_ACCESSOR: u32 = 2;
        const GET_OR_SET_ACCESSOR: u32 = GET_ACCESSOR | SET_ACCESSOR;
        const METHOD: u32 = 8;
        const PRIVATE_STATIC: u32 = 16;
        let mut instance_names: std::collections::HashMap<String, u32> = Default::default();
        let mut static_names: std::collections::HashMap<String, u32> = Default::default();
        let mut private_identifiers: std::collections::HashMap<String, u32> = Default::default();
        // addName (81402-81423): the meaning-merge lattice.
        fn add_name(
            state: &mut CheckerState<'_>,
            names: &mut std::collections::HashMap<String, u32>,
            location: NodeId,
            name: String,
            meaning: u32,
        ) -> CheckResult2<()> {
            match names.get(&name).copied() {
                Some(prev) => {
                    if (prev & PRIVATE_STATIC) != (meaning & PRIVATE_STATIC) {
                        let text = state.text_of_node(location)?;
                        state.error_at(
                            Some(location),
                            &diagnostics::Duplicate_identifier_0_Static_and_instance_elements_cannot_share_the_same_private_name,
                            &[&text],
                        );
                    } else {
                        let prev_is_method = prev & METHOD != 0;
                        let is_method = meaning & METHOD != 0;
                        if prev_is_method || is_method {
                            if prev_is_method != is_method {
                                let text = state.text_of_node(location)?;
                                state.error_at(
                                    Some(location),
                                    &diagnostics::Duplicate_identifier_0,
                                    &[&text],
                                );
                            }
                        } else if prev & meaning & !PRIVATE_STATIC != 0 {
                            let text = state.text_of_node(location)?;
                            state.error_at(
                                Some(location),
                                &diagnostics::Duplicate_identifier_0,
                                &[&text],
                            );
                        } else {
                            names.insert(name, prev | meaning);
                        }
                    }
                }
                None => {
                    names.insert(name, meaning);
                }
            }
            Ok(())
        }
        let members = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        for member in self.nodes_of(members) {
            if self.kind_of(member) == SyntaxKind::Constructor {
                let parameters = match self.data_of(member) {
                    NodeData::Constructor(data) => data.parameters,
                    _ => None,
                };
                for param in self.nodes_of(parameters) {
                    if self.is_parameter_property_declaration(param) {
                        let param_name = match self.data_of(param) {
                            NodeData::Parameter(data) => data.name,
                            _ => None,
                        };
                        if let Some(param_name) = param_name {
                            if let NodeData::Identifier(name_data) = self.data_of(param_name) {
                                let text = name_data.escaped_text.clone();
                                add_name(
                                    self,
                                    &mut instance_names,
                                    param_name,
                                    text,
                                    GET_OR_SET_ACCESSOR,
                                )?;
                            }
                        }
                    }
                }
            } else {
                let is_static_member = self.is_static_element(member);
                let Some(name) = self.name_of_node(member) else {
                    continue;
                };
                let is_private = self.kind_of(name) == SyntaxKind::PrivateIdentifier;
                let private_static_flags = if is_private && is_static_member {
                    PRIVATE_STATIC
                } else {
                    0
                };
                let member_name = self.effective_property_name_for_property_name_node(name)?;
                if let Some(member_name) = member_name.filter(|name| !name.is_empty()) {
                    let meaning = match self.kind_of(member) {
                        SyntaxKind::GetAccessor => GET_ACCESSOR,
                        SyntaxKind::SetAccessor => SET_ACCESSOR,
                        SyntaxKind::PropertyDeclaration => GET_OR_SET_ACCESSOR,
                        SyntaxKind::MethodDeclaration => METHOD,
                        _ => continue,
                    };
                    let names = if is_private {
                        &mut private_identifiers
                    } else if is_static_member {
                        &mut static_names
                    } else {
                        &mut instance_names
                    };
                    add_name(
                        self,
                        names,
                        name,
                        member_name,
                        meaning | private_static_flags,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkClassForStaticPropertyNameConflicts @6.0.3
    /// tsc-hash: a14bae24fb447999a090340b85a2ed0d795369282344c797c20100bfa65502a0
    /// tsc-span: _tsc.js:81425-81448
    ///
    /// getNameOfSymbolAsWritten reduces to the unescaped symbol name
    /// for class declarations (the anonymous-class and quoted-name
    /// flavors never reach this row: static members require a named
    /// container binding for the conflict to observably collide).
    pub(crate) fn check_class_for_static_property_name_conflicts(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        let members = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        for member in self.nodes_of(members) {
            let Some(member_name_node) = self.name_of_node(member) else {
                continue;
            };
            if !self.is_static_element(member) {
                continue;
            }
            let member_name =
                self.effective_property_name_for_property_name_node(member_name_node)?;
            let Some(member_name) = member_name else {
                continue;
            };
            let conflicts = match member_name.as_str() {
                // The checker reads the COMPUTED useDefineForClassFields
                // (18251: explicit value, else target >= ES2022) — NOT
                // emitStandardClassFields (staticPropertyNameConflicts
                // pins the es5+useDefineForClassFields=true rows silent).
                "name" | "length" | "caller" | "arguments" => {
                    !self.options.use_define_for_class_fields_effective()
                }
                "prototype" => true,
                _ => false,
            };
            if conflicts {
                let symbol = self.get_symbol_of_declaration(node)?;
                let class_name = self.symbol_display_name(symbol);
                let display_name =
                    tsrs2_binder::unescape_leading_underscores(&member_name).to_owned();
                self.error_at(
                    Some(member_name_node),
                    &diagnostics::Static_property_0_conflicts_with_built_in_property_Function_0_of_constructor_function_1,
                    &[&display_name, &class_name],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: getPropertyNameForPropertyNameNode @6.0.3
    /// tsc-hash: 5770eff9fe2f071f83fce9a7aaff9c54fa6f09141154c33c0f7f3e5dc86ee117
    /// tsc-span: _tsc.js:15861-15887
    ///
    /// The JsxNamespacedName arm is unreachable from property names.
    fn property_name_for_property_name_node(&self, name: NodeId) -> Option<String> {
        let source = self.binder.source_of_node(name);
        match self.kind_of(name) {
            SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral => {
                tsrs2_binder::node_util::get_escaped_text_of_identifier_or_literal(source, name)
            }
            SyntaxKind::ComputedPropertyName => {
                let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
                    return None;
                };
                let expression = data.expression?;
                if tsrs2_binder::node_util::is_string_or_numeric_literal_like(source, expression) {
                    return tsrs2_binder::node_util::get_escaped_text_of_identifier_or_literal(
                        source, expression,
                    );
                }
                if tsrs2_binder::node_util::is_signed_numeric_literal(source, expression) {
                    if let NodeData::PrefixUnaryExpression(unary) = self.data_of(expression) {
                        let operand_text =
                            unary
                                .operand
                                .and_then(|operand| match self.data_of(operand) {
                                    NodeData::NumericLiteral(literal) => Some(literal.text.clone()),
                                    _ => None,
                                })?;
                        return Some(if unary.operator == SyntaxKind::MinusToken {
                            format!("-{operand_text}")
                        } else {
                            operand_text
                        });
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// tsc-port: getEffectivePropertyNameForPropertyNameNode @6.0.3
    /// tsc-hash: 97f6f84e70231a6c4759b95c1d8a145c2b66f88aef026d2d50db8cc0c83d8132
    /// tsc-span: _tsc.js:90537-90540
    fn effective_property_name_for_property_name_node(
        &mut self,
        name: NodeId,
    ) -> CheckResult2<Option<String>> {
        if let Some(text) = self.property_name_for_property_name_node(name) {
            return Ok(Some(text));
        }
        if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
            let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
                return Ok(None);
            };
            let Some(expression) = data.expression else {
                return Ok(None);
            };
            let name_type = self.get_type_of_expression(expression)?;
            return Ok(self.property_name_from_type_usable(name_type));
        }
        Ok(None)
    }

    // ---- §6 override band (5.8c; getTargetSymbol = the structural.rs
    // port) ----

    /// tsc-port: checkMembersForOverrideModifier @6.0.3
    /// tsc-hash: 87e2395b62460f043f14332805c124e80e5202fbb4ba0d7b3fee324854b528d4
    /// tsc-span: _tsc.js:85112-85150
    fn check_members_for_override_modifier(
        &mut self,
        node: NodeId,
        ty: TypeId,
        type_with_this: TypeId,
        static_type: TypeId,
    ) -> CheckResult2<()> {
        let base_type_node = self.get_class_extends_heritage_element(node);
        let base_types = if base_type_node.is_some() {
            self.get_base_types(ty)?
        } else {
            Vec::new()
        };
        let base_with_this = match base_types.first() {
            Some(&first_base) => {
                let this_type = self.this_type_of_class_or_interface(ty);
                Some(self.get_type_with_this_argument(first_base, this_type, false)?)
            }
            None => None,
        };
        let base_static_type = self.get_base_constructor_type_of_class(ty)?;
        let members = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        for member in self.nodes_of(members) {
            if tsrs2_binder::node_util::has_syntactic_modifier(
                self.binder.source_of_node(member),
                member,
                ModifierFlags::AMBIENT,
            ) {
                continue;
            }
            if self.kind_of(member) == SyntaxKind::Constructor {
                let parameters = match self.data_of(member) {
                    NodeData::Constructor(data) => data.parameters,
                    _ => None,
                };
                for param in self.nodes_of(parameters) {
                    if self.is_parameter_property_declaration(param) {
                        self.check_existing_member_for_override_modifier(
                            node,
                            static_type,
                            base_static_type,
                            base_with_this,
                            ty,
                            type_with_this,
                            param,
                        )?;
                    }
                }
            }
            self.check_existing_member_for_override_modifier(
                node,
                static_type,
                base_static_type,
                base_with_this,
                ty,
                type_with_this,
                member,
            )?;
        }
        Ok(())
    }

    /// tsc-port: checkExistingMemberForOverrideModifier @6.0.3
    /// tsc-hash: 319852aa70d4b2205ec5d9477a8a9caa6a441d77fc64edc448f915899f6255d9
    /// tsc-span: _tsc.js:85151-85170
    ///
    /// getSymbolAtLocation(member.name) reduces to the declaration's
    /// binder symbol for class elements; reportErrors is always true
    /// on the checker path (the LSP flavor passes false).
    #[allow(clippy::too_many_arguments)]
    fn check_existing_member_for_override_modifier(
        &mut self,
        node: NodeId,
        static_type: TypeId,
        base_static_type: TypeId,
        base_with_this: Option<TypeId>,
        ty: TypeId,
        type_with_this: TypeId,
        member: NodeId,
    ) -> CheckResult2<()> {
        let Some(declared_prop) = self.node_symbol(member) else {
            return Ok(());
        };
        let source = self.binder.source_of_node(member);
        let member_has_override_modifier = tsrs2_binder::node_util::has_syntactic_modifier(
            source,
            member,
            ModifierFlags::OVERRIDE,
        );
        let member_is_static = self.is_static_element(member);
        self.check_member_for_override_modifier(
            node,
            static_type,
            base_static_type,
            base_with_this,
            ty,
            type_with_this,
            member_has_override_modifier,
            member_is_static,
            declared_prop,
            Some(member),
        )
    }

    /// tsc-port: checkMemberForOverrideModifier @6.0.3
    /// tsc-hash: 787be8e6a710f05ef10453458e6b5eb30814ab2fd17c0300adb573b8b0fe887e
    /// tsc-span: _tsc.js:85171-85232
    ///
    /// isJs is constant false; noImplicitOverride is ABSENT from
    /// CompilerOptions (§13 options audit) — the needs-override faces
    /// (This_member_must_have_an_override_modifier…, the
    /// parameter-property and abstract flavors) are DEAD; only the
    /// override-present faces live, so the memberHasAbstractModifier /
    /// memberIsParameterProperty params reduce away. MemberOverrideStatus
    /// is consumed only by services — elided. typeToString displays
    /// compute at their use sites (T2 curtain).
    #[allow(clippy::too_many_arguments)]
    fn check_member_for_override_modifier(
        &mut self,
        _node: NodeId,
        static_type: TypeId,
        base_static_type: TypeId,
        base_with_this: Option<TypeId>,
        ty: TypeId,
        type_with_this: TypeId,
        member_has_override_modifier: bool,
        member_is_static: bool,
        member: SymbolId,
        error_node: Option<NodeId>,
    ) -> CheckResult2<()> {
        if member_has_override_modifier {
            let value_declaration = self.binder.symbol(member).value_declaration;
            if let Some(value_declaration) = value_declaration {
                let name = self.name_of_node(value_declaration);
                if let Some(name) = name {
                    let source = self.binder.source_of_node(name);
                    let non_bindable_dynamic =
                        tsrs2_binder::node_util::is_dynamic_name(source, name)
                            && !self.has_late_bindable_ast_name(value_declaration);
                    if non_bindable_dynamic {
                        self.error_at(
                            error_node,
                            &diagnostics::This_member_cannot_have_an_override_modifier_because_its_name_is_dynamic,
                            &[],
                        );
                        return Ok(());
                    }
                }
            }
        }
        if let Some(base_with_this) = base_with_this {
            if member_has_override_modifier {
                let this_type = if member_is_static {
                    static_type
                } else {
                    type_with_this
                };
                let base_type = if member_is_static {
                    base_static_type
                } else {
                    base_with_this
                };
                let escaped_name = self.binder.symbol(member).escaped_name.clone();
                let prop = self.get_property_of_type_full(this_type, &escaped_name)?;
                let base_prop = self.get_property_of_type_full(base_type, &escaped_name)?;
                if prop.is_some() && base_prop.is_none() {
                    if let Some(error_node) = error_node {
                        let base_class_name = self.type_to_string_slice(base_with_this)?;
                        let member_name = self.symbol_display_name(member);
                        let suggestion = self.get_suggested_symbol_for_nonexistent_class_member(
                            &member_name,
                            base_type,
                        )?;
                        match suggestion {
                            Some(suggestion) => {
                                let suggestion_name = self.symbol_display_name(suggestion);
                                self.error_at(
                                    Some(error_node),
                                    &diagnostics::This_member_cannot_have_an_override_modifier_because_it_is_not_declared_in_the_base_class_0_Did_you_mean_1,
                                    &[&base_class_name, &suggestion_name],
                                );
                            }
                            None => {
                                self.error_at(
                                    Some(error_node),
                                    &diagnostics::This_member_cannot_have_an_override_modifier_because_it_is_not_declared_in_the_base_class_0,
                                    &[&base_class_name],
                                );
                            }
                        }
                    }
                    return Ok(());
                }
                // prop && baseProp && noImplicitOverride — the entire
                // needs-override arm is dead (option absent).
            }
        } else if member_has_override_modifier {
            if let Some(error_node) = error_node {
                let class_name = self.type_to_string_slice(ty)?;
                self.error_at(
                    Some(error_node),
                    &diagnostics::This_member_cannot_have_an_override_modifier_because_its_containing_class_0_does_not_extend_another_class,
                    &[&class_name],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: issueMemberSpecificError @6.0.3
    /// tsc-hash: 165c108816f966f5ab0028ab4f138fd2332ca10aac1f3192c2a407959b352a76
    /// tsc-span: _tsc.js:85233-85268
    ///
    /// The member row's CHAIN ROOT is the reported code/message
    /// (Property_0_in_type_1…, the elaboration tail elides — T2); the
    /// broad row falls through to check_type_assignable_to's head
    /// reporting.
    fn issue_member_specific_error(
        &mut self,
        node: NodeId,
        type_with_this: TypeId,
        base_with_this: TypeId,
        broad_diag: &'static tsrs2_diags::DiagnosticMessage,
    ) -> CheckResult2<()> {
        let mut issued_member_error = false;
        let (name, members) = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => (data.name, data.members),
            NodeData::ClassExpression(data) => (data.name, data.members),
            _ => (None, None),
        };
        for member in self.nodes_of(members) {
            if self.is_static_element(member) {
                continue;
            }
            let Some(declared_prop) = self.node_symbol(member) else {
                continue;
            };
            // tsc reads getSymbolAtLocation(member.name), which routes
            // computed names through getLateBoundSymbol — the member
            // table carries `__@toPrimitive@…`, never `__computed`
            // (symbolProperty24 pins the 2416 member row over the
            // 2420 head).
            let declared_prop = self.get_late_bound_symbol(declared_prop)?;
            let escaped_name = self.binder.symbol(declared_prop).escaped_name.clone();
            let prop = self.get_property_of_type_full(type_with_this, &escaped_name)?;
            let base_prop = self.get_property_of_type_full(base_with_this, &escaped_name)?;
            if let (Some(prop), Some(base_prop)) = (prop, base_prop) {
                let prop_type = self.get_type_of_symbol(prop)?;
                let base_prop_type = self.get_type_of_symbol(base_prop)?;
                if !self.is_type_assignable_to(prop_type, base_prop_type)? {
                    let error_node = self.name_of_node(member).or(Some(member));
                    let prop_name = self.symbol_display_name(declared_prop);
                    let type_text = self.type_to_string_slice(type_with_this)?;
                    let base_text = self.type_to_string_slice(base_with_this)?;
                    self.error_at(
                        error_node,
                        &diagnostics::Property_0_in_type_1_is_not_assignable_to_the_same_property_in_base_type_2,
                        &[&prop_name, &type_text, &base_text],
                    );
                    issued_member_error = true;
                }
            }
        }
        if !issued_member_error {
            self.check_type_assignable_to(
                type_with_this,
                base_with_this,
                name.or(Some(node)),
                broad_diag,
            )?;
        }
        Ok(())
    }

    /// tsc-port: checkBaseTypeAccessibility @6.0.3
    /// tsc-hash: 2ebb3395a2644f70d2a0c79e466680ef29c26cb0c1b4e7da8980121328357972
    /// tsc-span: _tsc.js:85269-85280
    ///
    /// getFullyQualifiedName reduces to the unescaped symbol name for
    /// parentless symbols; qualified flavors escape (display band).
    fn check_base_type_accessibility(&mut self, ty: TypeId, node: NodeId) -> CheckResult2<()> {
        let signatures =
            self.get_signatures_of_type(ty, crate::structural::SignatureKind::Construct)?;
        let Some(&first) = signatures.first() else {
            return Ok(());
        };
        let Some(declaration) = self.signature_of(first).declaration else {
            return Ok(());
        };
        if !tsrs2_binder::node_util::has_syntactic_modifier(
            self.binder.source_of_node(declaration),
            declaration,
            ModifierFlags::PRIVATE,
        ) {
            return Ok(());
        }
        let Some(type_symbol) = self.tables.type_of(ty).symbol else {
            return Ok(());
        };
        let type_class_declaration = self.get_class_like_declaration_of_symbol(type_symbol);
        let within = self.is_node_within_class(node, type_class_declaration);
        if !within {
            let name = self.fully_qualified_name_slice(type_symbol)?;
            self.error_at(
                Some(node),
                &diagnostics::Cannot_extend_a_class_0_Class_constructor_is_marked_as_private,
                &[&name],
            );
        }
        Ok(())
    }

    /// tsc getFullyQualifiedName (50040) sliced to the parentless
    /// case: a symbol with a container renders `A.B` through the
    /// nodeBuilder — escape (T2 display band).
    fn fully_qualified_name_slice(&self, symbol: SymbolId) -> CheckResult2<String> {
        if self.binder.symbol(symbol).parent.is_some() {
            return Err(crate::state::Unsupported::new(
                "getFullyQualifiedName over a contained symbol (nodeBuilder display, T2)",
            ));
        }
        Ok(self.symbol_display_name(symbol))
    }

    /// tsc-port: checkKindsOfPropertyMemberOverrides @6.0.3
    /// tsc-hash: dab2a43d0101b31a837848ef9d50fd86a7b68937a5565b7be1e71ba5ea66f44d
    /// tsc-span: _tsc.js:85315-85416
    ///
    /// The derived-is-binary-expression skip is JS-only (dead in TS
    /// files but ported). The useDefineForClassFields overwrite row
    /// reports all five faces: exclamationToken / no constructor /
    /// non-identifier name / !strictNullChecks / not initialized in
    /// the constructor (the flow probe, 85370).
    fn check_kinds_of_property_member_overrides(
        &mut self,
        ty: TypeId,
        base_type: TypeId,
    ) -> CheckResult2<()> {
        struct NotImplementedInfo {
            base_type_name: String,
            type_name: String,
            missed_properties: Vec<String>,
        }
        let base_properties = self.get_properties_of_type(base_type)?;
        let mut not_implemented_info: Vec<(Option<NodeId>, NotImplementedInfo)> = Vec::new();
        'base_property_check: for base_property in base_properties {
            let base = self.get_target_symbol(base_property);
            if self
                .binder
                .symbol(base)
                .flags
                .intersects(SymbolFlags::PROTOTYPE)
            {
                continue;
            }
            let base_escaped_name = self.binder.symbol(base).escaped_name.clone();
            let Some(base_symbol) = self.get_property_of_object_type(ty, &base_escaped_name)?
            else {
                continue;
            };
            let derived = self.get_target_symbol(base_symbol);
            let base_declaration_flags = self.get_declaration_modifier_flags_from_symbol(base);
            if derived == base {
                let derived_class_decl =
                    self.tables.type_of(ty).symbol.and_then(|type_symbol| {
                        self.get_class_like_declaration_of_symbol(type_symbol)
                    });
                let derived_is_abstract = derived_class_decl.is_some_and(|declaration| {
                    tsrs2_binder::node_util::has_syntactic_modifier(
                        self.binder.source_of_node(declaration),
                        declaration,
                        ModifierFlags::ABSTRACT,
                    )
                });
                if base_declaration_flags.intersects(ModifierFlags::ABSTRACT)
                    && !derived_is_abstract
                {
                    for other_base_type in self.get_base_types(ty)? {
                        if other_base_type == base_type {
                            continue;
                        }
                        let base_symbol2 =
                            self.get_property_of_object_type(other_base_type, &base_escaped_name)?;
                        let derived_elsewhere =
                            base_symbol2.map(|symbol| self.get_target_symbol(symbol));
                        if derived_elsewhere.is_some_and(|elsewhere| elsewhere != base) {
                            continue 'base_property_check;
                        }
                    }
                    let base_type_name = self.type_to_string_slice(base_type)?;
                    let type_name = self.type_to_string_slice(ty)?;
                    let base_property_name = self.symbol_display_name(base_property);
                    match not_implemented_info
                        .iter_mut()
                        .find(|(key, _)| *key == derived_class_decl)
                    {
                        Some((_, info)) => info.missed_properties.push(base_property_name),
                        None => not_implemented_info.push((
                            derived_class_decl,
                            NotImplementedInfo {
                                base_type_name,
                                type_name,
                                missed_properties: vec![base_property_name],
                            },
                        )),
                    }
                }
            } else {
                let derived_declaration_flags =
                    self.get_declaration_modifier_flags_from_symbol(derived);
                if base_declaration_flags.intersects(ModifierFlags::PRIVATE)
                    || derived_declaration_flags.intersects(ModifierFlags::PRIVATE)
                {
                    continue;
                }
                let base_flags = self.binder.symbol(base).flags;
                let derived_flags = self.binder.symbol(derived).flags;
                let property_or_accessor = SymbolFlags::from_bits(
                    SymbolFlags::PROPERTY.bits() | SymbolFlags::ACCESSOR.bits(),
                );
                let base_property_flags =
                    SymbolFlags::from_bits(base_flags.bits() & property_or_accessor.bits());
                let derived_property_flags =
                    SymbolFlags::from_bits(derived_flags.bits() & property_or_accessor.bits());
                if !base_property_flags.is_empty() && !derived_property_flags.is_empty() {
                    let base_check_flags = self.links.symbol(base).check_flags;
                    let base_declarations = self.binder.symbol(base).declarations.clone();
                    let abstract_or_interface_everywhere =
                        if base_check_flags.intersects(tsrs2_types::CheckFlags::SYNTHETIC) {
                            base_declarations.iter().any(|&declaration| {
                                self.is_property_abstract_or_interface(
                                    declaration,
                                    base_declaration_flags,
                                )
                            })
                        } else {
                            !base_declarations.is_empty()
                                && base_declarations.iter().all(|&declaration| {
                                    self.is_property_abstract_or_interface(
                                        declaration,
                                        base_declaration_flags,
                                    )
                                })
                        };
                    let derived_value_is_binary = self
                        .binder
                        .symbol(derived)
                        .value_declaration
                        .is_some_and(|declaration| {
                            self.kind_of(declaration) == SyntaxKind::BinaryExpression
                        });
                    if abstract_or_interface_everywhere
                        || base_check_flags.intersects(tsrs2_types::CheckFlags::MAPPED)
                        || derived_value_is_binary
                    {
                        continue;
                    }
                    let overridden_instance_property = base_property_flags != SymbolFlags::PROPERTY
                        && derived_property_flags == SymbolFlags::PROPERTY;
                    let overridden_instance_accessor = base_property_flags == SymbolFlags::PROPERTY
                        && derived_property_flags != SymbolFlags::PROPERTY;
                    if overridden_instance_property || overridden_instance_accessor {
                        let message: &'static tsrs2_diags::DiagnosticMessage =
                            if overridden_instance_property {
                                &diagnostics::_0_is_defined_as_an_accessor_in_class_1_but_is_overridden_here_in_2_as_an_instance_property
                            } else {
                                &diagnostics::_0_is_defined_as_a_property_in_class_1_but_is_overridden_here_in_2_as_an_accessor
                            };
                        let base_name = self.symbol_display_name(base);
                        let base_type_text = self.type_to_string_slice(base_type)?;
                        let type_text = self.type_to_string_slice(ty)?;
                        let error_node = self.derived_error_node(derived);
                        self.error_at(
                            error_node,
                            message,
                            &[&base_name, &base_type_text, &type_text],
                        );
                    } else if self.options.use_define_for_class_fields_effective() {
                        let derived_declarations = self.binder.symbol(derived).declarations.clone();
                        let uninitialized =
                            derived_declarations.iter().copied().find(|&declaration| {
                                matches!(self.data_of(declaration),
                                    NodeData::PropertyDeclaration(data) if data.initializer.is_none())
                            });
                        let any_ambient_declaration =
                            derived_declarations.iter().any(|&declaration| {
                                self.node_flags(declaration) & NodeFlags::AMBIENT.bits() != 0
                                    || tsrs2_binder::node_util::has_syntactic_modifier(
                                        self.binder.source_of_node(declaration),
                                        declaration,
                                        ModifierFlags::AMBIENT,
                                    )
                            });
                        if let Some(uninitialized) = uninitialized {
                            if !derived_flags.intersects(SymbolFlags::TRANSIENT)
                                && !base_declaration_flags.intersects(ModifierFlags::ABSTRACT)
                                && !derived_declaration_flags.intersects(ModifierFlags::ABSTRACT)
                                && !any_ambient_declaration
                            {
                                let constructor = self
                                    .tables
                                    .type_of(ty)
                                    .symbol
                                    .and_then(|type_symbol| {
                                        self.get_class_like_declaration_of_symbol(type_symbol)
                                    })
                                    .and_then(|declaration| {
                                        self.find_constructor_declaration(declaration)
                                    });
                                let (exclamation, prop_name_is_identifier) =
                                    match self.data_of(uninitialized) {
                                        NodeData::PropertyDeclaration(data) => (
                                            data.exclamation_token.is_some(),
                                            data.name.is_some_and(|name| {
                                                self.kind_of(name) == SyntaxKind::Identifier
                                            }),
                                        ),
                                        _ => (false, false),
                                    };
                                let strict_null_checks = self
                                    .options
                                    .strict_option_value(self.options.strict_null_checks);
                                // 85370: the probe's declared type is
                                // the DERIVED CLASS type (`type`), not
                                // the property type — tsc quirk,
                                // preserved.
                                let mut report = exclamation
                                    || constructor.is_none()
                                    || !prop_name_is_identifier
                                    || !strict_null_checks;
                                if !report {
                                    let ctor =
                                        constructor.expect("non-report arm implies constructor");
                                    report = !self.is_property_initialized_in_constructor(
                                        derived, ty, ctor,
                                    )?;
                                }
                                if report {
                                    let base_name = self.symbol_display_name(base);
                                    let base_type_text = self.type_to_string_slice(base_type)?;
                                    let error_node = self.derived_error_node(derived);
                                    self.error_at(
                                        error_node,
                                        &diagnostics::Property_0_will_overwrite_the_base_property_in_1_If_this_is_intentional_add_an_initializer_Otherwise_add_a_declare_modifier_or_remove_the_redundant_declaration,
                                        &[&base_name, &base_type_text],
                                    );
                                }
                            }
                        }
                    }
                    continue;
                }
                let message: &'static tsrs2_diags::DiagnosticMessage = if self
                    .is_prototype_property(base)
                {
                    if self.is_prototype_property(derived)
                        || derived_flags.intersects(SymbolFlags::PROPERTY)
                    {
                        continue;
                    }
                    debug_assert!(
                        derived_flags.intersects(SymbolFlags::ACCESSOR),
                        "non-method, non-property derived member is an accessor"
                    );
                    &diagnostics::Class_0_defines_instance_member_function_1_but_extended_class_2_defines_it_as_instance_member_accessor
                } else if base_flags.intersects(SymbolFlags::ACCESSOR) {
                    &diagnostics::Class_0_defines_instance_member_accessor_1_but_extended_class_2_defines_it_as_instance_member_function
                } else {
                    &diagnostics::Class_0_defines_instance_member_property_1_but_extended_class_2_defines_it_as_instance_member_function
                };
                let base_type_text = self.type_to_string_slice(base_type)?;
                let base_name = self.symbol_display_name(base);
                let type_text = self.type_to_string_slice(ty)?;
                let error_node = self.derived_error_node(derived);
                self.error_at(
                    error_node,
                    message,
                    &[&base_type_text, &base_name, &type_text],
                );
            }
        }
        for (error_node, info) in not_implemented_info {
            let is_class_expression =
                error_node.is_some_and(|node| self.kind_of(node) == SyntaxKind::ClassExpression);
            let missed = &info.missed_properties;
            if missed.len() == 1 {
                if is_class_expression {
                    self.error_at(
                        error_node,
                        &diagnostics::Non_abstract_class_expression_does_not_implement_inherited_abstract_member_0_from_class_1,
                        &[&missed[0], &info.base_type_name],
                    );
                } else {
                    self.error_at(
                        error_node,
                        &diagnostics::Non_abstract_class_0_does_not_implement_inherited_abstract_member_1_from_class_2,
                        &[&info.type_name, &missed[0], &info.base_type_name],
                    );
                }
            } else if missed.len() > 5 {
                let quoted = missed[..4]
                    .iter()
                    .map(|prop| format!("'{prop}'"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let remaining = (missed.len() - 4).to_string();
                if is_class_expression {
                    self.error_at(
                        error_node,
                        &diagnostics::Non_abstract_class_expression_is_missing_implementations_for_the_following_members_of_0_1_and_2_more,
                        &[&info.base_type_name, &quoted, &remaining],
                    );
                } else {
                    self.error_at(
                        error_node,
                        &diagnostics::Non_abstract_class_0_is_missing_implementations_for_the_following_members_of_1_2_and_3_more,
                        &[&info.type_name, &info.base_type_name, &quoted, &remaining],
                    );
                }
            } else {
                let quoted = missed
                    .iter()
                    .map(|prop| format!("'{prop}'"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if is_class_expression {
                    self.error_at(
                        error_node,
                        &diagnostics::Non_abstract_class_expression_is_missing_implementations_for_the_following_members_of_0_1,
                        &[&info.base_type_name, &quoted],
                    );
                } else {
                    self.error_at(
                        error_node,
                        &diagnostics::Non_abstract_class_0_is_missing_implementations_for_the_following_members_of_1_2,
                        &[&info.type_name, &info.base_type_name, &quoted],
                    );
                }
            }
        }
        Ok(())
    }

    /// getNameOfDeclaration(derived.valueDeclaration) ||
    /// derived.valueDeclaration — the derived-member error span.
    fn derived_error_node(&self, derived: SymbolId) -> Option<NodeId> {
        let value_declaration = self.binder.symbol(derived).value_declaration;
        value_declaration
            .and_then(|declaration| self.name_of_node(declaration))
            .or(value_declaration)
    }

    /// tsc-port: isPropertyAbstractOrInterface @6.0.3
    /// tsc-hash: 7d626b7780a88873122215f7c8e3b6cc540f44260b59b5ea844c9d57f53ab3ba
    /// tsc-span: _tsc.js:85417-85419
    fn is_property_abstract_or_interface(
        &self,
        declaration: NodeId,
        base_declaration_flags: ModifierFlags,
    ) -> bool {
        let abstract_without_initializer = base_declaration_flags
            .intersects(ModifierFlags::ABSTRACT)
            && match self.data_of(declaration) {
                NodeData::PropertyDeclaration(data) => data.initializer.is_none(),
                _ => true,
            };
        let interface_parent = self
            .parent_of(declaration)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::InterfaceDeclaration);
        abstract_without_initializer || interface_parent
    }

    /// tsc-port: checkInheritedPropertiesAreIdentical @6.0.3
    /// tsc-hash: eed9fd3f779d9fd05c0ab3b438ba3aab293de774e830b4d91a4f443ee79551b0
    /// tsc-span: _tsc.js:85439-85476
    ///
    /// The diagnostic lands AT THE INTERFACE NAME with the 2320 chain
    /// head; the Named_property detail rides message.next.
    pub(crate) fn check_inherited_properties_are_identical(
        &mut self,
        ty: TypeId,
        type_node: NodeId,
    ) -> CheckResult2<bool> {
        let base_types = self.get_base_types(ty)?;
        if base_types.len() < 2 {
            return Ok(true);
        }
        struct SeenEntry {
            prop: SymbolId,
            containing_type: TypeId,
        }
        let mut seen: std::collections::HashMap<String, SeenEntry> = Default::default();
        let declared_members = self.resolve_declared_members(ty)?;
        for prop in self.members_of(declared_members).properties.clone() {
            let escaped_name = self.binder.symbol(prop).escaped_name.clone();
            seen.insert(
                escaped_name,
                SeenEntry {
                    prop,
                    containing_type: ty,
                },
            );
        }
        let mut ok = true;
        for base in base_types {
            let this_type = self.this_type_of_class_or_interface(ty);
            let base_with_this = self.get_type_with_this_argument(base, this_type, false)?;
            for prop in self.get_properties_of_type(base_with_this)? {
                let escaped_name = self.binder.symbol(prop).escaped_name.clone();
                match seen.get(&escaped_name) {
                    None => {
                        seen.insert(
                            escaped_name,
                            SeenEntry {
                                prop,
                                containing_type: base,
                            },
                        );
                    }
                    Some(existing) => {
                        let is_inherited_property = existing.containing_type != ty;
                        if is_inherited_property {
                            let existing_prop = existing.prop;
                            let existing_containing_type = existing.containing_type;
                            if !self.is_property_identical_to(existing_prop, prop)? {
                                ok = false;
                                let type_name1 =
                                    self.type_to_string_slice(existing_containing_type)?;
                                let type_name2 = self.type_to_string_slice(base)?;
                                let prop_name = self.symbol_display_name(prop);
                                let type_display = self.type_to_string_slice(ty)?;
                                let mut diagnostic = self.create_error(
                                    Some(type_node),
                                    &diagnostics::Interface_0_cannot_simultaneously_extend_types_1_and_2,
                                    &[&type_display, &type_name1, &type_name2],
                                );
                                diagnostic.message.next =
                                    vec![tsrs2_diags::MessageChain::new(
                                        &diagnostics::Named_property_0_of_types_1_and_2_are_not_identical,
                                        &[prop_name, type_name1, type_name2],
                                    )];
                                self.push_error_diagnostic(diagnostic);
                            }
                        }
                    }
                }
            }
        }
        Ok(ok)
    }

    /// tsc-port: checkPropertyInitialization @6.0.3
    /// tsc-hash: ddc03688d84d2f003730db1971a6cd5aa2c31bbab0362f9e58138c771a001ce6
    /// tsc-span: _tsc.js:85477-85498
    pub(crate) fn check_property_initialization(&mut self, node: NodeId) -> CheckResult2<()> {
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let strict_property_initialization = self
            .options
            .strict_option_value(self.options.strict_property_initialization);
        let ambient = self.node_flags(node) & NodeFlags::AMBIENT.bits() != 0
            || tsrs2_binder::node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::AMBIENT,
            );
        if !strict_null_checks || !strict_property_initialization || ambient {
            return Ok(());
        }
        let constructor = self.find_constructor_declaration(node);
        let members = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        for member in self.nodes_of(members) {
            if tsrs2_binder::node_util::has_syntactic_modifier(
                self.binder.source_of_node(member),
                member,
                ModifierFlags::AMBIENT,
            ) {
                continue;
            }
            if self.is_static_element(member) || !self.is_property_without_initializer(member) {
                continue;
            }
            let Some(prop_name) = self.name_of_node(member) else {
                continue;
            };
            if !matches!(
                self.kind_of(prop_name),
                SyntaxKind::Identifier
                    | SyntaxKind::PrivateIdentifier
                    | SyntaxKind::ComputedPropertyName
            ) {
                continue;
            }
            let member_symbol = self.get_symbol_of_declaration(member)?;
            let member_type = self.get_type_of_symbol(member_symbol)?;
            if self
                .tables
                .flags_of(member_type)
                .intersects(TypeFlags::ANY_OR_UNKNOWN)
                || self.contains_undefined_type(member_type)
            {
                continue;
            }
            let initialized = match constructor {
                Some(ctor) => {
                    self.is_property_initialized_in_constructor(member_symbol, member_type, ctor)?
                }
                None => false,
            };
            if !initialized {
                let display = self.declaration_name_display(prop_name);
                self.error_at(
                    Some(prop_name),
                    &diagnostics::Property_0_has_no_initializer_and_is_not_definitely_assigned_in_the_constructor,
                    &[&display],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: isPropertyWithoutInitializer @6.0.3
    /// tsc-hash: 8370e069d548eefacf2e9cee538e49d3f25da4bd308441f99d511c96b6dc068c
    /// tsc-span: _tsc.js:85499-85501
    fn is_property_without_initializer(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::PropertyDeclaration(data) => {
                !tsrs2_binder::node_util::has_syntactic_modifier(
                    self.binder.source_of_node(node),
                    node,
                    ModifierFlags::ABSTRACT,
                ) && data.exclamation_token.is_none()
                    && data.initializer.is_none()
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Class-band pins (oracle: tsc 6.0.3 noLib, scratchpad probe.sh
    /// p2-p6, 2026-07-14).
    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            rows(state)
        })
    }

    fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
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
    }

    #[test]
    fn strict_property_initialization_constructor_face_reports_2564() {
        // Oracle: (2564, 10, 1) — the empty constructor never assigns
        // p; the flow probe (isPropertyInitializedInConstructor,
        // M5 post-close review) proves undefined survived. The
        // no-constructor face is pinned live in check.rs
        // (class_property_out_annotation_reports_2636).
        assert_eq!(
            checked_rows("class C { p: string; constructor() {} }\n"),
            [(2564, 10, 1)]
        );
        // Oracle: clean — a straight-line constructor assignment
        // proves initialization.
        assert_eq!(
            checked_rows("class C { p: string; constructor() { this.p = \"x\"; } }\n"),
            []
        );
        // Oracle: (2564, 10, 1) — a single-branch assignment is not
        // definite (the JOIN keeps undefined).
        assert_eq!(
            checked_rows(
                "class C { p: string; constructor(b: boolean) { if (b) { this.p = \"x\"; } } }\n"
            ),
            [(2564, 10, 1)]
        );
        // Oracle: clean — both branches assign.
        assert_eq!(
            checked_rows(
                "class C { p: string; constructor(b: boolean) { if (b) { this.p = \"x\"; } else { this.p = \"y\"; } } }\n"
            ),
            []
        );
        // Oracle: (2564, 10, 2) / clean — the private flavor grounds
        // on the `__#…@` description through the same synthetic
        // chain.
        assert_eq!(
            checked_rows("class C { #p: string; constructor() {} }\n"),
            [(2564, 10, 2)]
        );
        assert_eq!(
            checked_rows("class C { #p: string; constructor() { this.#p = \"x\"; } }\n"),
            []
        );
    }

    #[test]
    fn overwrite_base_property_fifth_face_reports_2612() {
        // Oracle: (2564, 38, 1) + (2612, 38, 1) — constructor present
        // but the property is NOT assigned in it: the fifth 2612
        // disjunct (85370, !isPropertyInitializedInConstructor) fires
        // alongside the 2564 face. The probe's declared type is the
        // DERIVED CLASS type (tsc quirk, preserved). Raw emission
        // order here (override checks run before property
        // initialization); the program layer's sort restores tsc's
        // 2564-first order at equal spans.
        assert_eq!(
            checked_rows(
                "class B { p = 1 }\nclass D extends B { p: number; constructor() { super(); } }\n"
            ),
            [(2612, 38, 1), (2564, 38, 1)]
        );
        // Oracle: clean — the constructor assignment clears BOTH
        // faces.
        assert_eq!(
            checked_rows(
                "class B { p = 1 }\nclass D extends B { p: number; constructor() { super(); this.p = 2; } }\n"
            ),
            []
        );
    }

    #[test]
    fn override_without_base_class_reports_4112() {
        // Oracle: (4112, 19, 1).
        assert_eq!(
            checked_rows("class C { override m(): void {} }\n"),
            [(4112, 19, 1)]
        );
    }

    #[test]
    fn incompatible_derived_property_reports_member_specific_2416() {
        // Oracle: (2416, 63, 1) — the member row's chain root IS the
        // reported code; the broad 2415 suppresses.
        assert_eq!(
            checked_rows(
                "class B2 { p: { x: number } = { x: 1 } }\nclass D2 extends B2 { p: { x: string } = { x: \"s\" } }\n"
            ),
            [(2416, 63, 1)]
        );
    }

    #[test]
    fn interface_multi_extends_mismatch_reports_2320_at_name() {
        // Oracle: (2320, 64, 2) with the Named_property 2319 detail in
        // the chain tail.
        let text =
            "interface I1 { a: number }\ninterface I2 { a: string }\ninterface I3 extends I1, I2 {}\n";
        assert_eq!(checked_rows(text), [(2320, 64, 2)]);
    }

    #[test]
    fn empty_string_class_members_do_not_conflict() {
        assert_eq!(
            checked_rows("class C { \"\": number; \"\": string; }\n"),
            [(2717, 22, 2)]
        );
    }

    #[test]
    fn empty_heritage_list_position_is_utf16() {
        assert_eq!(
            checked_rows("const é = 0; class C implements {}\n"),
            [(1097, 31, 0)]
        );
    }

    #[test]
    fn unimplemented_inherited_abstract_member_reports_2515() {
        // Oracle: (2515, 48, 2).
        assert_eq!(
            checked_rows("abstract class AB { abstract m(): void; }\nclass CC extends AB {}\n"),
            [(2515, 48, 2)]
        );
    }

    #[test]
    fn class_modifier_error_suppresses_heritage_grammar() {
        // m4-review S7 (oracle: vendored tsc 6.0.3, noLib, strict,
        // 2026-07-19): tsc reports 1042 ONLY — checkGrammarModifiers'
        // async verdict suppresses the duplicate-extends walk (1172).
        // The 1042 row itself stays the M7 FN, so the port answers
        // clean; pre-fix it reported the 1172.
        assert_eq!(
            checked_rows("declare const A: any, B: any;\nasync class C extends A extends B {}\n"),
            []
        );
    }
}

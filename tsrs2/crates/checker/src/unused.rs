//! M7 8.3/8.4 unused-identifier producers.
//!
//! Workers land by declaration owner. The semantic error surface is
//! activated first under `noUnusedLocals` / `noUnusedParameters`; the
//! same registrations feed the suggestion surface in 8.4.

use tsrs2_binder::node_util;
use tsrs2_diags::{gen as diagnostics, DiagnosticCategory};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckResult2, CheckerState};

#[derive(Clone, Copy)]
enum UnusedIdentifierKind {
    Local,
    Parameter,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: registerForUnusedIdentifiersCheck @6.0.3
    /// tsc-hash: bd4d966695b8aae018cbaea7cf4462c968f8d9672dc8812f6a7b06cbf76fa16f
    /// tsc-span: _tsc.js:82942-82953
    /// d2: d2:08b79e6517d01e5d88bb72d904893471db59fd488401a64241687e5df4e9affe
    ///
    /// The Rust checker stores registrations in their owning file's
    /// entry and drains it after that file's deferred nodes. The
    /// source-root key is essential because checking one file can force
    /// declarations owned by another before the latter's deferred body walk.
    pub(crate) fn register_for_unused_identifiers_check(&mut self, node: NodeId) {
        let root = self.binder.source_of_node(node).root;
        let nodes = self.potentially_unused_identifiers.entry(root).or_default();
        if !nodes.contains(&node) {
            nodes.push(node);
        }
    }

    /// tsc-port: checkUnusedIdentifiers @6.0.3
    /// tsc-hash: dcbee129b87b48f266b1bc1836718003e82f6b483b3d639b06b2ca7de12cd6df
    /// tsc-span: _tsc.js:82954-82991
    ///
    /// Suggestion/category projection additionally mirrors
    /// getSuggestionDiagnostics (46868-46878) and unusedIsError
    /// (86987-86998).
    ///
    /// Only registered producers can reach this match. Publication is
    /// kind-aware at each addDiagnostic call so mixed function owners
    /// preserve the independent noUnusedLocals/noUnusedParameters
    /// gates.
    pub(crate) fn check_registered_unused_identifiers(&mut self, root: NodeId) {
        let nodes = self
            .potentially_unused_identifiers
            .remove(&root)
            .unwrap_or_default();
        for node in nodes {
            let result = match self.kind_of(node) {
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => self
                    .check_unused_class_members(node)
                    .and_then(|()| self.check_unused_type_parameters(node)),
                SyntaxKind::SourceFile => self.check_unused_locals_and_parameters(node),
                SyntaxKind::ModuleDeclaration
                | SyntaxKind::Block
                | SyntaxKind::CaseBlock
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement => self.check_unused_locals_and_parameters(node),
                SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::Constructor => {
                    let locals =
                        if node_util::body_of(self.binder.source_of_node(node), node).is_some() {
                            self.check_unused_locals_and_parameters(node)
                        } else {
                            Ok(())
                        };
                    locals.and_then(|()| self.check_unused_type_parameters(node))
                }
                SyntaxKind::MethodSignature
                | SyntaxKind::CallSignature
                | SyntaxKind::ConstructSignature
                | SyntaxKind::FunctionType
                | SyntaxKind::ConstructorType
                | SyntaxKind::TypeAliasDeclaration
                | SyntaxKind::InterfaceDeclaration => self.check_unused_type_parameters(node),
                SyntaxKind::InferType => self.check_unused_infer_type_parameter(node),
                // registerForUnusedIdentifiersCheck is shape-driven in
                // binder/checkBlock while tsc's consumer switch is
                // kind-driven.  Recovery and newly materialized node kinds
                // can therefore own the same two data sets without appearing
                // in the historical switch: drain whichever ownership shape
                // is present instead of abandoning the node.
                _ => {
                    let locals = if self.binder.locals_of(node).is_some() {
                        self.check_unused_locals_and_parameters(node)
                    } else {
                        Ok(())
                    };
                    if self.type_parameter_declarations_of(node).is_empty() {
                        locals
                    } else {
                        locals.and_then(|()| self.check_unused_type_parameters(node))
                    }
                }
            };
            if let Err(abort) = result {
                self.mark_oracle_crash_range(node, abort);
            }
        }
    }

    fn is_ambient_for_unused(&self, node: NodeId) -> bool {
        self.binder.flags_of(node).intersects(NodeFlags::AMBIENT)
            || NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT)
            || node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::AMBIENT,
            )
    }

    fn is_recovery_only_unused_declaration(&self, node: NodeId) -> bool {
        NodeFlags::from_bits(self.node_flags(node))
            .intersects(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
    }

    fn unused_is_error(&self, node: NodeId, kind: UnusedIdentifierKind) -> bool {
        if self.is_ambient_for_unused(node) {
            return false;
        }
        match kind {
            UnusedIdentifierKind::Local => self.options.no_unused_locals == Some(true),
            UnusedIdentifierKind::Parameter => self.options.no_unused_parameters == Some(true),
        }
    }

    fn add_unused_diagnostic_at(
        &mut self,
        containing_node: NodeId,
        kind: UnusedIdentifierKind,
        location: Option<NodeId>,
        message: &'static tsrs2_diags::DiagnosticMessage,
        args: &[&str],
    ) {
        if self.is_recovery_only_unused_declaration(containing_node) {
            return;
        }
        let mut diagnostic = self.create_error(location, message, args);
        if !self.unused_is_error(containing_node, kind) {
            diagnostic.message.category = DiagnosticCategory::Suggestion;
        }
        self.push_error_diagnostic(diagnostic);
    }

    fn add_unused_diagnostic_at_byte_range(
        &mut self,
        containing_node: NodeId,
        kind: UnusedIdentifierKind,
        start_byte: usize,
        end_byte: usize,
        message: &'static tsrs2_diags::DiagnosticMessage,
        args: &[&str],
    ) {
        if self.is_recovery_only_unused_declaration(containing_node) {
            return;
        }
        let index = self.error_at_byte_range_with_args(
            containing_node,
            start_byte,
            end_byte,
            message,
            args,
        );
        if !self.unused_is_error(containing_node, kind) {
            self.diagnostics[index].message.category = DiagnosticCategory::Suggestion;
        }
    }

    /// tsc-port: checkUnusedClassMembers @6.0.3
    /// tsc-hash: b5c9ae6d244cc4bb01e39b9b4fd715a5417bb06e780f0a33cbb49b96ff1f65af
    /// tsc-span: _tsc.js:83008-83038
    /// d2: d2:5a2c45fdca4506945d356d1d7cf0abdfbf8b3db6c524587eb3031fd4e0169d16
    fn check_unused_class_members(&mut self, node: NodeId) -> CheckResult2<()> {
        let members = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => return Ok(()),
        };
        for member in self.nodes_of(members) {
            if self.is_recovery_only_unused_declaration(member) {
                continue;
            }
            match self.kind_of(member) {
                SyntaxKind::MethodDeclaration
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor => {
                    let symbol = self.get_symbol_of_declaration(member)?;
                    if self.kind_of(member) == SyntaxKind::SetAccessor
                        && self
                            .binder
                            .symbol(symbol)
                            .flags
                            .intersects(SymbolFlags::GET_ACCESSOR)
                    {
                        continue;
                    }
                    let Some(name) = self.name_of_node(member) else {
                        continue;
                    };
                    let private = node_util::get_combined_modifier_flags(
                        self.binder.source_of_node(member),
                        member,
                    )
                    .intersects(ModifierFlags::PRIVATE)
                        || self.kind_of(name) == SyntaxKind::PrivateIdentifier;
                    if self.links.symbol(symbol).is_referenced.is_empty()
                        && private
                        && !NodeFlags::from_bits(self.node_flags(member))
                            .intersects(NodeFlags::AMBIENT)
                    {
                        let display = self.declaration_name_display(name);
                        self.add_unused_diagnostic_at(
                            member,
                            UnusedIdentifierKind::Local,
                            Some(name),
                            &diagnostics::_0_is_declared_but_its_value_is_never_read,
                            &[&display],
                        );
                    }
                }
                SyntaxKind::Constructor => {
                    let parameters = match self.data_of(member) {
                        NodeData::Constructor(data) => data.parameters,
                        _ => None,
                    };
                    for parameter in self.nodes_of(parameters) {
                        let symbol = self.get_symbol_of_declaration(parameter)?;
                        if !self.links.symbol(symbol).is_referenced.is_empty()
                            || !node_util::has_syntactic_modifier(
                                self.binder.source_of_node(parameter),
                                parameter,
                                ModifierFlags::PRIVATE,
                            )
                        {
                            continue;
                        }
                        let Some(name) = self.name_of_node(parameter) else {
                            continue;
                        };
                        let display = self.symbol_display_name(symbol);
                        self.add_unused_diagnostic_at(
                            parameter,
                            UnusedIdentifierKind::Local,
                            Some(name),
                            &diagnostics::Property_0_is_declared_but_its_value_is_never_read,
                            &[&display],
                        );
                    }
                }
                SyntaxKind::IndexSignature
                | SyntaxKind::SemicolonClassElement
                | SyntaxKind::ClassStaticBlockDeclaration => {}
                // Parser-created class members are exhausted above. A
                // checker-synthetic recovery member has no supported name
                // ownership and therefore contributes no unused diagnostic.
                _ => {}
            }
        }
        Ok(())
    }

    /// tsc-port: checkUnusedInferTypeParameter @6.0.3
    /// tsc-hash: 5361f1034a082554254b852b09d2e7ae434d6e951b2527c0f511d91b1b7483e6
    /// tsc-span: _tsc.js:83039-83044
    /// d2: d2:1a7c9df7149acc8d3c6e6cc251f8528c50f63dacedad718d7f5bfff2b4783063
    fn check_unused_infer_type_parameter(&mut self, node: NodeId) -> CheckResult2<()> {
        let type_parameter = match self.data_of(node) {
            NodeData::InferType(data) => data.type_parameter,
            _ => None,
        };
        let Some(type_parameter) = type_parameter else {
            return Ok(());
        };
        if !self.is_type_parameter_unused(type_parameter)? {
            return Ok(());
        }
        let name = self
            .name_of_node(type_parameter)
            .and_then(|name| self.identifier_text_of(name))
            .unwrap_or_default()
            .to_owned();
        self.add_unused_diagnostic_at(
            node,
            UnusedIdentifierKind::Parameter,
            Some(node),
            &diagnostics::_0_is_declared_but_its_value_is_never_read,
            &[&name],
        );
        Ok(())
    }

    /// tsc-port: checkUnusedTypeParameters @6.0.3
    /// tsc-hash: c6294c45ceb55de9dfec242db2e34ba86ac499530e73b118fbefe0e41acf73fe
    /// tsc-span: _tsc.js:83045-83066
    /// d2: d2:6cbef847d0eda39c3a2178516c958e766146c3da957e1c890aacaecb5e78c48c
    fn check_unused_type_parameters(&mut self, node: NodeId) -> CheckResult2<()> {
        let symbol = self.get_symbol_of_declaration(node)?;
        let declarations = self.binder.symbol(symbol).declarations.clone();
        if declarations.last().copied() != Some(node) {
            return Ok(());
        }

        let type_parameters = self.type_parameter_declarations_of(node);
        let mut seen_parents_with_every_unused = Vec::new();
        for type_parameter in type_parameters {
            if !self.is_type_parameter_unused(type_parameter)? {
                continue;
            }
            let name = self
                .name_of_node(type_parameter)
                .and_then(|name| self.identifier_text_of(name))
                .unwrap_or_default()
                .to_owned();
            let Some(parent) = self.parent_of(type_parameter) else {
                continue;
            };
            let parent_type_parameters = match self.data_of(parent) {
                NodeData::JSDocTemplateTag(data) => self.nodes_of(data.type_parameters),
                _ => self
                    .type_parameter_declaration_list_of(parent)
                    .map_or_else(Vec::new, |list| self.nodes_of(Some(list))),
            };
            let every_unused = self.kind_of(parent) != SyntaxKind::InferType
                && !parent_type_parameters.is_empty()
                && {
                    let mut all_unused = true;
                    for &candidate in &parent_type_parameters {
                        if !self.is_type_parameter_unused(candidate)? {
                            all_unused = false;
                            break;
                        }
                    }
                    all_unused
                };
            if every_unused {
                if seen_parents_with_every_unused.contains(&parent) {
                    continue;
                }
                seen_parents_with_every_unused.push(parent);
                let (start_byte, end_byte) = if self.kind_of(parent) == SyntaxKind::JSDocTemplateTag
                {
                    let source = self.binder.source_of_node(parent);
                    let parent_node = source.arena.node(parent);
                    (
                        tsrs2_syntax::skip_trivia(&source.text, parent_node.pos as usize),
                        parent_node.end.max(parent_node.pos) as usize,
                    )
                } else {
                    let Some(list) = self.type_parameter_declaration_list_of(parent) else {
                        continue;
                    };
                    self.range_of_type_parameters(parent, list)
                };
                if parent_type_parameters.len() == 1 {
                    self.add_unused_diagnostic_at_byte_range(
                        type_parameter,
                        UnusedIdentifierKind::Parameter,
                        start_byte,
                        end_byte,
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&name],
                    );
                } else {
                    self.add_unused_diagnostic_at_byte_range(
                        type_parameter,
                        UnusedIdentifierKind::Parameter,
                        start_byte,
                        end_byte,
                        &diagnostics::All_type_parameters_are_unused,
                        &[],
                    );
                }
            } else {
                let name = self
                    .name_of_node(type_parameter)
                    .and_then(|name| self.identifier_text_of(name))
                    .unwrap_or_default()
                    .to_owned();
                self.add_unused_diagnostic_at(
                    type_parameter,
                    UnusedIdentifierKind::Parameter,
                    Some(type_parameter),
                    &diagnostics::_0_is_declared_but_its_value_is_never_read,
                    &[&name],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: rangeOfTypeParameters @6.0.3
    /// tsc-hash: 201db1995ff249c9a5f5edd046bc4a5992d8ca3a0d3615ed541f48a9ffa3af66
    /// tsc-span: _tsc.js:18872-18876
    /// d2: d2:63108413387c5421cc26a36984cfc811a2f88a143617830fcd738599c0238ad5
    fn range_of_type_parameters(
        &self,
        node: NodeId,
        list: tsrs2_syntax::NodeArrayId,
    ) -> (usize, usize) {
        let source = self.binder.source_of_node(node);
        let array = source.arena.node_array(list);
        let start = (array.pos as usize).saturating_sub(1);
        let end = tsrs2_syntax::skip_trivia(&source.text, array.end as usize)
            .saturating_add(1)
            .min(source.text.len());
        (start, end)
    }

    /// tsc-port: isTypeParameterUnused @6.0.3
    /// tsc-hash: d4cc4fc46164e7575e1f9964fbc87191270877a3ab40825f641d9c379c47e8fe
    /// tsc-span: _tsc.js:83067-83069
    /// d2: d2:94ef9c9390a0c96872a6bdc750aa0024387233d4c00c6d7b0c91fcd2a7049e60
    fn is_type_parameter_unused(&mut self, type_parameter: NodeId) -> CheckResult2<bool> {
        let Some(name) = self.name_of_node(type_parameter) else {
            return Ok(false);
        };
        if self.is_recovery_only_unused_declaration(name)
            || self.identifier_text_of(name).is_none_or(str::is_empty)
        {
            return Ok(false);
        }
        let symbol = self.get_symbol_of_declaration(type_parameter)?;
        Ok(!self
            .links
            .symbol(symbol)
            .is_referenced
            .intersects(SymbolFlags::TYPE_PARAMETER)
            && !self.identifier_starts_with_underscore(name))
    }

    /// tsc-port: checkUnusedLocalsAndParameters @6.0.3
    /// tsc-hash: 3ac75f66721fdf0f79ff81f8775c3d1dbb6eb2a95489e3a581a653c60696a264
    /// tsc-span: _tsc.js:83091-83179
    ///
    /// M7 8.3c activates the SourceFile producer. The worker is kept
    /// declaration-owner complete so later block/function
    /// registrations and the 8.4 suggestion pass reuse the same
    /// grouping semantics.
    fn check_unused_locals_and_parameters(&mut self, node: NodeId) -> CheckResult2<()> {
        let Some(locals) = self.binder.locals_of(node) else {
            return Ok(());
        };
        let locals = locals.values().copied().collect::<Vec<_>>();
        let mut unused_imports = Vec::<(NodeId, Vec<NodeId>)>::new();
        let mut unused_destructures = Vec::<(NodeId, Vec<NodeId>)>::new();
        let mut unused_variables = Vec::<(NodeId, Vec<NodeId>)>::new();

        // d2: d2:d02bffc14a5b17c97eff6900de2ce867ee8b870872e10164db9d6b41407a2382
        // tsc-hash: 978abe1ba0e43b808f1af98c0250c03f21f2b4b2c205df5dc87a163998c217c1
        // tsc-span: _tsc.js:83095-83134
        for local in locals {
            let symbol = self.binder.symbol(local);
            let referenced = self.links.symbol(local).is_referenced;
            if symbol.flags.intersects(SymbolFlags::TYPE_PARAMETER) {
                if !symbol.flags.intersects(SymbolFlags::VARIABLE)
                    || referenced.intersects(SymbolFlags::VARIABLE)
                {
                    continue;
                }
            } else if !referenced.is_empty() || symbol.export_symbol.is_some() {
                continue;
            }
            let declarations = symbol.declarations.clone();
            for declaration in declarations {
                if self.is_recovery_only_imported_declaration(declaration) {
                    continue;
                }
                if self.is_valid_unused_local_declaration(declaration) {
                    continue;
                }
                if self.is_imported_declaration_for_unused(declaration) {
                    if let Some(import_clause) =
                        self.import_clause_from_imported_declaration(declaration)
                    {
                        add_to_unused_group(&mut unused_imports, import_clause, declaration);
                    }
                } else if self.kind_of(declaration) == SyntaxKind::BindingElement
                    && self.parent_of(declaration).is_some_and(|parent| {
                        self.kind_of(parent) == SyntaxKind::ObjectBindingPattern
                    })
                {
                    let pattern = self.parent_of(declaration).expect("checked above");
                    let elements = match self.data_of(pattern) {
                        NodeData::ObjectBindingPattern(data) => self.nodes_of(data.elements),
                        _ => Vec::new(),
                    };
                    let last_has_rest = elements.last().is_some_and(|last| {
                        matches!(
                            self.data_of(*last),
                            NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some()
                        )
                    });
                    if elements.last().copied() == Some(declaration) || !last_has_rest {
                        add_to_unused_group(&mut unused_destructures, pattern, declaration);
                    }
                } else if self.kind_of(declaration) == SyntaxKind::VariableDeclaration {
                    let source = self.binder.source_of_node(declaration);
                    let block_scope_kind = node_util::get_combined_node_flags(source, declaration)
                        .bits()
                        & NodeFlags::BLOCK_SCOPED.bits();
                    let name = self.name_of_node(declaration);
                    if !matches!(
                        block_scope_kind,
                        bits if bits == NodeFlags::USING.bits()
                            || bits == NodeFlags::AWAIT_USING.bits()
                    ) || !name.is_some_and(|name| self.identifier_starts_with_underscore(name))
                    {
                        if let Some(declaration_list) = self.parent_of(declaration) {
                            add_to_unused_group(
                                &mut unused_variables,
                                declaration_list,
                                declaration,
                            );
                        }
                    }
                } else {
                    let value_declaration = self.binder.symbol(local).value_declaration;
                    let parameter = value_declaration.map(|value| {
                        node_util::get_root_declaration(self.binder.source_of_node(value), value)
                    });
                    let name = value_declaration.and_then(|value| self.name_of_node(value));
                    if let Some(parameter) = parameter
                        .filter(|parameter| self.kind_of(*parameter) == SyntaxKind::Parameter)
                    {
                        if let Some(name) = name {
                            if !self.is_parameter_property_declaration(parameter)
                                && !self.parameter_is_this_keyword(parameter)
                                && !self.identifier_starts_with_underscore(name)
                            {
                                if self.kind_of(declaration) == SyntaxKind::BindingElement
                                    && self.parent_of(declaration).is_some_and(|parent| {
                                        self.kind_of(parent) == SyntaxKind::ArrayBindingPattern
                                    })
                                {
                                    let pattern =
                                        self.parent_of(declaration).expect("checked above");
                                    add_to_unused_group(
                                        &mut unused_destructures,
                                        pattern,
                                        declaration,
                                    );
                                } else {
                                    let display = tsrs2_binder::unescape_leading_underscores(
                                        &self.binder.symbol(local).escaped_name,
                                    )
                                    .to_owned();
                                    self.add_unused_diagnostic_at(
                                        parameter,
                                        UnusedIdentifierKind::Parameter,
                                        Some(name),
                                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                                        &[&display],
                                    );
                                }
                            }
                        }
                    } else {
                        self.error_unused_local(declaration, local);
                    }
                }
            }
        }

        // d2: d2:8919b2f655dc58c18f02623ad56a97c61dc9d699c539ae15aaa335cc6b8b9f48
        // tsc-hash: b9f9aacce79c115a9368193c74ebfd2c4de226f95d8fa994d8e9998a34006e17
        // tsc-span: _tsc.js:83135-83147
        for (import_clause, unuseds) in unused_imports {
            let Some(import_decl) = self.parent_of(import_clause) else {
                continue;
            };
            let n_declarations = self.import_clause_declaration_count(import_clause);
            if n_declarations == unuseds.len() {
                if unuseds.len() == 1 {
                    let unused = unuseds[0];
                    let display = self
                        .name_of_node(unused)
                        .map(|name| self.declaration_name_display(name))
                        .unwrap_or_default();
                    self.add_unused_diagnostic_at(
                        import_decl,
                        UnusedIdentifierKind::Local,
                        Some(import_decl),
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                } else {
                    self.add_unused_diagnostic_at(
                        import_decl,
                        UnusedIdentifierKind::Local,
                        Some(import_decl),
                        &diagnostics::All_imports_in_import_declaration_are_unused,
                        &[],
                    );
                }
            } else {
                for unused in unuseds {
                    let Some(symbol) = self.binder.node_symbol(unused) else {
                        continue;
                    };
                    self.error_unused_local(unused, symbol);
                }
            }
        }

        // d2: d2:67471b584c654abfec76a4c7c0d2694f196e6180df978c5dbb98c7be4dfbda4a
        // tsc-hash: 719145c1c05fe3bacc72f7b46bd148754dd25e0b92d9b519debe96b9fe49c2d1
        // tsc-span: _tsc.js:83148-83165
        for (binding_pattern, binding_elements) in unused_destructures {
            if binding_elements.is_empty() {
                continue;
            }
            let kind = self.unused_binding_pattern_kind(binding_pattern);
            let elements = self.unused_binding_pattern_elements(binding_pattern);
            if elements.len() == binding_elements.len() {
                let single_variable = binding_elements.len() == 1
                    && self.parent_of(binding_pattern).is_some_and(|parent| {
                        self.kind_of(parent) == SyntaxKind::VariableDeclaration
                            && self.parent_of(parent).is_some_and(|list| {
                                self.kind_of(list) == SyntaxKind::VariableDeclarationList
                            })
                    });
                if single_variable {
                    let declaration = self.parent_of(binding_pattern).expect("checked above");
                    let list = self.parent_of(declaration).expect("checked above");
                    add_to_unused_group(&mut unused_variables, list, declaration);
                } else if binding_elements.len() == 1 {
                    let display = self
                        .name_of_node(binding_elements[0])
                        .map(|name| self.unused_binding_name_text(name))
                        .unwrap_or_default();
                    self.add_unused_diagnostic_at(
                        binding_pattern,
                        kind,
                        Some(binding_pattern),
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                } else {
                    self.add_unused_diagnostic_at(
                        binding_pattern,
                        kind,
                        Some(binding_pattern),
                        &diagnostics::All_destructured_elements_are_unused,
                        &[],
                    );
                }
            } else {
                for element in binding_elements {
                    let display = self
                        .name_of_node(element)
                        .map(|name| self.unused_binding_name_text(name))
                        .unwrap_or_default();
                    self.add_unused_diagnostic_at(
                        element,
                        kind,
                        Some(element),
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                }
            }
        }

        // d2: d2:57b43b03c2ff6fb8e42339f24cbc495a46fcbe7854f2ac4ab16a086a1485dbda
        // tsc-hash: b56504efa5a0e13944c2a5db705ff16d55f3f400a62a9a01f0af2e85cbb84956
        // tsc-span: _tsc.js:83166-83178
        for (declaration_list, declarations) in unused_variables {
            let all_declarations = match self.data_of(declaration_list) {
                NodeData::VariableDeclarationList(data) => self.nodes_of(data.declarations),
                _ => Vec::new(),
            };
            if all_declarations.len() == declarations.len() {
                if declarations.len() == 1 {
                    let name = self.name_of_node(declarations[0]);
                    let display = name
                        .map(|name| self.unused_binding_name_text(name))
                        .unwrap_or_default();
                    self.add_unused_diagnostic_at(
                        declaration_list,
                        UnusedIdentifierKind::Local,
                        name,
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                } else {
                    let range = self
                        .parent_of(declaration_list)
                        .filter(|parent| self.kind_of(*parent) == SyntaxKind::VariableStatement)
                        .unwrap_or(declaration_list);
                    self.add_unused_diagnostic_at(
                        declaration_list,
                        UnusedIdentifierKind::Local,
                        Some(range),
                        &diagnostics::All_variables_are_unused,
                        &[],
                    );
                }
            } else {
                for declaration in declarations {
                    let display = self
                        .name_of_node(declaration)
                        .map(|name| self.unused_binding_name_text(name))
                        .unwrap_or_default();
                    self.add_unused_diagnostic_at(
                        declaration,
                        UnusedIdentifierKind::Local,
                        Some(declaration),
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                }
            }
        }
        Ok(())
    }

    /// tsc-port: errorUnusedLocal @6.0.3
    /// tsc-hash: a0859bf31f12b34a4d97492b714654753bd5d7f9b198bfa8529d878e28eb06d3
    /// tsc-span: _tsc.js:83000-83004
    /// d2: d2:435cd87c2bcdcc3eb69b3135503cdb119fce2de337927782eb67ee038afe8576
    fn error_unused_local(&mut self, declaration: NodeId, symbol: tsrs2_types::SymbolId) {
        let node = node_util::get_name_of_declaration(
            self.binder.source_of_node(declaration),
            declaration,
        )
        .unwrap_or(declaration);
        let display =
            tsrs2_binder::unescape_leading_underscores(&self.binder.symbol(symbol).escaped_name)
                .to_owned();
        let message = if self.is_type_declaration_for_unused(declaration) {
            &diagnostics::_0_is_declared_but_never_used
        } else {
            &diagnostics::_0_is_declared_but_its_value_is_never_read
        };
        self.add_unused_diagnostic_at(
            declaration,
            UnusedIdentifierKind::Local,
            Some(node),
            message,
            &[&display],
        );
    }

    fn unused_binding_pattern_kind(&self, binding_pattern: NodeId) -> UnusedIdentifierKind {
        let source = self.binder.source_of_node(binding_pattern);
        let root = self
            .parent_of(binding_pattern)
            .map(|parent| node_util::get_root_declaration(source, parent));
        if root.is_some_and(|root| self.kind_of(root) == SyntaxKind::Parameter) {
            UnusedIdentifierKind::Parameter
        } else {
            UnusedIdentifierKind::Local
        }
    }

    fn is_type_declaration_for_unused(&self, declaration: NodeId) -> bool {
        match self.kind_of(declaration) {
            SyntaxKind::TypeParameter
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag
            | SyntaxKind::JSDocEnumTag => true,
            SyntaxKind::ImportClause => {
                matches!(self.data_of(declaration), NodeData::ImportClause(data) if data.is_type_only)
            }
            SyntaxKind::ImportSpecifier => self
                .parent_of(declaration)
                .and_then(|named| self.parent_of(named))
                .is_some_and(|clause| {
                    matches!(self.data_of(clause), NodeData::ImportClause(data) if data.is_type_only)
                }),
            _ => false,
        }
    }

    fn identifier_starts_with_underscore(&self, node: NodeId) -> bool {
        self.identifier_text_of(node)
            .is_some_and(|text| text.starts_with('_'))
    }

    fn is_valid_unused_local_declaration(&self, declaration: NodeId) -> bool {
        // A JSDoc import is bound into its effective host's locals for
        // type resolution, but the tag is not itself an
        // external/CommonJS SourceFile unused owner in tsc
        // (bindJSDocImports 44073-44103; SourceFile registration
        // 87025-87027). Keep its synthetic import-clause declarations
        // out of an enclosing recovery drain.
        if matches!(
            self.kind_of(declaration),
            SyntaxKind::ImportClause | SyntaxKind::ImportSpecifier | SyntaxKind::NamespaceImport
        ) && std::iter::successors(Some(declaration), |&node| self.parent_of(node))
            .take(4)
            .any(|node| self.kind_of(node) == SyntaxKind::JSDocImportTag)
        {
            return true;
        }
        // tsc does not surface ordinary unused-identifier suggestions
        // for declarations that exist only inside attached JSDoc.
        // Their symbols participate in type resolution, but the tags
        // are not source-language local declarations.
        if matches!(
            self.kind_of(declaration),
            SyntaxKind::JSDocTypedefTag
                | SyntaxKind::JSDocCallbackTag
                | SyntaxKind::JSDocEnumTag
                | SyntaxKind::JSDocPropertyTag
                | SyntaxKind::JSDocParameterTag
        ) {
            return true;
        }
        if self.kind_of(declaration) == SyntaxKind::BindingElement {
            let (name, property_name) = match self.data_of(declaration) {
                NodeData::BindingElement(data) => (data.name, data.property_name),
                _ => (None, None),
            };
            let object_binding = self
                .parent_of(declaration)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ObjectBindingPattern);
            return if object_binding {
                property_name.is_some()
                    && name.is_some_and(|name| self.identifier_starts_with_underscore(name))
            } else {
                name.is_some_and(|name| self.identifier_starts_with_underscore(name))
            };
        }
        if self.kind_of(declaration) == SyntaxKind::ModuleDeclaration
            && node_util::is_ambient_module(self.binder.source_of_node(declaration), declaration)
        {
            return true;
        }
        let name_starts_with_underscore = self
            .name_of_node(declaration)
            .is_some_and(|name| self.identifier_starts_with_underscore(name));
        if !name_starts_with_underscore {
            return false;
        }
        let variable_in_for = self.kind_of(declaration) == SyntaxKind::VariableDeclaration
            && self
                .parent_of(declaration)
                .and_then(|list| self.parent_of(list))
                .is_some_and(|parent| {
                    matches!(
                        self.kind_of(parent),
                        SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement
                    )
                });
        variable_in_for || self.is_imported_declaration_for_unused(declaration)
    }

    fn is_imported_declaration_for_unused(&self, declaration: NodeId) -> bool {
        matches!(
            self.kind_of(declaration),
            SyntaxKind::ImportClause | SyntaxKind::ImportSpecifier | SyntaxKind::NamespaceImport
        )
    }

    fn is_recovery_only_imported_declaration(&self, declaration: NodeId) -> bool {
        let Some(import_clause) = self.import_clause_from_imported_declaration(declaration) else {
            return false;
        };
        if matches!(
            self.data_of(import_clause),
            NodeData::ImportClause(data)
                if data.phase_modifier == Some(SyntaxKind::DeferKeyword)
                    && data
                        .name
                        .and_then(|name| self.identifier_text_of(name))
                        == Some("type")
        ) {
            return true;
        }
        let Some(import_declaration) = self.parent_of(import_clause) else {
            return false;
        };
        matches!(
            self.data_of(import_declaration),
            NodeData::ImportDeclaration(data)
                if data.attributes.is_some_and(|attributes| {
                    self.is_recovery_only_unused_declaration(attributes)
                })
        )
    }

    fn import_clause_from_imported_declaration(&self, declaration: NodeId) -> Option<NodeId> {
        match self.kind_of(declaration) {
            SyntaxKind::ImportClause => Some(declaration),
            SyntaxKind::NamespaceImport => self.parent_of(declaration),
            SyntaxKind::ImportSpecifier => self
                .parent_of(declaration)
                .and_then(|named| self.parent_of(named)),
            _ => None,
        }
    }

    fn import_clause_declaration_count(&self, import_clause: NodeId) -> usize {
        let NodeData::ImportClause(data) = self.data_of(import_clause) else {
            return 0;
        };
        usize::from(data.name.is_some())
            + data.named_bindings.map_or(0, |bindings| {
                if self.kind_of(bindings) == SyntaxKind::NamespaceImport {
                    1
                } else {
                    match self.data_of(bindings) {
                        NodeData::NamedImports(data) => self.nodes_of(data.elements).len(),
                        _ => 0,
                    }
                }
            })
    }

    fn unused_binding_pattern_elements(&self, binding_pattern: NodeId) -> Vec<NodeId> {
        match self.data_of(binding_pattern) {
            NodeData::ObjectBindingPattern(data) => self.nodes_of(data.elements),
            NodeData::ArrayBindingPattern(data) => self.nodes_of(data.elements),
            _ => Vec::new(),
        }
    }

    /// tsc `bindingNameText` (83196-83205): grouped unused-variable
    /// diagnostics name the first bound identifier recursively rather
    /// than rendering the surrounding binding pattern.
    fn unused_binding_name_text(&self, name: NodeId) -> String {
        match self.kind_of(name) {
            SyntaxKind::Identifier => node_util::id_text(self.binder.source_of_node(name), name)
                .map(str::to_owned)
                .unwrap_or_else(|| self.declaration_name_display(name)),
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern => self
                .unused_binding_pattern_elements(name)
                .into_iter()
                .find_map(|element| self.name_of_node(element))
                .map(|nested| self.unused_binding_name_text(nested))
                .unwrap_or_else(|| self.declaration_name_display(name)),
            _ => self.declaration_name_display(name),
        }
    }
}

fn add_to_unused_group(groups: &mut Vec<(NodeId, Vec<NodeId>)>, key: NodeId, value: NodeId) {
    if let Some((_, values)) = groups.iter_mut().find(|(candidate, _)| *candidate == key) {
        values.push(value);
    } else {
        groups.push((key, vec![value]));
    }
}

#[cfg(test)]
mod tests {
    use crate::state::test_support::with_program_state;
    use crate::{check_program, CompilerOptions, InputFile};
    use tsrs2_diags::DiagnosticCategory;

    fn unused_rows(
        text: &str,
        options: &CompilerOptions,
    ) -> Vec<(u32, DiagnosticCategory, u32, u32, String)> {
        unused_rows_for_files(&[("a.ts", text)], options)
    }

    fn unused_rows_for_files(
        files: &[(&str, &str)],
        options: &CompilerOptions,
    ) -> Vec<(u32, DiagnosticCategory, u32, u32, String)> {
        let files = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect::<Vec<_>>();
        check_program(&files, options)
            .diagnostics
            .into_iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.code(),
                    6133 | 6138 | 6192 | 6196 | 6198 | 6199 | 6205
                )
            })
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.category(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text().to_owned(),
                )
            })
            .collect()
    }

    fn unused_rows_with_file_for_files(
        files: &[(&str, &str)],
        options: &CompilerOptions,
    ) -> Vec<(String, u32, DiagnosticCategory, u32, u32, String)> {
        let files = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect::<Vec<_>>();
        check_program(&files, options)
            .diagnostics
            .into_iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.code(),
                    6133 | 6138 | 6192 | 6196 | 6198 | 6199 | 6205
                )
            })
            .map(|diagnostic| {
                let code = diagnostic.code();
                let category = diagnostic.category();
                let start = diagnostic.start.unwrap_or(u32::MAX);
                let length = diagnostic.length.unwrap_or(u32::MAX);
                let message = diagnostic.message_text().to_owned();
                (
                    diagnostic.file_name.unwrap_or_default(),
                    code,
                    category,
                    start,
                    length,
                    message,
                )
            })
            .collect()
    }

    #[test]
    fn unused_identifier_drain_preserves_bound_siblings_of_parse_errors() {
        let text = "export {}; const unused = 1; const used = 2; used; const broken = ;";
        let rows = unused_rows(text, &CompilerOptions::default());
        let start = text.find("unused").expect("unused declaration") as u32;
        assert_eq!(
            rows,
            vec![(
                6133,
                DiagnosticCategory::Suggestion,
                start,
                "unused".len() as u32,
                "'unused' is declared but its value is never read.".to_owned(),
            )]
        );

        assert!(unused_rows(
            "const globalUnused = 1; const broken = ;",
            &CompilerOptions::default(),
        )
        .is_empty());
    }

    #[test]
    fn parse_error_suppresses_the_nearest_unused_declaration_group() {
        assert!(unused_rows(
            "export {}; const hidden = 1, broken = ;",
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn unused_identifier_recovery_preserves_unicode_escape_spans() {
        let text = r"var \u0061wait = 12;
var \u0079ield = 12;
type typ\u0065 = 12;
export {};
";
        let rows = unused_rows(text, &CompilerOptions::default());
        assert_eq!(
            rows,
            vec![
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    text.find(r"\u0061wait").expect("escaped await") as u32,
                    r"\u0061wait".len() as u32,
                    "'await' is declared but its value is never read.".to_owned(),
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    text.find(r"\u0079ield").expect("escaped yield") as u32,
                    r"\u0079ield".len() as u32,
                    "'yield' is declared but its value is never read.".to_owned(),
                ),
                (
                    6196,
                    DiagnosticCategory::Suggestion,
                    text.find(r"typ\u0065").expect("escaped type") as u32,
                    r"typ\u0065".len() as u32,
                    "'type' is declared but never used.".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn merged_type_and_value_parameter_references_keep_their_meaning_masks() {
        let options = CompilerOptions {
            no_unused_locals: Some(true),
            no_unused_parameters: Some(true),
            ..CompilerOptions::default()
        };

        let value_only = "export function f<T>(T: number) { return T; }";
        assert_eq!(
            unused_rows(value_only, &options),
            [(
                6133,
                DiagnosticCategory::Error,
                value_only.find("<T>").expect("type parameter range") as u32,
                3,
                "'T' is declared but its value is never read.".to_owned(),
            )],
            "a value-meaning read must not mark the merged type-parameter face"
        );

        let type_only = "export function g<T>(T: number): T { throw 0; }";
        assert_eq!(
            unused_rows(type_only, &options),
            [(
                6133,
                DiagnosticCategory::Error,
                type_only.find("(T:").expect("value parameter") as u32 + 1,
                1,
                "'T' is declared but its value is never read.".to_owned(),
            )],
            "a type-meaning read must not mark the merged value-parameter face"
        );

        assert!(
            unused_rows(
                "export function h<T>(value: T): T { return value; }",
                &options,
            )
            .is_empty(),
            "the non-colliding sibling keeps both reference meanings"
        );
    }

    #[test]
    fn unused_module_instantiation_alias_does_not_reference_assignment_target() {
        let text = "declare namespace pack1 {
  const test1: string;
  export { test1 };
}
declare namespace pack2 {
  import test1 = pack1.test1;
  export { test1 };
}
export import test1 = pack2.test1;
declare namespace mod1 {
  type test1 = string;
  export { test1 };
}
declare namespace mod2 {
  import test1 = mod1.test1;
  export { test1 };
}
const test2 = mod2;
";
        let rows = unused_rows(text, &CompilerOptions::default());
        let start = text.find("test2 =").expect("test2 declaration") as u32;
        assert_eq!(
            rows,
            vec![(
                6133,
                DiagnosticCategory::Suggestion,
                start,
                "test2".len() as u32,
                "'test2' is declared but its value is never read.".to_owned(),
            )]
        );
    }

    #[test]
    fn unused_invalid_declare_accessor_uses_member_ambient_flag() {
        let text = "class C {
    declare get #pair()
    declare set #pair(value: number)
}
";
        let rows = unused_rows(text, &CompilerOptions::default());
        let start = text.find("#pair").expect("private getter name") as u32;
        assert_eq!(
            rows,
            vec![(
                6133,
                DiagnosticCategory::Suggestion,
                start,
                "#pair".len() as u32,
                "'#pair' is declared but its value is never read.".to_owned(),
            )]
        );
    }

    const CLASS_PROBE: &str = "class C {
  #used = 0;
  #unused = 0;
  private oldUsed = 0;
  private oldUnused = 0;
  get #pair() { return 0; }
  set #pair(_alue: number) {}
  get #dead() { return 0; }
  set #dead(_alue: number) {}
  constructor(private live: number, private dead: number) {
    this.#used;
    this.oldUsed;
    this.#pair;
    this.live;
  }
}
";

    #[test]
    fn unused_type_parameters_cover_every_ts_owner_and_exact_spans() {
        let text = "export class ClassSingle<T> {}\n\
                    export class ClassMultiple<T, U> {}\n\
                    export class ClassPartial<T, U> { value!: T; }\n\
                    export const ClassExpression = class<T, U> { value!: T; };\n\
                    export interface InterfaceSingle<T> {}\n\
                    export interface InterfaceMultiple<T, U> {}\n\
                    export interface InterfacePartial<T, U> { value: T; }\n\
                    export type AliasSingle<T> = number;\n\
                    export type AliasMultiple<T, U> = number;\n\
                    export type AliasPartial<T, U> = T;\n\
                    export function declaration<T, U>(value: T): T { return value; }\n\
                    export const expression = function<T, U>(value: T): T { return value; };\n\
                    export const arrow = <T, U>(value: T): T => value;\n\
                    export class Members { method<T, U>(value: T): T { return value; } }\n\
                    export type FunctionShape = <T, U>(value: T) => T;\n\
                    export type ConstructorShape = new <T, U>(value: T) => T;\n\
                    export interface Signatures {\n\
                        <T, U>(value: T): T;\n\
                        new<T, U>(value: T): T;\n\
                        method<T, U>(value: T): T;\n\
                    }\n\
                    export const Underscore = <_T>(value: number): number => value;\n";

        let rows = unused_rows(text, &CompilerOptions::default());
        assert_eq!(rows.len(), 19);
        assert_eq!(rows.iter().filter(|row| row.0 == 6205).count(), 3);
        assert_eq!(rows.iter().filter(|row| row.0 == 6133).count(), 16);
        assert!(rows
            .iter()
            .all(|row| row.1 == DiagnosticCategory::Suggestion));

        let single_start = text.find("<T> {}").expect("single class list") as u32;
        assert!(rows.iter().any(|row| {
            row.0 == 6133
                && row.2 == single_start
                && row.3 == 3
                && row.4 == "'T' is declared but its value is never read."
        }));
        let multiple_start = text.find("<T, U> {}").expect("multiple class list") as u32;
        assert!(rows.iter().any(|row| {
            row.0 == 6205
                && row.2 == multiple_start
                && row.3 == 6
                && row.4 == "All type parameters are unused."
        }));
        let partial_start =
            text.find("ClassPartial<T, U>").expect("partial class") + "ClassPartial<T, ".len();
        assert!(rows.iter().any(|row| {
            row.0 == 6133
                && row.2 == partial_start as u32
                && row.3 == 1
                && row.4 == "'U' is declared but its value is never read."
        }));

        let local_mode_rows = unused_rows(
            text,
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(local_mode_rows
            .iter()
            .all(|row| row.1 == DiagnosticCategory::Suggestion));

        let parameter_mode_rows = unused_rows(
            text,
            &CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(parameter_mode_rows.len(), 19);
        assert!(parameter_mode_rows
            .iter()
            .all(|row| row.1 == DiagnosticCategory::Error));
    }

    #[test]
    fn unused_type_parameters_honor_trivia_underscores_and_last_merged_declaration() {
        let text = "export class Trivia<T /* kept in aggregate span */> {}\n\
                    export interface LastUnused<T> { value: T; }\n\
                    export interface LastUnused<T> { other: number; }\n\
                    export interface LastUsed<T> { other: number; }\n\
                    export interface LastUsed<T> { value: T; }\n\
                    export function OverloadLastUnused<T>(value: T): T;\n\
                    export function OverloadLastUnused<T>(value: number): number { return value; }\n\
                    export function OverloadLastUsed<T>(value: number): number;\n\
                    export function OverloadLastUsed<T>(value: T): T { return value; }\n\
                    export type Ignored<_T, _U> = number;\n";
        let rows = unused_rows(text, &CompilerOptions::default());
        assert_eq!(rows.len(), 2);

        let trivia = "<T /* kept in aggregate span */>";
        let trivia_start = text.find(trivia).expect("trivia type parameter list") as u32;
        assert_eq!(
            (&rows[0].0, &rows[0].2, &rows[0].3, rows[0].4.as_str()),
            (
                &6133,
                &trivia_start,
                &(trivia.len() as u32),
                "'T' is declared but its value is never read.",
            )
        );

        let last_unused = "OverloadLastUnused<T>(value: number)";
        let last_unused_start = text
            .find(last_unused)
            .expect("last merged overload declaration")
            + "OverloadLastUnused".len();
        assert_eq!(
            (&rows[1].0, &rows[1].2, &rows[1].3, rows[1].4.as_str()),
            (
                &6133,
                &(last_unused_start as u32),
                &3,
                "'T' is declared but its value is never read.",
            )
        );
    }

    #[test]
    fn unused_infer_type_parameters_follow_node_spans_and_parameter_mode() {
        let text = "export type Used<T> = T extends infer U ? U : never;\n\
                    export type Unused<T> = T extends infer U ? string : never;\n\
                    export type Underscore<T> = T extends infer _U ? string : never;\n\
                    export type Repeated<T> = T extends { left: infer U; right: infer U } ? string : never;\n\
                    export type Outside = infer U;\n";
        let expected_starts = [
            text.find("infer U ? string").expect("single unused infer"),
            text.find("infer U; right").expect("first repeated infer"),
            text.find("infer U } ? string")
                .expect("second repeated infer"),
            text.rfind("infer U").expect("outside infer"),
        ];
        for (options, category) in [
            (CompilerOptions::default(), DiagnosticCategory::Suggestion),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
            ),
        ] {
            let rows = unused_rows(text, &options);
            assert_eq!(rows.len(), 4);
            assert_eq!(
                rows.iter()
                    .map(|row| (row.0, row.1, row.2, row.3, row.4.as_str()))
                    .collect::<Vec<_>>(),
                expected_starts
                    .iter()
                    .map(|start| (
                        6133,
                        category,
                        *start as u32,
                        7,
                        "'U' is declared but its value is never read.",
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn unused_private_class_members_follow_reference_and_accessor_anchors() {
        let rows = unused_rows(
            CLASS_PROBE,
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            rows.iter()
                .map(|(code, category, _, _, message)| (*code, *category, message.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'#unused' is declared but its value is never read."
                ),
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'oldUnused' is declared but its value is never read."
                ),
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'#dead' is declared but its value is never read."
                ),
                (
                    6138,
                    DiagnosticCategory::Error,
                    "Property 'dead' is declared but its value is never read."
                ),
            ]
        );
        assert_eq!(
            rows.iter()
                .map(|(_, _, start, length, _)| (*start, *length))
                .collect::<Vec<_>>(),
            [(25, 7), (71, 9), (150, 5), (246, 4)]
        );
    }

    #[test]
    fn unused_class_members_are_suggestions_without_no_unused_locals() {
        for options in [
            CompilerOptions::default(),
            CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        ] {
            let rows = unused_rows(CLASS_PROBE, &options);
            assert_eq!(rows.len(), 4);
            assert!(rows
                .iter()
                .all(|(_, category, _, _, _)| *category == DiagnosticCategory::Suggestion));
        }
    }

    #[test]
    fn private_brand_in_expression_counts_as_a_read() {
        let rows = unused_rows(
            "class C { #unused: undefined; #brand: undefined; has(v: any) { return #brand in v; } }\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 6133);
        assert_eq!(
            rows[0].4,
            "'#unused' is declared but its value is never read."
        );
    }

    #[test]
    fn source_file_locals_group_imports_and_publish_checked_js() {
        let options = CompilerOptions {
            no_unused_locals: Some(true),
            allow_js: true,
            check_js: Some(true),
            ..CompilerOptions::default()
        };
        let rows = unused_rows_for_files(
            &[
                ("dep.ts", "export class A {}\nexport class B {}\n"),
                ("imports.ts", "import { A, B } from './dep';\n"),
                (
                    "locals.ts",
                    "function deadTs() {}\nexport function keptTs() {}\n",
                ),
                (
                    "locals.js",
                    "function deadJs() {}\nexport function keptJs() {}\n",
                ),
            ],
            &options,
        );
        assert_eq!(
            rows,
            [
                (
                    6192,
                    DiagnosticCategory::Error,
                    0,
                    29,
                    "All imports in import declaration are unused.".to_owned(),
                ),
                (
                    6133,
                    DiagnosticCategory::Error,
                    9,
                    6,
                    "'deadJs' is declared but its value is never read.".to_owned(),
                ),
                (
                    6133,
                    DiagnosticCategory::Error,
                    9,
                    6,
                    "'deadTs' is declared but its value is never read.".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn source_file_locals_are_suggestions_by_default() {
        let rows = unused_rows("export {};\nconst dead = 1;\n", &CompilerOptions::default());
        assert_eq!(
            rows,
            [(
                6133,
                DiagnosticCategory::Suggestion,
                17,
                4,
                "'dead' is declared but its value is never read.".to_owned(),
            )]
        );
    }

    #[test]
    fn block_locals_follow_suggestion_and_error_modes() {
        let text = "export {};\nif (true) {\n    const dead = 1;\n}\n";
        for (options, category) in [
            (CompilerOptions::default(), DiagnosticCategory::Suggestion),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options),
                [(
                    6133,
                    category,
                    33,
                    4,
                    "'dead' is declared but its value is never read.".to_owned(),
                )]
            );
        }
    }

    #[test]
    fn block_locals_preserve_reads_and_group_unused_variables() {
        assert!(unused_rows(
            "export {};\nif (true) {\n    const used = 1;\n    void used;\n}\n",
            &CompilerOptions::default(),
        )
        .is_empty());
        assert_eq!(
            unused_rows(
                "export {};\nif (true) {\n    const first = 1, second = 2;\n}\n",
                &CompilerOptions::default(),
            ),
            [(
                6199,
                DiagnosticCategory::Suggestion,
                27,
                28,
                "All variables are unused.".to_owned(),
            )]
        );
    }

    #[test]
    fn module_locals_follow_suggestion_and_error_modes() {
        let text = "export namespace N {\n    const dead = 1;\n}\n";
        for (options, category) in [
            (CompilerOptions::default(), DiagnosticCategory::Suggestion),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options),
                [(
                    6133,
                    category,
                    31,
                    4,
                    "'dead' is declared but its value is never read.".to_owned(),
                )]
            );
        }
    }

    #[test]
    fn module_registration_preserves_exports_reads_and_global_augmentations() {
        assert!(unused_rows(
            "export namespace N {\n    const used = 1;\n    void used;\n    export const publicValue = 2;\n}\n",
            &CompilerOptions::default(),
        )
        .is_empty());
        assert!(unused_rows(
            "export {};\ndeclare global {\n    const ambientGlobal: number;\n}\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn loop_and_case_locals_follow_suggestion_and_error_modes() {
        let text = "export {};\nfor (let deadFor = 0; false;) {}\nfor (const deadOf of [1]) {}\nfor (const deadIn in { key: 1 }) {}\nswitch (0) { case 0: const deadCase = 1; break; }\n";
        for (options, category) in [
            (CompilerOptions::default(), DiagnosticCategory::Suggestion),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, row_category, _, _, message)| {
                        (*code, *row_category, message.as_str())
                    })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        category,
                        "'deadFor' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        category,
                        "'deadOf' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        category,
                        "'deadIn' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        category,
                        "'deadCase' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn loop_and_case_registration_preserves_reads_and_iteration_underscores() {
        assert!(unused_rows(
            "export {};\nfor (let usedFor = 0; usedFor < 1; usedFor++) {}\nfor (const usedOf of [1]) { void usedOf; }\nfor (const usedIn in { key: 1 }) { void usedIn; }\nfor (const _ignored of [1]) {}\nswitch (0) { case 0: const usedCase = 1; void usedCase; }\n",
            &CompilerOptions::default(),
        )
        .is_empty());
    }

    #[test]
    fn function_declaration_locals_and_parameters_use_independent_modes() {
        let text = "export function mixed(deadParameter: number, usedParameter: number) {\n    const deadLocal = 1;\n    return usedParameter;\n}\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn function_declaration_registration_preserves_body_and_parameter_exemptions() {
        assert!(unused_rows(
            "export declare function declared(deadParameter: number): void;\n\
             export function implemented(_ignoredParameter: number, usedParameter: number) {\n\
                 const usedLocal = 1;\n\
                 return usedParameter + usedLocal;\n\
             }\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn function_declaration_shadowed_array_bindings_keep_tsc_spans() {
        let rows = unused_rows(
            "export declare const y: any;\n\
             export function first(x: any) {\n    var [x] = y;\n}\n\
             export function initialized(x: any) {\n    var [x = y] = y;\n}\n\
             export function rest(x: any) {\n    var [...x] = y;\n}\n\
             export function nested(x: any) {\n    var [[x]] = y;\n}\n\
             export function nestedInitialized(x: any) {\n    var [[x] = y] = y;\n}\n\
             export function parameter([x]: [any]) {\n}\n",
            &CompilerOptions::default(),
        );
        assert_eq!(
            rows.iter()
                .map(|(code, category, start, length, _)| { (*code, *category, *start, *length) })
                .collect::<Vec<_>>(),
            [
                (6133, DiagnosticCategory::Suggestion, 51, 1),
                (6133, DiagnosticCategory::Suggestion, 69, 3),
                (6133, DiagnosticCategory::Suggestion, 108, 1),
                (6133, DiagnosticCategory::Suggestion, 126, 7),
                (6133, DiagnosticCategory::Suggestion, 162, 1),
                (6133, DiagnosticCategory::Suggestion, 180, 6),
                (6133, DiagnosticCategory::Suggestion, 217, 1),
                (6133, DiagnosticCategory::Suggestion, 236, 3),
                (6133, DiagnosticCategory::Suggestion, 282, 1),
                (6133, DiagnosticCategory::Suggestion, 301, 3),
                (6133, DiagnosticCategory::Suggestion, 343, 3),
            ]
        );
    }

    #[test]
    fn function_expression_locals_and_parameters_use_independent_modes() {
        let text = "export const assigned = function (deadParameter: number) {\n    const deadLocal = 1;\n};\n\
                    (function (deadIifeParameter: number) {\n    const deadIifeLocal = 1;\n})();\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        parameter_category,
                        "'deadIifeParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadIifeLocal' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn function_expression_registration_preserves_names_reads_and_parameter_exemptions() {
        assert!(unused_rows(
            "export const assigned = function named(\n\
                 _ignoredParameter: number,\n\
                 usedParameter: number,\n\
             ) {\n\
                 const usedLocal = usedParameter;\n\
                 return named && usedLocal;\n\
             };\n\
             (function (_ignoredIifeParameter: number) {})();\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn function_expression_shadowed_parameter_and_local_keep_distinct_kinds() {
        assert_eq!(
            unused_rows(
                "export const shadowed = function (value: number) {\n    var [value] = [1];\n};\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn function_expression_nested_class_uses_the_local_mode() {
        assert_eq!(
            unused_rows(
                "export const nested = function () {\n    class DeadClass {}\n};\n",
                &CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [(
                6196,
                DiagnosticCategory::Error,
                "'DeadClass' is declared but never used.",
            )]
        );
    }

    #[test]
    fn arrow_function_locals_and_parameters_use_independent_modes() {
        let text = "export const mixed = (deadParameter: number, usedParameter: number) => {\n    const deadLocal = 1;\n    return usedParameter;\n};\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn arrow_function_registration_preserves_expression_bodies_and_parameter_exemptions() {
        assert!(unused_rows(
            "export const expression = (_ignoredParameter: number, usedParameter: number) => usedParameter;\n\
             export const block = (usedParameter: number) => {\n\
                 const usedLocal = 1;\n\
                 return usedParameter + usedLocal;\n\
             };\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn arrow_function_checked_js_assignment_local_uses_property_name_anchor() {
        let text = "class D {}\nD.prototype.foo = () => {\n    this.n = 1;\n};\n";
        let expected_start = text.find("n = 1").expect("property name") as u32;
        assert_eq!(
            unused_rows_for_files(
                &[("a.js", text)],
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    ..CompilerOptions::default()
                },
            ),
            [(
                6133,
                DiagnosticCategory::Suggestion,
                expected_start,
                1,
                "'n' is declared but its value is never read.".to_owned(),
            )]
        );
    }

    #[test]
    fn arrow_function_shadowed_parameter_and_local_keep_distinct_kinds() {
        assert_eq!(
            unused_rows(
                "export const shadowed = (value: number) => {\n    var [value] = [1];\n    return 0;\n};\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn method_declaration_locals_and_parameters_use_independent_modes() {
        let text = "export class Container {\n    method(deadParameter: number, usedParameter: number) {\n        const deadLocal = 1;\n        return usedParameter;\n    }\n}\n\
                    export const object = {\n    method(deadObjectParameter: number) {\n        const deadObjectLocal = 1;\n        return 0;\n    },\n};\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        parameter_category,
                        "'deadObjectParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadObjectLocal' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn method_declaration_registration_preserves_overloads_reads_and_parameter_exemptions() {
        assert!(unused_rows(
            "export class Container {\n\
                 overload(deadSignatureParameter: number): void;\n\
                 overload(_ignoredImplementationParameter: number): void {}\n\
                 used(_ignoredParameter: number, usedParameter: number) {\n\
                     const usedLocal = 1;\n\
                     return usedParameter + usedLocal;\n\
                 }\n\
             }\n\
             export const object = {\n\
                 used(_ignoredParameter: number, usedParameter: number) {\n\
                     return usedParameter;\n\
                 },\n\
             };\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn method_declaration_shadowed_parameter_and_local_keep_distinct_kinds() {
        assert_eq!(
            unused_rows(
                "export class Container {\n    shadowed(value: number) {\n        var [value] = [1];\n        return 0;\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn get_accessor_locals_and_parameters_use_independent_modes() {
        let text = "export class Container {\n    get value() {\n        const deadLocal = 1;\n        return 0;\n    }\n}\n\
                    export const Expression = class {\n    get value() {\n        const deadExpressionLocal = 1;\n        return 0;\n    }\n};\n\
                    export const object = {\n    get value() {\n        const deadObjectLocal = 1;\n        return 0;\n    },\n};\n\
                    export class Invalid {\n    get value(deadParameter: number) {\n        return 0;\n    }\n}\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadExpressionLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadObjectLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn get_accessor_registration_preserves_reads_underscores_and_ambient_declarations() {
        assert!(unused_rows(
            "export class Container {\n\
                 get used() {\n\
                     const usedLocal = 1;\n\
                     return usedLocal;\n\
                 }\n\
                 get ignored(_ignoredParameter: number) {\n\
                     return 0;\n\
                 }\n\
             }\n\
             export const object = {\n\
                 get used() {\n\
                     const usedLocal = 1;\n\
                     return usedLocal;\n\
                 },\n\
             };\n\
             export declare class Ambient {\n\
                 get value(): number;\n\
             }\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn get_accessor_shadowed_parameter_and_local_keep_distinct_kinds() {
        assert_eq!(
            unused_rows(
                "export class Container {\n    get value(value: number) {\n        var [value] = [1];\n        return 0;\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn get_accessor_nested_class_uses_the_local_mode() {
        assert_eq!(
            unused_rows(
                "export class Container {\n    get value() {\n        class DeadClass {}\n        return 0;\n    }\n}\n",
                &CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [(
                6196,
                DiagnosticCategory::Error,
                "'DeadClass' is declared but never used.",
            )]
        );
    }

    #[test]
    fn set_accessor_locals_and_parameters_use_independent_modes() {
        let text = "export class Container {\n    set value(deadParameter: number) {\n        const deadLocal = 1;\n    }\n}\n\
                    export const object = {\n    set value(deadObjectParameter: number) {\n        const deadObjectLocal = 1;\n    },\n};\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        parameter_category,
                        "'deadObjectParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadObjectLocal' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn set_accessor_registration_preserves_reads_underscores_and_ambient_declarations() {
        assert!(unused_rows(
            "export class Container {\n\
                 set used(usedParameter: number) {\n\
                     const usedLocal = usedParameter;\n\
                     usedLocal;\n\
                 }\n\
                 set ignored(_ignoredParameter: number) {}\n\
             }\n\
             export const object = {\n\
                 set used(usedParameter: number) {\n\
                     usedParameter;\n\
                 },\n\
             };\n\
             export declare class Ambient {\n\
                 set value(deadSignatureParameter: number);\n\
             }\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn set_accessor_shadowed_parameter_and_local_keep_distinct_kinds() {
        assert_eq!(
            unused_rows(
                "export class Container {\n    set value(value: number) {\n        var [value] = [1];\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn constructor_locals_and_parameters_use_independent_modes() {
        let text = "export class Container {\n    constructor(deadParameter: number) {\n        const deadLocal = 1;\n    }\n}\n\
                    export const Expression = class {\n    constructor(deadExpressionParameter: number) {\n        const deadExpressionLocal = 1;\n    }\n};\n";
        for (options, parameter_category, local_category) in [
            (
                CompilerOptions::default(),
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Suggestion,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Suggestion,
                DiagnosticCategory::Error,
            ),
            (
                CompilerOptions {
                    no_unused_locals: Some(true),
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
                DiagnosticCategory::Error,
                DiagnosticCategory::Error,
            ),
        ] {
            assert_eq!(
                unused_rows(text, &options)
                    .iter()
                    .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
                    .collect::<Vec<_>>(),
                [
                    (
                        6133,
                        parameter_category,
                        "'deadParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadLocal' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        parameter_category,
                        "'deadExpressionParameter' is declared but its value is never read.",
                    ),
                    (
                        6133,
                        local_category,
                        "'deadExpressionLocal' is declared but its value is never read.",
                    ),
                ]
            );
        }
    }

    #[test]
    fn constructor_registration_preserves_overloads_reads_and_parameter_properties() {
        assert!(unused_rows(
            "export class Container {\n\
                 constructor(\n\
                     public publicProperty: number,\n\
                     _ignoredParameter: number,\n\
                     usedParameter: number,\n\
                 ) {\n\
                     const usedLocal = usedParameter;\n\
                     usedLocal;\n\
                 }\n\
             }\n\
             export class Overloaded {\n\
                 constructor(deadSignatureParameter: number);\n\
                 constructor(_ignoredImplementationParameter: number) {}\n\
             }\n\
             export const Expression = class {\n\
                 constructor(usedParameter: number) {\n\
                     usedParameter;\n\
                 }\n\
             };\n",
            &CompilerOptions {
                no_unused_locals: Some(true),
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn destructured_parameter_property_uses_synthetic_symbol_name() {
        let rows = unused_rows(
            "class Container {\n\
                 constructor(private [a, b, c]: [number, string, boolean]) {}\n\
             }\n",
            &CompilerOptions::default(),
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.0 == 6138)
                .map(|row| (row.1, row.4.as_str()))
                .collect::<Vec<_>>(),
            [(
                DiagnosticCategory::Suggestion,
                "Property '__missing' is declared but its value is never read.",
            )]
        );
    }

    #[test]
    fn constructor_shadowed_parameter_and_local_keep_distinct_kinds() {
        assert_eq!(
            unused_rows(
                "export class Container {\n    constructor(value: number) {\n        var [value] = [1];\n    }\n}\n",
                &CompilerOptions {
                    no_unused_parameters: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Error,
                    "'value' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    "'value' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn constructor_nested_class_uses_the_local_mode() {
        assert_eq!(
            unused_rows(
                "export class Container {\n    constructor() {\n        class DeadClass {}\n    }\n}\n",
                &CompilerOptions {
                    no_unused_locals: Some(true),
                    ..CompilerOptions::default()
                },
            )
            .iter()
            .map(|(code, category, _, _, message)| { (*code, *category, message.as_str()) })
            .collect::<Vec<_>>(),
            [(
                6196,
                DiagnosticCategory::Error,
                "'DeadClass' is declared but never used.",
            )]
        );
    }

    #[test]
    fn declaration_file_unused_locals_are_ambient_suggestions() {
        let rows = unused_rows_for_files(
            &[("a.d.ts", "export {};\ndeclare const dead: number;\n")],
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            rows,
            [(
                6133,
                DiagnosticCategory::Suggestion,
                25,
                4,
                "'dead' is declared but its value is never read.".to_owned(),
            )]
        );
    }

    #[test]
    fn ambient_declaration_in_source_file_is_an_unused_suggestion() {
        let text = "export {};\ndeclare const dead: number;\n";
        let rows = unused_rows(
            text,
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            rows,
            [(
                6133,
                DiagnosticCategory::Suggestion,
                text.find("dead").expect("ambient declaration") as u32,
                4,
                "'dead' is declared but its value is never read.".to_owned(),
            )]
        );
    }

    #[test]
    fn jsdoc_links_mark_direct_and_qualified_import_roots_as_referenced() {
        let rows = unused_rows_for_files(
            &[
                ("dep.ts", "export interface A {}\n"),
                (
                    "direct.ts",
                    "import type { A } from './dep';\n/** {@link A} */\nexport interface B {}\n",
                ),
                (
                    "qualified.ts",
                    "import * as ns from './dep';\n/** {@linkplain ns.A details} */\nexport function documented() {}\n",
                ),
            ],
            &CompilerOptions {
                no_unused_locals: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.is_empty());
    }

    #[test]
    fn checked_js_type_queries_and_typedef_merges_mark_source_locals() {
        let rows = unused_rows_for_files(
            &[(
                "a.js",
                "const exemplar = () => 1;\n\
                 /** @param {typeof exemplar} value */\n\
                 export function consume(value) { void value; }\n\
                 /** @typedef {number} Local */\n\
                 var Local = 1;\n",
            )],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn checked_js_property_assignment_function_parameters_remain_unused() {
        let usage = "/** @constructor */
Outer.Pos = function (line, ch) {};
Outer.Used = function (used) { return used; };
/** @type {number} */
Outer.Pos.prototype.line;
var pos = new Outer.Pos(1, 'x');
pos.line;
";
        let rows = unused_rows_for_files(
            &[
                ("module.js", "var Outer = function(element, config) {};\n"),
                ("usage.js", usage),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            rows.iter()
                .filter(|row| {
                    row.4.starts_with("'line'")
                        || row.4.starts_with("'ch'")
                        || row.4.starts_with("'used'")
                })
                .map(|row| (row.0, row.1, row.2, row.3, row.4.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    usage.find("line,").expect("line parameter") as u32,
                    4,
                    "'line' is declared but its value is never read.",
                ),
                (
                    6133,
                    DiagnosticCategory::Suggestion,
                    usage.find("ch)").expect("ch parameter") as u32,
                    2,
                    "'ch' is declared but its value is never read.",
                ),
            ]
        );
    }

    #[test]
    fn checked_js_require_alias_reads_mark_the_source_local() {
        let rows = unused_rows_for_files(
            &[
                (
                    "dep.js",
                    "function Exported() {}\nmodule.exports = Exported;\n",
                ),
                (
                    "index.js",
                    "const Exported = require('./dep');\nExported.member;\nnew Exported;\n",
                ),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn checked_js_jsdoc_types_and_destructured_alias_exports_mark_source_locals() {
        let rows = unused_rows_for_files(
            &[
                (
                    "lib.js",
                    "class SomeClass {}\nmodule.exports = { SomeClass };\n",
                ),
                (
                    "main.js",
                    "const { SomeClass, SomeClass: Another } = require('./lib');\n\
                     /** @param {SomeClass} value */\n\
                     export function consume(value) { void value; }\n\
                     module.exports = { SomeClass, Another };\n",
                ),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(tsrs2_types::ScriptTarget::ES2015.bits()),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn checked_js_contained_reads_mark_nested_and_commonjs_locals() {
        let rows = unused_rows_for_files(
            &[
                (
                    "node.d.ts",
                    "declare var exports: any;\n\
                     declare var module: { exports: any };\n",
                ),
                (
                    "main.js",
                    "/// <reference path='node.d.ts' />\n\
                     exports = module.exports = C;\n\
                     function C() {\n\
                       var x = {};\n\
                       return x;\n\
                     }\n\
                     function build() {\n\
                       const obj = {};\n\
                       return obj;\n\
                     }\n\
                     build();\n",
                ),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(tsrs2_types::ScriptTarget::ES2015.bits()),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn checked_js_reference_reconciliation_preserves_non_reads() {
        let text = "export {};\n\
                    function recursive() { recursive(); }\n\
                    function write() { let assigned; assigned = 1; }\n\
                    function labels() { let marker = 1; marker: { break marker; } }\n\
                    write();\n\
                    labels();\n";
        let rows = unused_rows_for_files(
            &[("main.js", text)],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            rows.iter()
                .map(|row| (row.0, row.4.as_str()))
                .collect::<Vec<_>>(),
            [
                (6133, "'recursive' is declared but its value is never read."),
                (6133, "'assigned' is declared but its value is never read."),
                (6133, "'marker' is declared but its value is never read."),
            ]
        );
    }

    #[test]
    fn unused_jsdoc_import_tag_does_not_become_a_source_local_diagnostic() {
        let rows = unused_rows_for_files(
            &[
                ("types.ts", "export interface Foo { a: number; }\n"),
                ("foo.js", "/** @import x = require(\"types\") */\n"),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(tsrs2_types::ScriptTarget::ES2015.bits()),
                ..CompilerOptions::default()
            },
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn checked_js_conformance_reference_reads_mark_source_locals() {
        let cases = [
            (
                "jsDeclarationsReferenceToClassInstanceCrossFile",
                vec![
                    (
                        "/rectangle.js",
                        r#"class Rectangle {
    constructor() {
        console.log("I'm a rectangle!");
    }
}

module.exports = { Rectangle };
"#,
                    ),
                    (
                        "/index.js",
                        r#"const {Rectangle} = require('./rectangle');

class Render {
    constructor() {
        /**
         * Object list
         * @type {Rectangle[]}
         */
        this.objects = [];
    }
    /**
     * Adds a rectangle
     *
     * @returns {Rectangle} the rect
     */
    addRectangle() {
        const obj = new Rectangle();
        this.objects.push(obj);
        return obj;
    }
}

module.exports = { Render };
"#,
                    ),
                    (
                        "/test.js",
                        r#"const {Render} = require("./index");
let render = new Render();

render.addRectangle();
console.log("Objects", render.objects);
"#,
                    ),
                ],
            ),
            (
                "importTag13",
                vec![
                    ("/types.ts", "export interface Foo {\n    a: number;\n}\n"),
                    ("/foo.js", "/** @import x = require(\"types\") */\n"),
                ],
            ),
            (
                "jsdocTypeReferenceToImportOfFunctionExpression",
                vec![
                    (
                        "/MW.js",
                        r#"/** @typedef {import("./MC")} MC */

class MW {
  /**
   * @param {MC} compiler the compiler
   */
  constructor(compiler) {
    this.compiler = compiler;
  }
}

module.exports = MW;
"#,
                    ),
                    (
                        "/MC.js",
                        r#"const MW = require("./MW");

/** @typedef {number} Meyerhauser */

/** @class */
module.exports = function MC() {
    /** @type {any} */
    var x = {}
    return new MW(x);
};
"#,
                    ),
                ],
            ),
            (
                "moduleExportAlias2",
                vec![
                    (
                        "/node.d.ts",
                        "declare function require(name: string): any;\n\
declare var exports: any;\n\
declare var module: { exports: any };\n",
                    ),
                    (
                        "/semver.js",
                        r#"/// <reference path='node.d.ts' />
exports = module.exports = C
exports.f = n => n + 1
function C() {
    this.p = 1
}
"#,
                    ),
                    (
                        "/index.js",
                        r#"/// <reference path='node.d.ts' />
const C = require("./semver")
var two = C.f(1)
var c = new C
"#,
                    ),
                ],
            ),
            (
                "moduleExportAlias5",
                vec![(
                    "/bug24754.js",
                    r#"// #24754
const webpack = function (){
}
exports = module.exports = webpack;
exports.version = 1001;

webpack.WebpackOptionsDefaulter = 1111;
"#,
                )],
            ),
            (
                "typeFromPropertyAssignment19",
                vec![
                    (
                        "/types.d.ts",
                        "declare var require: any;\ndeclare var module: any;\n",
                    ),
                    (
                        "/semver.js",
                        r#"/// <reference path='./types.d.ts'/>
exports = module.exports = C
C.f = n => n + 1
function C() {
    this.p = 1
}
"#,
                    ),
                    (
                        "/index.js",
                        r#"/// <reference path='./types.d.ts'/>
const C = require("./semver")
var two = C.f(1)
"#,
                    ),
                ],
            ),
        ];
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(tsrs2_types::ScriptTarget::ES2015.bits()),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        };
        let failures = cases
            .into_iter()
            .filter_map(|(name, files)| {
                let rows = unused_rows_with_file_for_files(&files, &options);
                let expected = match name {
                    "moduleExportAlias2" => vec![
                        (
                            "/index.js".to_owned(),
                            6133,
                            DiagnosticCategory::Suggestion,
                            69,
                            3,
                            "'two' is declared but its value is never read.".to_owned(),
                        ),
                        (
                            "/index.js".to_owned(),
                            6133,
                            DiagnosticCategory::Suggestion,
                            86,
                            1,
                            "'c' is declared but its value is never read.".to_owned(),
                        ),
                    ],
                    "typeFromPropertyAssignment19" => vec![(
                        "/index.js".to_owned(),
                        6133,
                        DiagnosticCategory::Suggestion,
                        71,
                        3,
                        "'two' is declared but its value is never read.".to_owned(),
                    )],
                    _ => Vec::new(),
                };
                (rows != expected).then_some((name, expected, rows))
            })
            .collect::<Vec<_>>();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn checked_js_cross_file_unused_registrations_are_source_owned_and_deduplicated() {
        let mw = r#"/** @typedef {import("./MC")} MC */
class MW {
    /** @param {MC} compiler */
    constructor(compiler) {
        this.compiler = compiler;
    }
}
module.exports = MW;
"#;
        let mc = r#"const MW = require("./MW");
/** @class */
module.exports = function MC() {
    var y = 0;
    var x = {};
    return new MW(x);
};
"#;
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(tsrs2_types::ScriptTarget::ES2015.bits()),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        };
        with_program_state(&[("/MW.js", mw), ("/MC.js", mc)], &options, |state| {
            state.check_source_file(0);
            let mc_root = state.binder.source(1).root;
            assert_eq!(
                state
                    .potentially_unused_identifiers
                    .get(&mc_root)
                    .map(Vec::len),
                Some(1),
                "a cross-file forced registration belongs to MC.js"
            );

            state.check_source_file(1);
            assert!(
                !state.potentially_unused_identifiers.contains_key(&mc_root),
                "the owning source-file drain removes its entry"
            );
            let y_start = mc.find("y = 0").expect("fixture y") as u32;
            let rows = state
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 6133)
                .map(|diagnostic| {
                    (
                        diagnostic.file_name.as_deref(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                        diagnostic.message_text(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                rows,
                [(
                    Some("/MC.js"),
                    y_start,
                    1,
                    "'y' is declared but its value is never read.",
                )],
                "x is read and the duplicated function registration emits y only once"
            );

            let diagnostic_count = state.diagnostics.len();
            state.check_source_file(1);
            assert_eq!(
                state.diagnostics.len(),
                diagnostic_count,
                "rechecking a type-checked source file is idempotent"
            );
        });
    }
}

#[cfg(test)]
mod c0_unused_owner_recovery_tests {
    use tsrs2_syntax::SyntaxKind;
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;

    #[test]
    fn unexpected_scope_owner_drains_by_shape_beside_a_known_block_owner() {
        let text = "export {};\n\
                    try {} catch (caught) {}\n\
                    if (true) { const local = 1; }\n";
        let options = CompilerOptions {
            no_unused_locals: Some(true),
            ..CompilerOptions::default()
        };
        with_program_state(&[("unused-owner.ts", text)], &options, |state| {
            let source = state.binder.source(0);
            let catch = source
                .arena
                .node_ids()
                .find(|&node| state.kind_of(node) == SyntaxKind::CatchClause)
                .expect("unexpected but scope-owning recovery canary");
            assert!(
                state.binder.locals_of(catch).is_some(),
                "the fallback must be exercised with a real locals owner"
            );
            let block = source
                .arena
                .node_ids()
                .find(|&node| {
                    state.kind_of(node) == SyntaxKind::Block
                        && state.binder.locals_of(node).is_some()
                })
                .expect("known Block sibling with locals");

            state.register_for_unused_identifiers_check(catch);
            state.register_for_unused_identifiers_check(block);
            state.check_registered_unused_identifiers(source.root);

            assert!(
                state.partially_checked_ranges.is_empty(),
                "neither the shape fallback nor the known sibling is partial"
            );
            assert!(
                state
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message_text().contains("'local'")),
                "the known Block sibling still runs its ordinary unused worker"
            );
        });
    }
}

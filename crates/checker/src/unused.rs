//! M7 8.3/8.4 unused-identifier producers.
//!
//! Workers land by declaration owner. The semantic error surface is
//! activated first under `noUnusedLocals` / `noUnusedParameters`; the
//! same registrations feed the suggestion surface in 8.4.

use tsc_binder::node_util;
use tsc_diagnostics::{gen as diagnostics, DiagnosticCategory};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckResult, CheckerState};

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
        message: &'static tsc_diagnostics::DiagnosticMessage,
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
        message: &'static tsc_diagnostics::DiagnosticMessage,
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
    fn check_unused_class_members(&mut self, node: NodeId) -> CheckResult<()> {
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
    fn check_unused_infer_type_parameter(&mut self, node: NodeId) -> CheckResult<()> {
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
    fn check_unused_type_parameters(&mut self, node: NodeId) -> CheckResult<()> {
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
                        tsc_syntax::skip_trivia(&source.text(), parent_node.pos as usize),
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
        list: tsc_syntax::NodeArrayId,
    ) -> (usize, usize) {
        let source = self.binder.source_of_node(node);
        let array = source.arena.node_array(list);
        let start = (array.pos as usize).saturating_sub(1);
        let end = tsc_syntax::skip_trivia(&source.text(), array.end as usize)
            .saturating_add(1)
            .min(source.text().len());
        (start, end)
    }

    /// tsc-port: isTypeParameterUnused @6.0.3
    /// tsc-hash: d4cc4fc46164e7575e1f9964fbc87191270877a3ab40825f641d9c379c47e8fe
    /// tsc-span: _tsc.js:83067-83069
    /// d2: d2:94ef9c9390a0c96872a6bdc750aa0024387233d4c00c6d7b0c91fcd2a7049e60
    fn is_type_parameter_unused(&mut self, type_parameter: NodeId) -> CheckResult<bool> {
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
    fn check_unused_locals_and_parameters(&mut self, node: NodeId) -> CheckResult<()> {
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
                                    let display = tsc_binder::unescape_leading_underscores(
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
    fn error_unused_local(&mut self, declaration: NodeId, symbol: tsc_types::SymbolId) {
        let node = node_util::get_name_of_declaration(
            self.binder.source_of_node(declaration),
            declaration,
        )
        .unwrap_or(declaration);
        let display =
            tsc_binder::unescape_leading_underscores(&self.binder.symbol(symbol).escaped_name)
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
#[path = "../tests/unit/unused/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/unused/c0_unused_owner_recovery_tests.rs"]
mod c0_unused_owner_recovery_tests;

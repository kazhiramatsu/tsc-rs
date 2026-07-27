//! M7 8.3/8.4 unused-identifier producers.
//!
//! Workers land by declaration owner. The semantic error surface is
//! activated first under `noUnusedLocals` / `noUnusedParameters`; the
//! same registrations feed the suggestion surface in 8.4.

use tsrs2_binder::node_util;
use tsrs2_diags::{gen as diagnostics, DiagnosticCategory};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckResult2, CheckerState, Unsupported};

impl<'a> CheckerState<'a> {
    /// tsc-port: registerForUnusedIdentifiersCheck @6.0.3
    /// tsc-hash: bd4d966695b8aae018cbaea7cf4462c968f8d9672dc8812f6a7b06cbf76fa16f
    /// tsc-span: _tsc.js:82942-82953
    /// d2: d2:08b79e6517d01e5d88bb72d904893471db59fd488401a64241687e5df4e9affe
    ///
    /// The Rust checker stores only the current file's entry and
    /// drains after deferred nodes; this is the eager equivalent of
    /// tsc's addLazyDiagnostic + source-file map.
    pub(crate) fn register_for_unused_identifiers_check(&mut self, node: NodeId) {
        self.potentially_unused_identifiers.push(node);
    }

    /// tsc-port: checkUnusedIdentifiers @6.0.3
    /// tsc-hash: dcbee129b87b48f266b1bc1836718003e82f6b483b3d639b06b2ca7de12cd6df
    /// tsc-span: _tsc.js:82954-82991
    ///
    /// Suggestion/category projection additionally mirrors
    /// getSuggestionDiagnostics (46868-46878) and unusedIsError
    /// (86987-86998).
    ///
    /// Only registered producers can reach this match. The current
    /// SourceFile and Class producers emit Local-kind diagnostics, so
    /// noUnusedLocals selects Error versus Suggestion for the complete
    /// range. Later mixed local/parameter producers must preserve the
    /// callback's per-diagnostic kind when their registrations land.
    pub(crate) fn check_registered_unused_identifiers(&mut self) {
        let nodes = std::mem::take(&mut self.potentially_unused_identifiers);
        for node in nodes {
            if self.contains_parse_error_for_unused(node) {
                continue;
            }
            let diagnostics_before = self.diagnostics.len();
            let result = match self.kind_of(node) {
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                    self.check_unused_class_members(node)
                }
                SyntaxKind::SourceFile => {
                    self.mark_jsdoc_references_for_unused(node);
                    self.mark_checked_js_source_references_for_unused(node);
                    self.check_unused_locals_and_parameters(node)
                }
                SyntaxKind::ModuleDeclaration
                | SyntaxKind::Block
                | SyntaxKind::CaseBlock
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement => self.check_unused_locals_and_parameters(node),
                _ => Ok(()),
            };
            if self.is_ambient_for_unused(node) || self.options.no_unused_locals != Some(true) {
                for diagnostic in &mut self.diagnostics[diagnostics_before..] {
                    diagnostic.message.category = DiagnosticCategory::Suggestion;
                }
            }
            if self.is_in_js_file(node) {
                self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
            }
            if let Err(unsupported) = result {
                self.mark_partially_checked_node(node, unsupported.reason);
            }
        }
    }

    /// Checked-JS CommonJS declarations can bind as aliases or ordinary
    /// destructured variables. Some expression paths consume their
    /// resolved module type or shorthand export without forcing
    /// checkIdentifier on the receiver; tsc's linked-reference pass
    /// still marks the local. Project only matching read sites whose
    /// nearest locals table resolves to that source local, preserving
    /// destructuring, shadowing, and write-only rules.
    fn mark_checked_js_source_references_for_unused(&mut self, root: NodeId) {
        if !self.is_in_js_file(root) {
            return;
        }
        let candidates = self
            .binder
            .locals_of(root)
            .into_iter()
            .flat_map(|locals| locals.values().copied())
            .filter_map(|symbol| {
                let raw = self.binder.symbol(symbol);
                let declaration = if raw.flags.intersects(SymbolFlags::ALIAS) {
                    self.get_declaration_of_alias_symbol(symbol)?
                } else {
                    let declaration = raw.declarations.iter().copied().find(|&declaration| {
                        self.kind_of(declaration) == SyntaxKind::BindingElement
                    })?;
                    let variable =
                        std::iter::successors(Some(declaration), |&node| self.parent_of(node))
                            .find(|&node| self.kind_of(node) == SyntaxKind::VariableDeclaration)?;
                    let initializer = match self.data_of(variable) {
                        NodeData::VariableDeclaration(data) => data.initializer?,
                        _ => return None,
                    };
                    if !self.is_require_call(initializer, true) {
                        return None;
                    }
                    declaration
                };
                if !matches!(
                    self.kind_of(declaration),
                    SyntaxKind::VariableDeclaration | SyntaxKind::BindingElement
                ) {
                    return None;
                }
                Some((raw.escaped_name.clone(), symbol, declaration))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return;
        }

        let mut stack = vec![root];
        let mut identifiers = Vec::new();
        while let Some(node) = stack.pop() {
            if self.kind_of(node) == SyntaxKind::Identifier {
                identifiers.push(node);
            }
            stack.extend(self.children_of(node));
        }
        for (name, symbol, declaration) in candidates {
            let declaration_name = self.name_of_node(declaration);
            for &identifier in &identifiers {
                if Some(identifier) == declaration_name
                    || self.identifier_text_of(identifier) != Some(name.as_str())
                    || self.is_non_reference_identifier_name_for_unused(identifier)
                    || self.is_write_only_access(identifier)
                    || !self.identifier_resolves_to_source_local_for_unused(
                        identifier, &name, symbol, root,
                    )
                {
                    continue;
                }
                self.links
                    .set_symbol_is_referenced(self.speculation_depth, symbol);
                break;
            }
        }
    }

    fn identifier_resolves_to_source_local_for_unused(
        &self,
        node: NodeId,
        escaped_name: &str,
        source_symbol: tsrs2_types::SymbolId,
        root: NodeId,
    ) -> bool {
        let mut current = self.parent_of(node);
        while let Some(scope) = current {
            if let Some(symbol) = self
                .binder
                .locals_of(scope)
                .and_then(|locals| locals.get(escaped_name))
            {
                return *symbol == source_symbol;
            }
            if scope == root {
                break;
            }
            current = self.parent_of(scope);
        }
        false
    }

    fn is_non_reference_identifier_name_for_unused(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::PropertyAccessExpression(data) => data.name == Some(node),
            NodeData::PropertyAssignment(data) => data.name == Some(node),
            NodeData::MethodDeclaration(data) => data.name == Some(node),
            NodeData::GetAccessor(data) => data.name == Some(node),
            NodeData::SetAccessor(data) => data.name == Some(node),
            NodeData::PropertyDeclaration(data) => data.name == Some(node),
            NodeData::PropertySignature(data) => data.name == Some(node),
            NodeData::MethodSignature(data) => data.name == Some(node),
            NodeData::ClassDeclaration(data) => data.name == Some(node),
            NodeData::ClassExpression(data) => data.name == Some(node),
            NodeData::FunctionDeclaration(data) => data.name == Some(node),
            NodeData::FunctionExpression(data) => data.name == Some(node),
            NodeData::InterfaceDeclaration(data) => data.name == Some(node),
            NodeData::TypeAliasDeclaration(data) => data.name == Some(node),
            NodeData::EnumDeclaration(data) => data.name == Some(node),
            NodeData::EnumMember(data) => data.name == Some(node),
            NodeData::ModuleDeclaration(data) => data.name == Some(node),
            NodeData::Parameter(data) => data.name == Some(node),
            NodeData::BindingElement(data) => data.name == Some(node),
            _ => false,
        }
    }

    fn contains_parse_error_for_unused(&self, node: NodeId) -> bool {
        NodeFlags::from_bits(self.node_flags(node))
            .intersects(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
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
                    if !self.links.symbol(symbol).is_referenced
                        && private
                        && !self.is_ambient_for_unused(member)
                    {
                        let display = self.declaration_name_display(name);
                        self.error_at(
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
                        if self.links.symbol(symbol).is_referenced
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
                        let display = self.declaration_name_display(name);
                        self.error_at(
                            Some(name),
                            &diagnostics::Property_0_is_declared_but_its_value_is_never_read,
                            &[&display],
                        );
                    }
                }
                SyntaxKind::IndexSignature
                | SyntaxKind::SemicolonClassElement
                | SyntaxKind::ClassStaticBlockDeclaration => {}
                _ => {
                    return Err(Unsupported::new(
                        "checkUnusedClassMembers unexpected class member (Debug.fail transcription, parse recovery)",
                    ));
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkJSDocLinkLikeTag @6.0.3
    /// tsc-hash: 670de5faef306240a1f40aedcbe389e3bdcb2495dfe29b06ca8086c83118a0af
    /// tsc-span: _tsc.js:82824-82832
    /// d2: d2:7af838c0f26203b767a593b87d8fd7b70f904695e93e35e87d432846ed7799f5
    ///
    /// This also projects checked-JS JSDoc type resolution effects.
    /// The syntax arena does not materialize JSDocLink nodes yet.
    /// Project their root entity names over the existing parser-owned
    /// JSDoc trivia ranges, then apply the same reference effect to
    /// source-file locals. Besides link-like tags, checked-JS `typeof`
    /// type queries and the value declaration paired with a same-name
    /// `@typedef` are observable prerequisites of the unused worker.
    /// Qualified references mark only their root.
    fn mark_jsdoc_references_for_unused(&mut self, root: NodeId) {
        let ranges = self.jsdoc_comment_body_ranges(root);
        let (names, typedef_names) = {
            let source = self.binder.source_of_node(root);
            let mut names = jsdoc_link_root_names(&source.text, &ranges);
            let mut typedef_names = Vec::new();
            if self.is_in_js_file(root) {
                names.extend(jsdoc_type_reference_root_names(&source.text, &ranges));
                typedef_names = jsdoc_typedef_names(&source.text, &ranges);
            }
            (names, typedef_names)
        };
        let symbols = {
            let Some(locals) = self.binder.locals_of(root) else {
                return;
            };
            let mut symbols = names
                .into_iter()
                .filter_map(|name| {
                    locals
                        .get(&tsrs2_binder::escape_leading_underscores(&name))
                        .copied()
                })
                .collect::<Vec<_>>();
            symbols.extend(typedef_names.into_iter().filter_map(|name| {
                let symbol = locals
                    .get(&tsrs2_binder::escape_leading_underscores(&name))
                    .copied()?;
                self.binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .any(|&declaration| {
                        self.kind_of(declaration) == SyntaxKind::VariableDeclaration
                    })
                    .then_some(symbol)
            }));
            symbols
        };
        for symbol in symbols {
            self.links
                .set_symbol_is_referenced(self.speculation_depth, symbol);
        }
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
                if !symbol.flags.intersects(SymbolFlags::VARIABLE) || referenced {
                    continue;
                }
            } else if referenced || symbol.export_symbol.is_some() {
                continue;
            }
            let declarations = symbol.declarations.clone();
            for declaration in declarations {
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
                    let source = self.binder.source_of_node(declaration);
                    let parameter = node_util::get_root_declaration(source, declaration);
                    let name = self
                        .binder
                        .symbol(local)
                        .value_declaration
                        .and_then(|value| self.name_of_node(value));
                    if self.kind_of(parameter) == SyntaxKind::Parameter {
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
                                    self.error_at(
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
                    self.error_at(
                        Some(import_decl),
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                } else {
                    self.error_at(
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
                        .map(|name| self.declaration_name_display(name))
                        .unwrap_or_default();
                    self.error_at(
                        Some(binding_pattern),
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                } else {
                    self.error_at(
                        Some(binding_pattern),
                        &diagnostics::All_destructured_elements_are_unused,
                        &[],
                    );
                }
            } else {
                for element in binding_elements {
                    let display = self
                        .name_of_node(element)
                        .map(|name| self.declaration_name_display(name))
                        .unwrap_or_default();
                    self.error_at(
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
                        .map(|name| self.declaration_name_display(name))
                        .unwrap_or_default();
                    self.error_at(
                        name,
                        &diagnostics::_0_is_declared_but_its_value_is_never_read,
                        &[&display],
                    );
                } else {
                    let range = self
                        .parent_of(declaration_list)
                        .filter(|parent| self.kind_of(*parent) == SyntaxKind::VariableStatement)
                        .unwrap_or(declaration_list);
                    self.error_at(Some(range), &diagnostics::All_variables_are_unused, &[]);
                }
            } else {
                for declaration in declarations {
                    let display = self
                        .name_of_node(declaration)
                        .map(|name| self.declaration_name_display(name))
                        .unwrap_or_default();
                    self.error_at(
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
        let node = self.name_of_node(declaration).unwrap_or(declaration);
        let display =
            tsrs2_binder::unescape_leading_underscores(&self.binder.symbol(symbol).escaped_name)
                .to_owned();
        let message = if self.is_type_declaration_for_unused(declaration) {
            &diagnostics::_0_is_declared_but_never_used
        } else {
            &diagnostics::_0_is_declared_but_its_value_is_never_read
        };
        self.error_at(Some(node), message, &[&display]);
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
}

fn add_to_unused_group(groups: &mut Vec<(NodeId, Vec<NodeId>)>, key: NodeId, value: NodeId) {
    if let Some((_, values)) = groups.iter_mut().find(|(candidate, _)| *candidate == key) {
        values.push(value);
    } else {
        groups.push((key, vec![value]));
    }
}

fn jsdoc_link_root_names(text: &str, ranges: &[(usize, usize)]) -> Vec<String> {
    let mut names = Vec::new();
    for &(start, end) in ranges {
        let Some(body) = text.get(start..end) else {
            continue;
        };
        let mut cursor = 0;
        while let Some(relative) = body[cursor..].find("{@") {
            let tag_start = cursor + relative + 2;
            let tail = &body[tag_start..];
            let tag_len = tail
                .char_indices()
                .take_while(|(_, character)| character.is_ascii_alphabetic())
                .last()
                .map_or(0, |(index, character)| index + character.len_utf8());
            let tag = &tail[..tag_len];
            if matches!(tag, "link" | "linkcode" | "linkplain") {
                let after_tag = &tail[tag_len..];
                let whitespace_len = after_tag
                    .char_indices()
                    .take_while(|(_, character)| matches!(character, ' ' | '\t' | '\r' | '\n'))
                    .last()
                    .map_or(0, |(index, character)| index + character.len_utf8());
                if whitespace_len > 0 {
                    let name_tail = &after_tag[whitespace_len..];
                    let name_len = name_tail
                        .char_indices()
                        .take_while(|(index, character)| {
                            if *index == 0 {
                                *character == '_' || *character == '$' || character.is_alphabetic()
                            } else {
                                *character == '_'
                                    || *character == '$'
                                    || character.is_alphanumeric()
                            }
                        })
                        .last()
                        .map_or(0, |(index, character)| index + character.len_utf8());
                    if name_len > 0 {
                        names.push(name_tail[..name_len].to_owned());
                    }
                }
            }
            cursor = tag_start;
            if cursor >= body.len() {
                break;
            }
        }
    }
    names
}

#[cfg(test)]
fn jsdoc_type_query_root_names(text: &str, ranges: &[(usize, usize)]) -> Vec<String> {
    let mut names = Vec::new();
    for &(start, end) in ranges {
        let Some(body) = text.get(start..end) else {
            continue;
        };

        // JSDoc type expressions are brace-delimited. Resolve the root
        // of each `typeof Name` query, matching TypeQuery's value-use
        // side effect without manufacturing names from prose.
        let mut cursor = 0;
        while let Some(open_relative) = body[cursor..].find('{') {
            let open = cursor + open_relative + 1;
            let Some(close_relative) = body[open..].find('}') else {
                break;
            };
            let close = open + close_relative;
            let type_text = &body[open..close];
            let mut type_cursor = 0;
            while let Some(typeof_relative) = type_text[type_cursor..].find("typeof") {
                let typeof_start = type_cursor + typeof_relative;
                let before_ok = typeof_start == 0
                    || !is_jsdoc_identifier_part(
                        type_text[..typeof_start]
                            .chars()
                            .next_back()
                            .expect("nonzero has a previous char"),
                    );
                let after_keyword = typeof_start + "typeof".len();
                let after_ok = type_text[after_keyword..]
                    .chars()
                    .next()
                    .is_none_or(|character| !is_jsdoc_identifier_part(character));
                if before_ok && after_ok {
                    let tail = type_text[after_keyword..].trim_start_matches(char::is_whitespace);
                    if let Some(name) = jsdoc_root_identifier(tail) {
                        names.push(name.to_owned());
                    }
                }
                type_cursor = after_keyword;
            }
            cursor = close + 1;
        }
    }
    names
}

fn jsdoc_type_reference_root_names(text: &str, ranges: &[(usize, usize)]) -> Vec<String> {
    let mut names = Vec::new();
    for &(start, end) in ranges {
        let Some(body) = text.get(start..end) else {
            continue;
        };
        let mut cursor = 0;
        let mut brace_depth = 0_u32;
        let mut quote = None;
        while cursor < body.len() {
            let Some(character) = body[cursor..].chars().next() else {
                break;
            };
            let character_len = character.len_utf8();
            if let Some(delimiter) = quote {
                if character == '\\' {
                    cursor += character_len;
                    if let Some(escaped) = body[cursor..].chars().next() {
                        cursor += escaped.len_utf8();
                    }
                    continue;
                }
                if character == delimiter {
                    quote = None;
                }
                cursor += character_len;
                continue;
            }
            match character {
                '\'' | '"' | '`' if brace_depth > 0 => {
                    quote = Some(character);
                    cursor += character_len;
                    continue;
                }
                '{' => {
                    brace_depth += 1;
                    cursor += character_len;
                    continue;
                }
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    cursor += character_len;
                    continue;
                }
                _ => {}
            }
            if brace_depth == 0 || !is_jsdoc_identifier_start(character) {
                cursor += character_len;
                continue;
            }

            let identifier_start = cursor;
            cursor += character_len;
            while cursor < body.len() {
                let Some(next) = body[cursor..].chars().next() else {
                    break;
                };
                if !is_jsdoc_identifier_part(next) {
                    break;
                }
                cursor += next.len_utf8();
            }
            let previous = body[..identifier_start]
                .chars()
                .rev()
                .find(|candidate| !candidate.is_whitespace());
            let next = body[cursor..]
                .chars()
                .find(|candidate| !candidate.is_whitespace());
            // A name after `.` is a qualified member, and a name
            // before `:` is a record property. Neither resolves
            // independently in the source-file locals table.
            if previous != Some('.') && next != Some(':') {
                names.push(body[identifier_start..cursor].to_owned());
            }
        }
    }
    names
}

fn jsdoc_typedef_names(text: &str, ranges: &[(usize, usize)]) -> Vec<String> {
    let mut names = Vec::new();
    for &(start, end) in ranges {
        let Some(body) = text.get(start..end) else {
            continue;
        };
        let mut cursor = 0;
        while let Some(tag_relative) = body[cursor..].find("@typedef") {
            let tag_start = cursor + tag_relative;
            let after_tag = tag_start + "@typedef".len();
            let tag_tail = &body[after_tag..];
            let tag_tail = tag_tail.trim_start_matches(char::is_whitespace);
            let name_tail = if let Some(type_tail) = tag_tail.strip_prefix('{') {
                type_tail
                    .find('}')
                    .map(|close| &type_tail[close + 1..])
                    .unwrap_or("")
            } else {
                tag_tail
            };
            if let Some(name) =
                jsdoc_root_identifier(name_tail.trim_start_matches(char::is_whitespace))
            {
                names.push(name.to_owned());
            }
            cursor = after_tag;
        }
    }
    names
}

fn jsdoc_root_identifier(text: &str) -> Option<&str> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if !is_jsdoc_identifier_start(first) {
        return None;
    }
    let end = chars
        .take_while(|(_, character)| is_jsdoc_identifier_part(*character))
        .last()
        .map_or(first.len_utf8(), |(index, character)| {
            index + character.len_utf8()
        });
    text.get(..end)
}

fn is_jsdoc_identifier_start(character: char) -> bool {
    character == '_' || character == '$' || character.is_alphabetic()
}

fn is_jsdoc_identifier_part(character: char) -> bool {
    is_jsdoc_identifier_start(character) || character.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::{
        jsdoc_link_root_names, jsdoc_type_query_root_names, jsdoc_type_reference_root_names,
        jsdoc_typedef_names,
    };
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
                matches!(diagnostic.code(), 6133 | 6138 | 6192 | 6196 | 6198 | 6199)
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

    const CLASS_PROBE: &str = "class C {
  #used = 0;
  #unused = 0;
  private oldUsed = 0;
  private oldUnused = 0;
  get #pair() { return 0; }
  set #pair(value: number) {}
  get #dead() { return 0; }
  set #dead(value: number) {}
  constructor(private live: number, private dead: number) {
    this.#used;
    this.oldUsed;
    this.#pair;
    this.live;
  }
}
";

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
                 export function consume(value) {}\n\
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
                     export function consume(value) {}\n\
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
    fn jsdoc_link_projection_rejects_similar_text_and_keeps_root_names() {
        let text = "{@link A} {@linkcode ns.Member label} {@linkplain $value} {@linkish Wrong}";
        assert_eq!(
            jsdoc_link_root_names(text, &[(0, text.len())]),
            ["A", "ns", "$value"]
        );
    }

    #[test]
    fn jsdoc_type_projection_keeps_type_query_and_typedef_roots() {
        let text = "@param {typeof exemplar.Member} x prose typeof Wrong\n@typedef {number} Local";
        assert_eq!(
            jsdoc_type_query_root_names(text, &[(0, text.len())]),
            ["exemplar"]
        );
        assert_eq!(
            jsdoc_type_reference_root_names(text, &[(0, text.len())]),
            ["typeof", "exemplar", "number"]
        );
        assert_eq!(jsdoc_typedef_names(text, &[(0, text.len())]), ["Local"]);
    }

    #[test]
    fn jsdoc_type_reference_projection_skips_members_and_record_properties() {
        let text = "/** @typedef {{field: Local} | ns.Member | typeof Value} Result */";
        assert_eq!(
            jsdoc_type_reference_root_names(text, &[(3, text.len() - 2)]),
            ["Local", "ns", "typeof", "Value"]
        );
    }
}

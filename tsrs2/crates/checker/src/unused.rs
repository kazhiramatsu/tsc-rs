//! M7 8.3/8.4 unused-identifier producers.
//!
//! Workers land by declaration owner. The semantic error surface is
//! activated first under `noUnusedLocals` / `noUnusedParameters`; the
//! same registrations feed the suggestion surface in 8.4.

use tsrs2_binder::node_util;
use tsrs2_diags::gen as diagnostics;
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

    /// tsrs-native: incremental error-mode projection of tsc
    /// checkUnusedIdentifiers. Only registered producers can reach
    /// this match; later 8.3 slices add their registrations and arms.
    pub(crate) fn check_unused_identifiers_error_mode(&mut self) {
        let nodes = std::mem::take(&mut self.potentially_unused_identifiers);
        for node in nodes {
            if self.contains_parse_error_for_unused(node)
                || self.is_ambient_for_unused(node)
                || self.options.no_unused_locals != Some(true)
            {
                continue;
            }
            let diagnostics_before = self.diagnostics.len();
            let result = match self.kind_of(node) {
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                    self.check_unused_class_members(node)
                }
                SyntaxKind::SourceFile => {
                    self.mark_jsdoc_link_references_for_unused(node);
                    self.check_unused_locals_and_parameters(node)
                }
                _ => Ok(()),
            };
            if self.is_in_js_file(node) {
                self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
            }
            if let Err(unsupported) = result {
                self.mark_partially_checked_node(node, unsupported.reason);
            }
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
    /// The syntax arena does not materialize JSDocLink nodes yet.
    /// Project their root entity names over the existing
    /// parser-owned JSDoc trivia ranges, then apply the same
    /// resolveJSDocMemberName reference effect to source-file locals.
    /// A qualified link marks only its root (`ns` in `ns.Member`),
    /// exactly the symbol the unused-import worker consumes.
    fn mark_jsdoc_link_references_for_unused(&mut self, root: NodeId) {
        let ranges = self.jsdoc_comment_body_ranges(root);
        let names = {
            let source = self.binder.source_of_node(root);
            jsdoc_link_root_names(&source.text, &ranges)
        };
        let symbols = {
            let Some(locals) = self.binder.locals_of(root) else {
                return;
            };
            names
                .into_iter()
                .filter_map(|name| {
                    locals
                        .get(&tsrs2_binder::escape_leading_underscores(&name))
                        .copied()
                })
                .collect::<Vec<_>>()
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
mod tests {
    use super::jsdoc_link_root_names;
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
            .filter(|diagnostic| matches!(diagnostic.code(), 6133 | 6138 | 6192))
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
    fn unused_class_member_errors_require_no_unused_locals() {
        assert!(unused_rows(CLASS_PROBE, &CompilerOptions::default()).is_empty());
        assert!(unused_rows(
            CLASS_PROBE,
            &CompilerOptions {
                no_unused_parameters: Some(true),
                ..CompilerOptions::default()
            },
        )
        .is_empty());
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
    fn jsdoc_link_projection_rejects_similar_text_and_keeps_root_names() {
        let text = "{@link A} {@linkcode ns.Member label} {@linkplain $value} {@linkish Wrong}";
        assert_eq!(
            jsdoc_link_root_names(text, &[(0, text.len())]),
            ["A", "ns", "$value"]
        );
    }
}

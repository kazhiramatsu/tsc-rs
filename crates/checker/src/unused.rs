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
        // IndexSet keeps first-insertion order; a repeated registration is a
        // no-op exactly like the previous `contains` guard.
        self.potentially_unused_identifiers
            .entry(root)
            .or_default()
            .insert(node);
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
                | SyntaxKind::ForOfStatement
                | SyntaxKind::ClassStaticBlockDeclaration => {
                    self.check_unused_locals_and_parameters(node)
                }
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
                | SyntaxKind::JSDocTypedefTag
                | SyntaxKind::JSDocCallbackTag
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
        self.add_unused_diagnostic_at_js(
            containing_node,
            kind,
            location,
            message,
            &args.iter().map(|&arg| arg.into()).collect::<Vec<_>>(),
        );
    }

    fn add_unused_diagnostic_at_js(
        &mut self,
        containing_node: NodeId,
        kind: UnusedIdentifierKind,
        location: Option<NodeId>,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        args: &[tsc_types::JsStr<'_>],
    ) {
        if self.is_recovery_only_unused_declaration(containing_node) {
            return;
        }
        let mut diagnostic = self.create_error_js(location, message, args);
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
            self.diagnostics.update(index, |diagnostic| {
                diagnostic.message.category = DiagnosticCategory::Suggestion;
            });
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
                    if self
                        .links
                        .read_symbol(symbol, |links| links.is_referenced)
                        .is_empty()
                        && private
                        && !NodeFlags::from_bits(self.node_flags(member))
                            .intersects(NodeFlags::AMBIENT)
                    {
                        let display = self.declaration_name_display(name);
                        self.add_unused_diagnostic_at_js(
                            member,
                            UnusedIdentifierKind::Local,
                            Some(name),
                            &diagnostics::_0_is_declared_but_its_value_is_never_read,
                            &[(&display).into()],
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
                        if !self
                            .links
                            .read_symbol(symbol, |links| links.is_referenced)
                            .is_empty()
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
                        self.add_unused_diagnostic_at_js(
                            parameter,
                            UnusedIdentifierKind::Local,
                            Some(name),
                            &diagnostics::Property_0_is_declared_but_its_value_is_never_read,
                            &[(&display).into()],
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

    /// tsgo-port: checkUnusedInferTypeParameter @7.1 (checker.go:7452-7457):
    /// TS6196 at the type parameter's name.
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
        let name_node = self.name_of_node(type_parameter);
        let name = name_node
            .and_then(|name| self.identifier_text_of(name))
            .unwrap_or_default()
            .to_owned();
        self.add_unused_diagnostic_at(
            node,
            UnusedIdentifierKind::Parameter,
            name_node.or(Some(type_parameter)),
            &diagnostics::_0_is_declared_but_never_used,
            &[&name],
        );
        Ok(())
    }

    /// tsgo-port: checkUnusedTypeParameters @7.1 (checker.go:7459-7480).
    /// Every declaration whose symbol's declarations share one source file is
    /// checked (tsc 6.0 checked only the last declaration; merged interface
    /// type parameters share one symbol, overload type parameters do not).
    /// More than one type parameter, all unreferenced, is one TS6205 over the
    /// `<…>` range; any other unreferenced type parameter is TS6196 at its
    /// own node.
    fn check_unused_type_parameters(&mut self, node: NodeId) -> CheckResult<()> {
        let symbol = self.get_symbol_of_declaration(node)?;
        if !self.all_declarations_in_same_source_file(symbol) {
            return Ok(());
        }
        let type_parameters = self.type_parameter_declarations_of(node);
        if type_parameters.is_empty() {
            return Ok(());
        }
        let mut unreferenced = Vec::with_capacity(type_parameters.len());
        for &type_parameter in &type_parameters {
            unreferenced.push(self.is_type_parameter_unused(type_parameter)?);
        }
        if type_parameters.len() > 1 && unreferenced.iter().all(|&unused| unused) {
            // tsgo `rangeOfTypeParameters`: from the character before the list
            // to the one after the last parameter's trailing trivia. JSDoc
            // `@template` parameters are reparsed into a list positioned at the
            // first tag, so a list that came from a tag starts there.
            let source = self.binder.source_of_node(node);
            let first = type_parameters[0];
            let last = source
                .arena
                .node(type_parameters[type_parameters.len() - 1]);
            let (list_start, list_end) = match self.type_parameter_declaration_list_of(node) {
                Some(list) => {
                    let list = source.arena.node_array(list);
                    (list.pos, list.end)
                }
                // A JavaScript list reparsed from `@template` tags spans
                // them, from the first tag to the last.
                None => self
                    .reparsed_type_parameter_range(node)
                    .unwrap_or((source.arena.node(first).pos, last.end)),
            };
            let start_byte = (list_start as usize).saturating_sub(1);
            let end_byte = tsc_syntax::skip_trivia(source.text(), list_end as usize)
                .saturating_add(1)
                .min(source.text().len());
            self.add_unused_diagnostic_at_byte_range(
                node,
                UnusedIdentifierKind::Parameter,
                start_byte,
                end_byte,
                &diagnostics::All_type_parameters_are_unused,
                &[],
            );
            return Ok(());
        }
        for (&type_parameter, &unused) in type_parameters.iter().zip(&unreferenced) {
            if !unused {
                continue;
            }
            let name = self
                .name_of_node(type_parameter)
                .and_then(|name| self.identifier_text_of(name))
                .unwrap_or_default()
                .to_owned();
            self.add_unused_diagnostic_at(
                node,
                UnusedIdentifierKind::Parameter,
                Some(type_parameter),
                &diagnostics::_0_is_declared_but_never_used,
                &[&name],
            );
        }
        Ok(())
    }

    /// tsgo-port: allDeclarationsInSameSourceFile @7.1 (utilities.go:1633-1645)
    fn all_declarations_in_same_source_file(&self, symbol: tsc_types::SymbolId) -> bool {
        let declarations = &self.binder.symbol(symbol).declarations;
        let mut first: Option<&tsc_syntax::SourceFile> = None;
        for &declaration in declarations.iter() {
            let source = self.binder.source_of_node(declaration);
            match first {
                None => first = Some(source),
                Some(seen) if !std::ptr::eq(seen, source) => return false,
                Some(_) => {}
            }
        }
        true
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
            .read_symbol(symbol, |links| links.is_referenced)
            .intersects(SymbolFlags::TYPE_PARAMETER)
            && !self.identifier_starts_with_underscore(name))
    }

    /// tsgo: checkUnusedLocalsAndParameters (checker.go:7312-7352)
    ///
    /// tsgo replaced tsc 6.0's grouping of unused declarations by
    /// the declarations' owners: every variable declaration list or
    /// function-like owning an unreferenced local is walked once, and a
    /// list or binding pattern with more than one declaration reports
    /// the whole when every declaration in it is unreferenced.
    fn check_unused_locals_and_parameters(&mut self, node: NodeId) -> CheckResult<()> {
        let Some(locals) = self.binder.locals_of(node) else {
            return Ok(());
        };
        let locals = locals.values().copied().collect::<Vec<_>>();
        let mut variable_parents = Vec::<NodeId>::new();
        let mut import_clauses = Vec::<(NodeId, Vec<NodeId>)>::new();
        for local in locals {
            let symbol = self.binder.symbol(local);
            let reference_kinds = self.links.read_symbol(local, |links| links.is_referenced);
            let used = if symbol.flags.intersects(SymbolFlags::TYPE_PARAMETER) {
                !symbol.flags.intersects(SymbolFlags::VARIABLE)
                    || reference_kinds.intersects(SymbolFlags::VARIABLE)
            } else {
                !reference_kinds.is_empty()
                    || symbol.export_symbol.is_some()
                    || symbol.flags.intersects(SymbolFlags::MODULE_EXPORTS)
            };
            if used {
                continue;
            }
            let declarations = symbol.declarations.clone();
            for declaration in declarations {
                match self.kind_of(declaration) {
                    SyntaxKind::VariableDeclaration
                    | SyntaxKind::Parameter
                    | SyntaxKind::BindingElement => {
                        let source = self.binder.source_of_node(declaration);
                        let root = node_util::get_root_declaration(source, declaration);
                        if let Some(parent) = self.parent_of(root) {
                            if !variable_parents.contains(&parent) {
                                variable_parents.push(parent);
                            }
                        }
                    }
                    SyntaxKind::ImportClause
                    | SyntaxKind::ImportSpecifier
                    | SyntaxKind::NamespaceImport => {
                        let underscore = self
                            .name_of_node(declaration)
                            .is_some_and(|name| self.identifier_starts_with_underscore(name));
                        if !underscore {
                            if let Some(import_clause) =
                                self.import_clause_from_imported_declaration(declaration)
                            {
                                add_to_unused_group(
                                    &mut import_clauses,
                                    import_clause,
                                    declaration,
                                );
                            }
                        }
                    }
                    // JSDoc parameter and property tags are not declarations
                    // in tsgo, where they are reparsed into parameters and
                    // type members.
                    SyntaxKind::TypeParameter
                    | SyntaxKind::JSDocParameterTag
                    | SyntaxKind::JSDocPropertyTag => {}
                    SyntaxKind::ModuleDeclaration
                        if node_util::is_ambient_module(
                            self.binder.source_of_node(declaration),
                            declaration,
                        ) => {}
                    _ => self.error_unused_local(declaration, local),
                }
            }
        }
        for parent in variable_parents {
            if self.kind_of(parent) == SyntaxKind::VariableDeclarationList {
                self.report_unused_variables(parent)?;
            } else {
                let parameters = self.parameters_of_function(parent);
                self.report_unused_variable_declarations(&parameters)?;
            }
        }
        for (import_clause, unuseds) in import_clauses {
            self.report_unused_imports(import_clause, unuseds);
        }
        Ok(())
    }

    /// tsgo: reportUnusedVariables (checker.go:7358-7365)
    fn report_unused_variables(&mut self, declaration_list: NodeId) -> CheckResult<()> {
        let declarations = match self.data_of(declaration_list) {
            NodeData::VariableDeclarationList(data) => self.nodes_of(data.declarations),
            _ => Vec::new(),
        };
        if declarations.len() > 1 && self.all_unreferenced_variable_declarations(&declarations)? {
            self.report_unused_variable(
                declaration_list,
                declaration_list,
                &diagnostics::All_variables_are_unused,
                &[],
            );
            return Ok(());
        }
        self.report_unused_variable_declarations(&declarations)
    }

    /// tsgo: reportUnusedBindingElements (checker.go:7371-7378)
    fn report_unused_binding_elements(&mut self, binding_pattern: NodeId) -> CheckResult<()> {
        let elements = self.unused_binding_pattern_elements(binding_pattern);
        if elements.len() > 1 && self.all_unreferenced_variable_declarations(&elements)? {
            self.report_unused_variable(
                binding_pattern,
                binding_pattern,
                &diagnostics::All_destructured_elements_are_unused,
                &[],
            );
            return Ok(());
        }
        self.report_unused_variable_declarations(&elements)
    }

    /// tsgo: reportUnusedVariableDeclarations (checker.go:7380-7391)
    fn report_unused_variable_declarations(&mut self, declarations: &[NodeId]) -> CheckResult<()> {
        for &declaration in declarations {
            let Some(name) = self.name_of_node(declaration) else {
                continue;
            };
            if self.is_parameter_property_declaration(declaration)
                || self.kind_of(declaration) == SyntaxKind::Parameter
                    && self.parameter_is_this_keyword(declaration)
            {
                continue;
            }
            if matches!(
                self.kind_of(name),
                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
            ) {
                self.report_unused_binding_elements(name)?;
            } else if self.is_unreferenced_variable_declaration(declaration)? {
                let text = node_util::id_text(self.binder.source_of_node(name), name)
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.declaration_name_display(name));
                self.report_unused_variable(
                    declaration,
                    name,
                    &diagnostics::_0_is_declared_but_its_value_is_never_read,
                    &[&text],
                );
            }
        }
        Ok(())
    }

    fn all_unreferenced_variable_declarations(
        &mut self,
        declarations: &[NodeId],
    ) -> CheckResult<bool> {
        for &declaration in declarations {
            if !self.is_unreferenced_variable_declaration(declaration)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsgo: isUnreferencedVariableDeclaration (checker.go:7393-7418)
    fn is_unreferenced_variable_declaration(&mut self, node: NodeId) -> CheckResult<bool> {
        let Some(name) = self.name_of_node(node) else {
            return Ok(true);
        };
        if matches!(
            self.kind_of(name),
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ) {
            let elements = self.unused_binding_pattern_elements(name);
            return self.all_unreferenced_variable_declarations(&elements);
        }
        if let Some(symbol) = self.get_symbol_of_declaration_opt(node) {
            if self
                .links
                .read_symbol(symbol, |links| links.is_referenced)
                .intersects(SymbolFlags::VARIABLE)
            {
                return Ok(false);
            }
        }
        let parent = self.parent_of(node);
        let object_binding_element = self.kind_of(node) == SyntaxKind::BindingElement
            && parent
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ObjectBindingPattern);
        if object_binding_element {
            // In `{ a, ...b }`, `a` is considered used since it removes a
            // property from `b`.
            let elements = parent.map_or_else(Vec::new, |parent| {
                self.unused_binding_pattern_elements(parent)
            });
            if let Some(&last) = elements.last() {
                let last_is_rest = matches!(
                    self.data_of(last),
                    NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some()
                );
                if node != last && last_is_rest {
                    return Ok(false);
                }
            }
        }
        let underscore_exempt = match self.kind_of(node) {
            SyntaxKind::Parameter => true,
            SyntaxKind::VariableDeclaration => {
                let in_for_in_or_of =
                    parent
                        .and_then(|list| self.parent_of(list))
                        .is_some_and(|statement| {
                            matches!(
                                self.kind_of(statement),
                                SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement
                            )
                        });
                in_for_in_or_of
                    || node_util::get_combined_node_flags(self.binder.source_of_node(node), node)
                        .intersects(NodeFlags::USING)
            }
            SyntaxKind::BindingElement => {
                let property_name = match self.data_of(node) {
                    NodeData::BindingElement(data) => data.property_name,
                    _ => None,
                };
                !(object_binding_element && property_name.is_none())
            }
            _ => false,
        };
        Ok(!(underscore_exempt && self.identifier_starts_with_underscore(name)))
    }

    /// tsgo: reportUnusedVariable (checker.go:7255-7260)
    ///
    /// The parse-error and ambient checks and the error kind read the
    /// declaration that owns a binding element or pattern.
    fn report_unused_variable(
        &mut self,
        location: NodeId,
        error_node: NodeId,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        args: &[&str],
    ) {
        let mut location = location;
        while matches!(
            self.kind_of(location),
            SyntaxKind::BindingElement
                | SyntaxKind::ObjectBindingPattern
                | SyntaxKind::ArrayBindingPattern
        ) {
            let Some(parent) = self.parent_of(location) else {
                break;
            };
            location = parent;
        }
        let kind = if self.kind_of(location) == SyntaxKind::Parameter {
            UnusedIdentifierKind::Parameter
        } else {
            UnusedIdentifierKind::Local
        };
        self.add_unused_diagnostic_at(location, kind, Some(error_node), message, args);
    }

    /// tsgo: reportUnusedImports (checker.go:7420-7437)
    fn report_unused_imports(&mut self, import_clause: NodeId, unuseds: Vec<NodeId>) {
        let declaration_count = self.import_clause_declaration_count(import_clause);
        if declaration_count > 1 && declaration_count == unuseds.len() {
            let Some(import_declaration) = self.parent_of(import_clause) else {
                return;
            };
            self.add_unused_diagnostic_at(
                import_clause,
                UnusedIdentifierKind::Local,
                Some(import_declaration),
                &diagnostics::All_imports_in_import_declaration_are_unused,
                &[],
            );
            return;
        }
        for unused in unuseds {
            let Some(symbol) = self.binder.node_symbol(unused) else {
                continue;
            };
            self.error_unused_local(unused, symbol);
        }
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
            tsc_binder::unescape_leading_underscores(self.binder.symbol(symbol).escaped_name)
                .to_owned();
        let message = if self.is_type_declaration_for_unused(declaration) {
            &diagnostics::_0_is_declared_but_never_used
        } else {
            &diagnostics::_0_is_declared_but_its_value_is_never_read
        };
        self.add_unused_diagnostic_at_js(
            declaration,
            UnusedIdentifierKind::Local,
            Some(node),
            message,
            &[(&display).into()],
        );
    }

    /// tsgo: IsTypeDeclaration (ast/utilities.go). A JSDoc `@import` is
    /// reparsed into a type-only import declaration.
    fn is_type_declaration_for_unused(&self, declaration: NodeId) -> bool {
        match self.kind_of(declaration) {
            SyntaxKind::TypeParameter
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::JSDocTypedefTag
            | SyntaxKind::JSDocCallbackTag => true,
            SyntaxKind::ImportClause => self.is_type_only_import_clause_for_unused(declaration),
            SyntaxKind::ImportSpecifier => self
                .parent_of(declaration)
                .and_then(|named| self.parent_of(named))
                .is_some_and(|clause| self.is_type_only_import_clause_for_unused(clause)),
            _ => false,
        }
    }

    fn is_type_only_import_clause_for_unused(&self, import_clause: NodeId) -> bool {
        matches!(self.data_of(import_clause), NodeData::ImportClause(data) if data.is_type_only)
            || self
                .parent_of(import_clause)
                .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::JSDocImportTag)
    }

    fn identifier_starts_with_underscore(&self, node: NodeId) -> bool {
        self.identifier_text_of(node)
            .is_some_and(|text| text.starts_with('_'))
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

#[cfg(test)]
#[path = "../tests/unit/unused/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/unused/c0_unused_owner_recovery_tests.rs"]
mod c0_unused_owner_recovery_tests;

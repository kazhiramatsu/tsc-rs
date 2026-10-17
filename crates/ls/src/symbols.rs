//! tsgo `ls/symbols.go`: a document's symbols (`textDocument/documentSymbol`),
//! the workspace's symbols matching a query, and the protocol kind of a
//! declaration.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use tsc_diagnostics::spanmap::FEATURE_DOCUMENT_SYMBOLS;

use tsc_binder::assignment::{get_assignment_declaration_kind, AssignmentDeclarationKind};
use tsc_binder::node_util;
use tsc_checker::program::ProgramFileId;
use tsc_checker::state::CheckerState;
use tsc_lsproto::{
    DocumentSymbol, DocumentSymbolResponse, DocumentUri, Location, SymbolInformation,
    SymbolInformationsOrDocumentSymbolsOrNull, SymbolInformationsOrWorkspaceSymbolsOrNull,
    SymbolKind, WorkspaceSymbolResponse,
};
use tsc_program::uri::file_name_to_document_uri;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::utilities::{path_key, script_of, token_start};

/// tsgo's `maxLength` of a document symbol's name.
const MAX_LENGTH: usize = 150;

/// tsgo `getDocumentSymbolsForChildren`'s state: the symbols so far and
/// the names of the functions and classes their expandos may extend.
#[derive(Default)]
struct DocumentSymbols {
    symbols: Vec<DocumentSymbol>,
    expando_targets: std::collections::HashSet<String>,
}

impl LanguageService<'_> {
    /// tsgo `ProvideDocumentSymbols`: the symbols of the file and its
    /// content mapper's supplemental outputs, each name, kind and range
    /// once, as a hierarchy for a client that takes one, else flattened.
    pub fn provide_document_symbols(
        &mut self,
        uri: &DocumentUri,
    ) -> Result<DocumentSymbolResponse, LsError> {
        let file = self.file(uri);
        let symbols = self
            .with_program_request(false, |request| {
                let mut symbols = Vec::new();
                for projection in request.source_projections(file) {
                    let root = request.state.binder.source(projection).root;
                    symbols.extend(request.document_symbols_for_children(root));
                }
                Ok(symbols)
            })?
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let symbols: Vec<DocumentSymbol> = symbols
            .into_iter()
            .filter(|symbol| {
                seen.insert((
                    symbol.name.clone(),
                    symbol.kind.0,
                    (
                        symbol.range.start.line,
                        symbol.range.start.character,
                        symbol.range.end.line,
                        symbol.range.end.character,
                    ),
                ))
            })
            .collect();
        if self
            .capabilities
            .text_document
            .document_symbol
            .hierarchical_document_symbol_support
        {
            return Ok(SymbolInformationsOrDocumentSymbolsOrNull::DocumentSymbols(
                symbols,
            ));
        }
        // A client without hierarchical document symbols takes them flat.
        let mut flat = Vec::new();
        flatten_document_symbols(&symbols, uri, None, &mut flat);
        Ok(SymbolInformationsOrDocumentSymbolsOrNull::SymbolInformations(flat))
    }
}

/// tsgo `flattenDocumentSymbols`: each symbol, then its children with it as
/// their container.
fn flatten_document_symbols(
    symbols: &[DocumentSymbol],
    uri: &DocumentUri,
    container_name: Option<&str>,
    result: &mut Vec<SymbolInformation>,
) {
    for symbol in symbols {
        result.push(SymbolInformation {
            name: symbol.name.clone(),
            kind: symbol.kind,
            tags: symbol.tags.clone(),
            deprecated: symbol.deprecated,
            location: Location {
                uri: uri.clone(),
                range: symbol.range.clone(),
            },
            container_name: container_name.map(str::to_owned),
        });
        if let Some(children) = symbol
            .children
            .as_ref()
            .filter(|children| !children.is_empty())
        {
            flatten_document_symbols(children, uri, Some(&symbol.name), result);
        }
    }
}

impl Request<'_, '_> {
    /// A node's children as tsgo's navigation bar walks them (`children`).
    fn navigation_children(&self, node: NodeId) -> Vec<NodeId> {
        children(self.state.binder.source_of_node(node), node)
    }

    /// tsgo `getDocumentSymbolsForChildren`: the symbols of a node's
    /// children, expandos merged into their targets.
    fn document_symbols_for_children(&mut self, node: NodeId) -> Vec<DocumentSymbol> {
        let mut state = DocumentSymbols::default();
        for child in self.navigation_children(node) {
            self.visit_document_symbol(&mut state, child);
        }
        merge_expandos(state.symbols)
    }

    /// tsgo's `getSymbolsForChildren`: the symbols of a node's children,
    /// with expando targets of their own.
    fn document_symbols_of_children(
        &mut self,
        state: &mut DocumentSymbols,
        node: Option<NodeId>,
    ) -> Vec<DocumentSymbol> {
        let Some(node) = node else {
            return Vec::new();
        };
        let saved_targets = std::mem::take(&mut state.expando_targets);
        let saved_symbols = std::mem::take(&mut state.symbols);
        for child in self.navigation_children(node) {
            self.visit_document_symbol(state, child);
        }
        state.expando_targets = saved_targets;
        std::mem::replace(&mut state.symbols, saved_symbols)
    }

    /// tsgo's `getSymbolsForNode`: the symbols of a node itself.
    fn document_symbols_of_node(
        &mut self,
        state: &mut DocumentSymbols,
        node: Option<NodeId>,
    ) -> Vec<DocumentSymbol> {
        let Some(node) = node else {
            return Vec::new();
        };
        let saved_symbols = std::mem::take(&mut state.symbols);
        self.visit_document_symbol(state, node);
        std::mem::replace(&mut state.symbols, saved_symbols)
    }

    /// tsgo's `addSymbolForNode`.
    fn add_document_symbol(
        &self,
        state: &mut DocumentSymbols,
        node: NodeId,
        name: Option<NodeId>,
        children: Vec<DocumentSymbol>,
    ) {
        if let Some(symbol) = self.new_document_symbol(node, name, children) {
            state.symbols.push(symbol);
        }
    }

    /// Whether a node is an element of a source file's or a block's
    /// statement list — where tsgo's parser keeps the declarations its
    /// reparser makes of JSDoc tags (parseListIndex).
    fn is_statement_list_element(&self, node: NodeId) -> bool {
        self.state
            .binder
            .node_record(node)
            .parent
            .is_some_and(|parent| {
                matches!(
                    self.node_kind(parent),
                    SyntaxKind::SourceFile | SyntaxKind::Block | SyntaxKind::ModuleBlock
                )
            })
    }

    /// The JSDoc hosts in a statement's subtree outside nested blocks, in
    /// the order their nodes finish parsing (children before their parent):
    /// the order tsgo's parser propagates their reparsed declarations out
    /// to the statement's list.
    fn hoisted_jsdoc_hosts(&self, node: NodeId, hosts: &mut Vec<NodeId>) {
        let source = self.state.binder.source_of_node(node);
        let mut children = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            if !matches!(
                self.node_kind(child),
                SyntaxKind::Block | SyntaxKind::ModuleBlock
            ) {
                self.hoisted_jsdoc_hosts(child, hosts);
            }
            if self.state.binder.node_record(child).js_doc.is_some() {
                hosts.push(child);
            }
        }
    }

    /// The symbols of what tsgo's reparser makes of a comment's typedef,
    /// callback and import tags, in tag order.
    fn visit_reparsed_jsdoc_children(&mut self, state: &mut DocumentSymbols, comment: NodeId) {
        let NodeData::JSDoc(data) = &self.state.binder.node_record(comment).data else {
            return;
        };
        for tag in self.node_list(data.tags) {
            let reparsed = match &self.state.binder.node_record(tag).data {
                NodeData::JSDocTypedefTag(typedef) => {
                    typedef.type_expression.and_then(|expression| {
                        match &self.state.binder.node_record(expression).data {
                            NodeData::JSDocTypeExpression(expression) => expression.r#type,
                            _ => None,
                        }
                    })
                }
                NodeData::JSDocCallbackTag(callback) => callback.type_expression,
                NodeData::JSDocImportTag(import) => import.import_clause,
                _ => None,
            };
            if let Some(reparsed) = reparsed {
                self.visit_document_symbol(state, reparsed);
            }
        }
    }

    /// tsgo `getDocumentSymbolsForChildren`'s `visit`: the symbols of a
    /// declaration (with its body's or initializer's as its children), of a
    /// JSDoc typedef or callback, or of an expando assignment to a function
    /// or class; top-level imports have none.
    fn visit_document_symbol(&mut self, state: &mut DocumentSymbols, node: NodeId) {
        let source = self.state.binder.source_of_node(node);
        let record = self.state.binder.node_record(node);
        // tsgo's reparser makes statements of a comment's typedef, callback
        // and import tags (reparseUnhosted), and its parser propagates them
        // out of any list but a source file's or a block's statements
        // (parseListIndex: class and object literal members, parameters,
        // switch clauses) to stand before the statement the host is part
        // of. The parts of those statements that are not reparsed nodes
        // themselves — an inline type literal's members, an import's
        // bindings — get symbols of their own there, before the tags'
        // symbols and the statement's: a host that is such a statement
        // emits them for every comment in its subtree outside nested
        // blocks, inner hosts first (their nodes finish parsing first), and
        // a host inside one emits none (webpack's `JavascriptParser` lists
        // a class-body typedef's members at the top of the file). Only a
        // JavaScript file is reparsed: in a TypeScript file an `@import`'s
        // bindings are no symbols (astro's `vite-plugin-config-alias/
        // index.ts`, whose comment text "CSS @import statements" parses as
        // a tag), while the typedef and callback tags below are (tsgo's
        // navigation bar adds them from any file's JSDoc).
        if source.is_source_file_js() && self.is_statement_list_element(node) {
            let mut hosts = Vec::new();
            self.hoisted_jsdoc_hosts(node, &mut hosts);
            hosts.push(node);
            let mut seen = std::collections::HashSet::new();
            for host in hosts {
                let Some(js_doc) = self.state.binder.node_record(host).js_doc else {
                    continue;
                };
                for comment in self.state.binder.node_array(js_doc).nodes.to_vec() {
                    if seen.insert(comment) {
                        self.visit_reparsed_jsdoc_children(state, comment);
                    }
                }
            }
        }
        if let Some(js_doc) = record.js_doc {
            let comments = self.state.binder.node_array(js_doc).nodes.to_vec();
            for &comment in &comments {
                let NodeData::JSDoc(data) = &self.state.binder.node_record(comment).data else {
                    continue;
                };
                for tag in self.node_list(data.tags) {
                    if matches!(
                        self.node_kind(tag),
                        SyntaxKind::JSDocTypedefTag | SyntaxKind::JSDocCallbackTag
                    ) {
                        self.add_document_symbol(state, tag, None, Vec::new());
                    }
                }
            }
        }
        let kind = record.kind;
        if record
            .parent
            .is_some_and(|parent| self.node_kind(parent) == SyntaxKind::SourceFile)
            && matches!(
                kind,
                SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
            )
        {
            return;
        }
        match kind {
            SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::EnumDeclaration => {
                if matches!(
                    kind,
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                ) {
                    let name = get_declaration_name(source, node);
                    if !name.is_empty() {
                        state.expando_targets.insert(name);
                    }
                }
                let children = self.document_symbols_of_children(state, Some(node));
                self.add_document_symbol(state, node, None, children);
            }
            SyntaxKind::ModuleDeclaration => {
                let interior = interior_module(source, node);
                let children = self.document_symbols_of_children(state, Some(interior));
                self.add_document_symbol(state, node, None, children);
            }
            SyntaxKind::Constructor => {
                let body = node_util::body_of(source, node);
                let children = self.document_symbols_of_children(state, body);
                self.add_document_symbol(state, node, None, children);
                for parameter in crate::utilities::parameters_in(source, node) {
                    if node_util::has_syntactic_modifier(
                        source,
                        parameter,
                        ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
                    ) {
                        self.add_document_symbol(state, parameter, None, Vec::new());
                    }
                }
            }
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor => {
                let name = get_declaration_name(source, node);
                if !name.is_empty() {
                    state.expando_targets.insert(name);
                }
                let body = node_util::body_of(source, node);
                let children = self.document_symbols_of_children(state, body);
                self.add_document_symbol(state, node, None, children);
            }
            SyntaxKind::VariableDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::PropertyDeclaration => {
                let Some(name) = node_util::name_field_of(source, node) else {
                    return;
                };
                if matches!(
                    self.node_kind(name),
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                ) {
                    self.visit_document_symbol(state, name);
                } else {
                    let initializer = node_util::initializer_of(source, node);
                    let children = self.document_symbols_of_children(state, initializer);
                    self.add_document_symbol(state, node, None, children);
                }
            }
            SyntaxKind::SpreadAssignment => {
                let expression = match &record.data {
                    NodeData::SpreadAssignment(data) => data.expression,
                    _ => None,
                };
                self.add_document_symbol(state, node, expression, Vec::new());
            }
            SyntaxKind::MethodSignature
            | SyntaxKind::PropertySignature
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::EnumMember
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ExportSpecifier => {
                self.add_document_symbol(state, node, None, Vec::new());
            }
            SyntaxKind::ImportClause => {
                let NodeData::ImportClause(data) = &record.data else {
                    return;
                };
                // A default import, `import d from "mod"`.
                if let Some(name) = data.name {
                    self.add_document_symbol(state, name, Some(name), Vec::new());
                }
                // Named bindings, `import * as NS from "mod"` or
                // `import { a, b as B } from "mod"`.
                if let Some(bindings) = data.named_bindings {
                    match &self.state.binder.node_record(bindings).data {
                        NodeData::NamespaceImport(_) => {
                            self.add_document_symbol(state, bindings, None, Vec::new());
                        }
                        NodeData::NamedImports(named) => {
                            for element in self.node_list(named.elements) {
                                self.add_document_symbol(state, element, None, Vec::new());
                            }
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::BinaryExpression | SyntaxKind::CallExpression => {
                match get_assignment_declaration_kind(source, node) {
                    AssignmentDeclarationKind::Property
                    | AssignmentDeclarationKind::ObjectDefinePropertyValue => {
                        self.visit_expando_assignment(state, node);
                    }
                    _ => {
                        for child in self.navigation_children(node) {
                            self.visit_document_symbol(state, child);
                        }
                    }
                }
            }
            SyntaxKind::ExportAssignment => {
                let NodeData::ExportAssignment(data) = &record.data else {
                    return;
                };
                if data.is_export_equals == Some(true) {
                    let children = self.document_symbols_of_node(state, data.expression);
                    self.add_document_symbol(state, node, None, children);
                } else {
                    for child in self.navigation_children(node) {
                        self.visit_document_symbol(state, child);
                    }
                }
            }
            _ => {
                for child in self.navigation_children(node) {
                    self.visit_document_symbol(state, child);
                }
            }
        }
    }

    /// The expando part of tsgo's `visit`: `A.b = …`, `A.prototype.b = …`
    /// or `Object.defineProperty(A, "b", …)` of a function or class `A` the
    /// scope declared is a symbol named `A` holding `b`'s (merged into `A`
    /// later); another target's assignment is just visited.
    fn visit_expando_assignment(&mut self, state: &mut DocumentSymbols, node: NodeId) {
        let source = self.state.binder.source_of_node(node);
        let (target, mut target_function, definition, property_name) =
            match &self.state.binder.node_record(node).data {
                NodeData::BinaryExpression(data) => {
                    let Some(target) = data.left else {
                        return;
                    };
                    let (target_function, property_name) =
                        match &self.state.binder.node_record(target).data {
                            NodeData::PropertyAccessExpression(access) => {
                                (access.expression, access.name)
                            }
                            NodeData::ElementAccessExpression(access) => {
                                (access.expression, access.argument_expression)
                            }
                            _ => (None, None),
                        };
                    (Some(target), target_function, data.right, property_name)
                }
                NodeData::CallExpression(data) => {
                    let arguments = self.node_list(data.arguments);
                    (
                        arguments.get(1).copied(),
                        arguments.first().copied(),
                        arguments.get(2).copied(),
                        arguments.get(1).copied(),
                    )
                }
                _ => return,
            };
        if let Some(function) =
            target_function.filter(|&function| is_prototype_expando(source, function))
        {
            target_function = node_util::expression_of(source, function);
            // A prototype assignment starts tracking its target as an expando
            // target.
            if let Some(name) =
                target_function.and_then(|function| node_util::id_text(source, function))
            {
                state.expando_targets.insert(name.to_owned());
            }
        }
        let is_target = target_function.is_some_and(|function| {
            node_util::id_text(source, function)
                .is_some_and(|name| state.expando_targets.contains(name))
        });
        let (Some(target), true) = (target, is_target) else {
            for child in self.navigation_children(node) {
                self.visit_document_symbol(state, child);
            }
            return;
        };
        // tsgo's `startNode(node, targetFunction)` around the property's
        // symbol.
        let saved_targets = std::mem::take(&mut state.expando_targets);
        let saved_symbols = std::mem::take(&mut state.symbols);
        let children = self.document_symbols_of_node(state, definition);
        self.add_document_symbol(state, target, property_name, children);
        let result = std::mem::replace(&mut state.symbols, saved_symbols);
        state.expando_targets = saved_targets;
        self.add_document_symbol(state, node, target_function, result);
    }

    /// tsgo `newDocumentSymbol`: a node's symbol named by `name` (else its
    /// declaration's name, a module's dotted name, `export=`, or the label of
    /// an unnamed node), from its start to its end with its name selected;
    /// none without a name.
    fn new_document_symbol(
        &self,
        node: NodeId,
        name: Option<NodeId>,
        children: Vec<DocumentSymbol>,
    ) -> Option<DocumentSymbol> {
        let source = self.state.binder.source_of_node(node);
        let text_of = source.text();
        let record = self.state.binder.node_record(node);
        let node_start = tsc_syntax::skip_trivia(text_of, record.pos as usize);
        let name = name.or_else(|| node_util::get_name_of_declaration(source, node));
        let (mut text, name_start, name_end);
        if record.kind == SyntaxKind::ModuleDeclaration
            && !node_util::is_ambient_module(source, node)
        {
            text = module_name(source, node);
            let name = name?;
            name_start = tsc_syntax::skip_trivia(text_of, source.arena.node(name).pos as usize);
            let interior = interior_module(source, node);
            name_end = node_util::name_field_of(source, interior)
                .map_or(name_start, |name| source.arena.node(name).end as usize);
        } else if matches!(&record.data, NodeData::ExportAssignment(data) if data.is_export_equals == Some(true))
        {
            text = "export=".to_owned();
            if !node_util::node_is_missing(source, name) {
                let name = name.expect("a name that is not missing");
                name_start = tsc_syntax::skip_trivia(text_of, source.arena.node(name).pos as usize);
                name_end = source.arena.node(name).end as usize;
            } else {
                name_start = node_start;
                name_end = record.end as usize;
            }
        } else if let Some(name) = name {
            text = text_of_name(source, name);
            name_start = tsc_syntax::skip_trivia(text_of, source.arena.node(name).pos as usize)
                .max(node_start);
            name_end = (source.arena.node(name).end as usize).max(node_start);
        } else {
            text = self.unnamed_node_label(node);
            name_start = node_start;
            name_end = node_start;
        }
        if text.is_empty() {
            return None;
        }
        if text.chars().count() > MAX_LENGTH {
            text = text.chars().take(MAX_LENGTH).collect::<String>() + "...";
        }
        // tsgo: a content mapper's output's symbol only where its name maps
        // to one document symbol segment; its range the name's where the
        // node's does not map.
        let script = script_of(source);
        let (selection_range, selection_fidelity) = self.converters.to_lsp_range_for_feature(
            &script,
            name_start..name_end,
            FEATURE_DOCUMENT_SYMBOLS,
        );
        if !selection_fidelity.is_single_segment() {
            return None;
        }
        let (mut range, range_fidelity) = self.converters.to_lsp_range_for_feature(
            &script,
            node_start..record.end as usize,
            FEATURE_DOCUMENT_SYMBOLS,
        );
        if range_fidelity.is_none() {
            range = selection_range.clone();
        }
        Some(DocumentSymbol {
            name: text,
            detail: None,
            kind: symbol_kind_from_node(self, node),
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: Some(children),
        })
    }

    /// tsgo `getUnnamedNodeLabel`: `export=` or `default` for an exported
    /// expression, `default` for a default-exported function or class, a
    /// callback's `f("literal") callback`, `<function>`, `<class>`,
    /// `constructor`, `()`, `new()` or `[]`; empty for anything else.
    fn unnamed_node_label(&self, node: NodeId) -> String {
        let source = self.state.binder.source_of_node(node);
        let mut parent = self.parent_of(node);
        while let Some(current) =
            parent.filter(|&current| self.node_kind(current) == SyntaxKind::ParenthesizedExpression)
        {
            parent = self.parent_of(current);
        }
        if let Some(parent) = parent {
            if let NodeData::ExportAssignment(data) = &self.state.binder.node_record(parent).data {
                return if data.is_export_equals == Some(true) {
                    "export="
                } else {
                    "default"
                }
                .to_owned();
            }
        }
        let is_default = || node_util::has_syntactic_modifier(source, node, ModifierFlags::DEFAULT);
        match self.node_kind(node) {
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction => {
                if is_default() {
                    return "default".to_owned();
                }
                if let Some(call) = self
                    .parent_of(node)
                    .filter(|&parent| self.node_kind(parent) == SyntaxKind::CallExpression)
                {
                    let callee = node_util::expression_of(source, call);
                    let name = callee
                        .map(|callee| call_expression_name(source, callee))
                        .unwrap_or_default();
                    if !name.is_empty() {
                        let name = clean_callback_text(&name);
                        if name.len() > MAX_LENGTH {
                            return name + " callback";
                        }
                        let arguments =
                            clean_callback_text(&self.call_expression_literal_arguments(call));
                        return format!("{name}({arguments}) callback");
                    }
                }
                "<function>".to_owned()
            }
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                if is_default() {
                    "default".to_owned()
                } else {
                    "<class>".to_owned()
                }
            }
            SyntaxKind::Constructor => "constructor".to_owned(),
            SyntaxKind::CallSignature => "()".to_owned(),
            SyntaxKind::ConstructSignature => "new()".to_owned(),
            SyntaxKind::IndexSignature => "[]".to_owned(),
            _ => String::new(),
        }
    }

    /// tsgo `getCallExpressionLiteralArgs`: the call's string literal and
    /// template arguments' text, joined.
    fn call_expression_literal_arguments(&self, call: NodeId) -> String {
        let source = self.state.binder.source_of_node(call);
        let NodeData::CallExpression(data) = &source.arena.node(call).data else {
            return String::new();
        };
        self.node_list(data.arguments)
            .into_iter()
            .filter(|&argument| {
                matches!(
                    self.node_kind(argument),
                    SyntaxKind::StringLiteral
                        | SyntaxKind::NoSubstitutionTemplateLiteral
                        | SyntaxKind::TemplateExpression
                )
            })
            .map(|argument| text_of_node(source, argument).to_owned())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// tsgo `isPrototypeExpando`: `f.prototype` (or `f["prototype"]`).
fn is_prototype_expando(source: &SourceFile, target: NodeId) -> bool {
    match &source.arena.node(target).data {
        NodeData::PropertyAccessExpression(access) => access
            .name
            .and_then(|name| node_util::id_text(source, name))
            == Some("prototype"),
        NodeData::ElementAccessExpression(access) => access.argument_expression.is_some_and(|argument| {
            matches!(&source.arena.node(argument).data, NodeData::StringLiteral(literal) if literal.text == "prototype")
        }),
        _ => false,
    }
}

/// tsgo `mergeExpandos`: the symbols with each expando (a property named
/// like a class, function or variable) merged into those of its name, and
/// each namespace into the first of its name; children merged first.
fn merge_expandos(symbols: Vec<DocumentSymbol>) -> Vec<DocumentSymbol> {
    let mut expando_targets: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut namespaces: BTreeMap<String, usize> = BTreeMap::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if is_anonymous_name(&symbol.name) {
            continue;
        }
        if matches!(
            symbol.kind,
            SymbolKind::CLASS | SymbolKind::FUNCTION | SymbolKind::VARIABLE
        ) {
            expando_targets
                .entry(symbol.name.clone())
                .or_default()
                .push(index);
        }
        if symbol.kind == SymbolKind::NAMESPACE {
            namespaces.entry(symbol.name.clone()).or_insert(index);
        }
    }
    let mut symbols: Vec<Option<DocumentSymbol>> = symbols.into_iter().map(Some).collect();
    for index in 0..symbols.len() {
        let Some(mut symbol) = symbols[index].take() else {
            continue;
        };
        if let Some(children) = symbol.children.take() {
            symbol.children = Some(merge_expandos(children));
        }
        // Anonymous symbols never merge.
        let mut merged = false;
        if !is_anonymous_name(&symbol.name) {
            if symbol.kind == SymbolKind::PROPERTY {
                if let Some(targets) = expando_targets.get(&symbol.name) {
                    for &target in targets.iter().rev() {
                        if let Some(target) = symbols[target].as_mut() {
                            merge_children(target, &symbol);
                        }
                        merged = true;
                    }
                }
            }
            if symbol.kind == SymbolKind::NAMESPACE {
                if let Some(&target) = namespaces
                    .get(&symbol.name)
                    .filter(|&&target| target != index)
                {
                    if let Some(target) = symbols[target].as_mut() {
                        merge_children(target, &symbol);
                    }
                    merged = true;
                }
            }
        }
        if !merged {
            symbols[index] = Some(symbol);
        }
    }
    symbols.into_iter().flatten().collect()
}

/// tsgo `mergeChildren`: the source's children added to the target's,
/// merged and in range order.
fn merge_children(target: &mut DocumentSymbol, source: &DocumentSymbol) {
    let Some(source_children) = &source.children else {
        return;
    };
    let mut children = target.children.take().unwrap_or_default();
    children.extend(source_children.iter().cloned());
    let mut children = merge_expandos(children);
    children.sort_by(|left, right| {
        let position = |position: &tsc_lsproto::Position| (position.line, position.character);
        position(&left.range.start)
            .cmp(&position(&right.range.start))
            .then_with(|| position(&left.range.end).cmp(&position(&right.range.end)))
    });
    target.children = Some(children);
}

/// tsgo `isAnonymousName` (`getUnnamedNodeLabel`'s labels).
fn is_anonymous_name(name: &str) -> bool {
    matches!(
        name,
        "<function>" | "<class>" | "export=" | "default" | "constructor" | "()" | "new()" | "[]"
    ) || name.ends_with(") callback")
}

/// tsgo `getTextOfName`: an identifier's or number's text, a string's or
/// template's escaped and quoted, a computed name's literal's; else the
/// node's source text.
fn text_of_name(source: &SourceFile, node: NodeId) -> String {
    match &source.arena.node(node).data {
        NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => {
            node_util::id_text(source, node)
                .unwrap_or_default()
                .to_owned()
        }
        NodeData::NumericLiteral(data) => data.text.clone(),
        NodeData::StringLiteral(data) => format!(
            "\"{}\"",
            tsc_emitter::escape_string(&data.text.to_utf16(), tsc_emitter::LiteralQuote::Double)
        ),
        NodeData::NoSubstitutionTemplateLiteral(data) => format!(
            "`{}`",
            tsc_emitter::escape_string(&data.text.to_utf16(), tsc_emitter::LiteralQuote::Backtick)
        ),
        NodeData::ComputedPropertyName(data)
            if data.expression.is_some_and(|expression| {
                node_util::is_string_or_numeric_literal_like(source, expression)
            }) =>
        {
            text_of_name(source, data.expression.expect("a literal expression"))
        }
        _ => text_of_node(source, node).to_owned(),
    }
}

/// tsgo `scanner.GetTextOfNode`: a node's source text from its token start.
fn text_of_node(source: &SourceFile, node: NodeId) -> &str {
    let end = source.arena.node(node).end as usize;
    &source.text()[token_start(source, node)..end]
}

/// tsgo `getCallExpressionName`: an identifier's text, or a property
/// access's dotted names.
fn call_expression_name(source: &SourceFile, node: NodeId) -> String {
    match &source.arena.node(node).data {
        NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => {
            node_util::id_text(source, node)
                .unwrap_or_default()
                .to_owned()
        }
        NodeData::PropertyAccessExpression(access) => {
            let left = access
                .expression
                .map(|expression| call_expression_name(source, expression))
                .unwrap_or_default();
            let right = access
                .name
                .map(|name| call_expression_name(source, name))
                .unwrap_or_default();
            if left.is_empty() {
                right
            } else {
                format!("{left}.{right}")
            }
        }
        _ => String::new(),
    }
}

/// tsgo `cleanCallbackText`: cut to the maximum length (with `...`), line
/// breaks removed.
fn clean_callback_text(text: &str) -> String {
    let mut text = text.to_owned();
    if text.chars().count() > MAX_LENGTH {
        text = text.chars().take(MAX_LENGTH).collect::<String>() + "...";
    }
    text.chars()
        .filter(|&ch| !tsc_syntax::is_line_break(ch))
        .collect()
}

/// tsgo `getInteriorModule`: the innermost of a dotted namespace's modules.
fn interior_module(source: &SourceFile, mut node: NodeId) -> NodeId {
    while let Some(body) = node_util::body_of(source, node)
        .filter(|&body| source.arena.node(body).kind == SyntaxKind::ModuleDeclaration)
    {
        node = body;
    }
    node
}

/// tsgo `getModuleName`: a dotted namespace's names joined with dots.
fn module_name(source: &SourceFile, mut node: NodeId) -> String {
    let name_text = |node: NodeId| {
        node_util::name_field_of(source, node)
            .and_then(|name| node_util::id_text(source, name))
            .unwrap_or_default()
            .to_owned()
    };
    let mut result = name_text(node);
    while let Some(body) = node_util::body_of(source, node)
        .filter(|&body| source.arena.node(body).kind == SyntaxKind::ModuleDeclaration)
    {
        node = body;
        result.push('.');
        result.push_str(&name_text(node));
    }
    result
}

/// A declaration matching a workspace symbol query (tsgo
/// `DeclarationInfo`), with its answer.
pub struct WorkspaceSymbolMatch {
    /// The file's path (tsgo `PathKey`).
    path: String,
    name: String,
    score: usize,
    position: u32,
    /// None for a content mapper's output's name without a counterpart in
    /// its original file (left out after the cut, as tsgo does).
    symbol: Option<SymbolInformation>,
}

impl LanguageService<'_> {
    /// tsgo `ProvideWorkspaceSymbols`' part of one program: the matches of
    /// its files (the declaration files only of a program with TypeScript
    /// files; no library or `node_modules` file when the preferences leave
    /// them out), by file path.
    pub fn workspace_symbol_matches(
        &mut self,
        query: &str,
    ) -> Result<Vec<(String, Vec<WorkspaceSymbolMatch>)>, LsError> {
        let exclude_library_symbols =
            self.user_preferences().exclude_library_symbols_in_nav_to == Some(true);
        let matches = self.with_program_request(false, |request| {
            let file_count = request.state.binder.file_count();
            let has_ts_file = (0..file_count).any(|file| {
                has_implementation_ts_file_extension(
                    &request
                        .state
                        .binder
                        .source(file)
                        .file_name
                        .to_string_lossy(),
                )
            });
            let mut files = Vec::new();
            for file in 0..file_count {
                let source = request.state.binder.source(file);
                let file_name = source.file_name.to_string_lossy().into_owned();
                let library = request
                    .state
                    .binder
                    .file_facts(ProgramFileId::from_raw(file as u32))
                    .is_default_library();
                let excluded = exclude_library_symbols
                    && (file_name.to_lowercase().contains("/node_modules/") || library);
                // A program without TypeScript files leaves out declaration files.
                let included = has_ts_file || !source.is_declaration_file;
                if !included || excluded {
                    continue;
                }
                let mut matches = Vec::new();
                for (name, declarations) in declaration_map(request.state, source) {
                    let Some(score) = match_score(&name, query) else {
                        continue;
                    };
                    for declaration in declarations {
                        matches.push(WorkspaceSymbolMatch {
                            path: path_key(request.prepared, &file_name),
                            name: name.clone(),
                            score,
                            position: source.arena.node(declaration).pos,
                            symbol: request.workspace_symbol_information(
                                source,
                                &name,
                                declaration,
                            ),
                        });
                    }
                }
                files.push((path_key(request.prepared, &file_name), matches));
            }
            Ok(files)
        })?;
        Ok(matches.unwrap_or_default())
    }
}

/// tsgo `ProvideWorkspaceSymbols`: the matches of every program's files (a
/// file's last program's), by score, name, file and position; the first
/// 256.
pub fn workspace_symbols(
    programs: Vec<Vec<(String, Vec<WorkspaceSymbolMatch>)>>,
) -> WorkspaceSymbolResponse {
    let mut files: BTreeMap<String, Vec<WorkspaceSymbolMatch>> = BTreeMap::new();
    for program in programs {
        for (path, matches) in program {
            files.insert(path, matches);
        }
    }
    let mut matches: Vec<WorkspaceSymbolMatch> = files.into_values().flatten().collect();
    matches.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| compare_strings_case_insensitive(&left.name, &right.name))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.position.cmp(&right.position))
    });
    matches.truncate(256);
    SymbolInformationsOrWorkspaceSymbolsOrNull::SymbolInformations(
        matches
            .into_iter()
            .filter_map(|found| found.symbol)
            .collect(),
    )
}

impl Request<'_, '_> {
    /// A workspace symbol's answer: its name's location, kind and
    /// container.
    fn workspace_symbol_information(
        &self,
        source: &SourceFile,
        name: &str,
        declaration: NodeId,
    ) -> Option<SymbolInformation> {
        let container_name = node_util::get_container_node(source, declaration)
            .map(|container| get_declaration_name(source, container))
            .filter(|name| !name.is_empty());
        // Use the name node's span so that VS selects just the symbol name (matching
        // the TS5 navto behaviour). GetNameOfDeclaration is always non-nil here because
        // computeDeclarationMap only adds declarations whose GetDeclarationName (string
        // form) is non-empty, which implies a name node exists.
        let name_node = node_util::get_name_of_declaration(source, declaration)
            .expect("a declaration with a name");
        let range = token_start(source, name_node)..source.arena.node(name_node).end as usize;
        // tsgo `ToLSPLocationForFeature`: a content mapper's output's name
        // in its original file, none where the name has no counterpart.
        let script = script_of(source);
        let (range, fidelity) =
            self.converters
                .to_lsp_range_for_feature(&script, range, FEATURE_DOCUMENT_SYMBOLS);
        if !fidelity.is_single_segment() {
            return None;
        }
        Some(SymbolInformation {
            name: name.to_owned(),
            kind: symbol_kind_from_node(self, declaration),
            tags: None,
            container_name,
            deprecated: None,
            location: Location {
                uri: DocumentUri(file_name_to_document_uri(
                    crate::lsconv::Script::original_file_name(&script),
                )),
                range,
            },
        })
    }
}

/// tsgo `SourceFile.computeDeclarationMap`: a file's declarations by name
/// (an overload group once, by its implementation when it has one).
fn declaration_map(state: &CheckerState<'_>, source: &SourceFile) -> Vec<(String, Vec<NodeId>)> {
    let mut map = DeclarationMap {
        state,
        source,
        names: Vec::new(),
    };
    for child in children(source, source.root) {
        map.visit(child);
    }
    map.names
}

struct DeclarationMap<'s, 'c> {
    state: &'s CheckerState<'c>,
    source: &'s SourceFile,
    names: Vec<(String, Vec<NodeId>)>,
}

impl DeclarationMap<'_, '_> {
    fn entry(&mut self, name: String) -> &mut Vec<NodeId> {
        let index = match self.names.iter().position(|(known, _)| *known == name) {
            Some(index) => index,
            None => {
                self.names.push((name, Vec::new()));
                self.names.len() - 1
            }
        };
        &mut self.names[index].1
    }

    fn add(&mut self, declaration: NodeId) {
        let name = get_declaration_name(self.source, declaration);
        if !name.is_empty() {
            self.entry(name).push(declaration);
        }
    }

    fn visit_children(&mut self, node: NodeId) {
        for child in children(self.source, node) {
            self.visit(child);
        }
    }

    fn visit(&mut self, node: NodeId) {
        let source = self.source;
        let data = &source.arena.node(node).data;
        match source.arena.node(node).kind {
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature => {
                let name = get_declaration_name(source, node);
                if !name.is_empty() {
                    let state = self.state;
                    let declarations = self.entry(name);
                    match declarations.last().copied() {
                        // Check whether this declaration belongs to an "overload group".
                        Some(last)
                            if source.arena.node(node).parent == source.arena.node(last).parent
                                && state.binder.node_symbol(node)
                                    == state.binder.node_symbol(last) =>
                        {
                            // Overwrite the last declaration if it was an overload and this one is an implementation.
                            if node_util::body_of(source, node).is_some()
                                && node_util::body_of(source, last).is_none()
                            {
                                *declarations.last_mut().expect("a last declaration") = node;
                            }
                        }
                        _ => declarations.push(node),
                    }
                }
                self.visit_children(node);
            }
            SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportClause
            | SyntaxKind::NamespaceImport
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::TypeLiteral => {
                self.add(node);
                self.visit_children(node);
            }
            SyntaxKind::ImportSpecifier | SyntaxKind::ExportSpecifier => {
                let property_name = match data {
                    NodeData::ImportSpecifier(data) => data.property_name,
                    NodeData::ExportSpecifier(data) => data.property_name,
                    _ => None,
                };
                if property_name.is_some() {
                    self.add(node);
                }
            }
            SyntaxKind::Parameter
                if !node_util::has_syntactic_modifier(
                    source,
                    node,
                    ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
                ) =>
            {
                // Only consider parameter properties
            }
            SyntaxKind::Parameter
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::BindingElement => {
                if let Some(name) = node_util::name_field_of(source, node) {
                    if node_util::is_binding_pattern(source, name) {
                        self.visit_children(name);
                    } else {
                        if let Some(initializer) = node_util::initializer_of(source, node) {
                            self.visit(initializer);
                        }
                        self.add(node);
                    }
                }
            }
            SyntaxKind::EnumMember
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature => {
                self.add(node);
            }
            SyntaxKind::ExportDeclaration => {
                // Handle named exports case e.g.:
                //    export {a, b as B} from "mod";
                let NodeData::ExportDeclaration(export) = data else {
                    return;
                };
                if let Some(clause) = export.export_clause {
                    match &source.arena.node(clause).data {
                        NodeData::NamedExports(named) => {
                            for element in list(source, named.elements) {
                                self.visit(element);
                            }
                        }
                        NodeData::NamespaceExport(namespace) => {
                            if let Some(name) = namespace.name {
                                self.visit(name);
                            }
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::ImportDeclaration => {
                let NodeData::ImportDeclaration(import) = data else {
                    return;
                };
                let Some(clause) = import.import_clause else {
                    return;
                };
                let NodeData::ImportClause(clause) = &source.arena.node(clause).data else {
                    return;
                };
                // Handle default import case e.g.:
                //    import d from "mod";
                if let Some(name) = clause.name {
                    self.add(name);
                }
                // Handle named bindings in imports e.g.:
                //    import * as NS from "mod";
                //    import {a, b as B} from "mod";
                if let Some(bindings) = clause.named_bindings {
                    match &source.arena.node(bindings).data {
                        NodeData::NamespaceImport(_) => self.add(bindings),
                        NodeData::NamedImports(named) => {
                            for element in list(source, named.elements) {
                                self.visit(element);
                            }
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::BinaryExpression => {
                if matches!(
                    get_assignment_declaration_kind(source, node),
                    AssignmentDeclarationKind::ExportsProperty
                        | AssignmentDeclarationKind::ThisProperty
                        | AssignmentDeclarationKind::Property
                ) {
                    self.add(node);
                }
                self.visit_children(node);
            }
            _ => self.visit_children(node),
        }
    }
}

/// tsgo `ast.GetDeclarationName`: the text of a declaration's own name (a
/// computed name's literal or property name); empty without one.
pub(crate) fn get_declaration_name(source: &SourceFile, declaration: NodeId) -> String {
    let Some(name) = node_util::get_non_assigned_name_of_declaration(source, declaration) else {
        return String::new();
    };
    match &source.arena.node(name).data {
        NodeData::ComputedPropertyName(computed) => {
            let Some(expression) = computed.expression else {
                return String::new();
            };
            if node_util::is_string_or_numeric_literal_like(source, expression) {
                return node_util::try_get_text_of_property_name(source, expression)
                    .unwrap_or_default();
            }
            match &source.arena.node(expression).data {
                NodeData::PropertyAccessExpression(access) => access
                    .name
                    .and_then(|name| node_util::id_text(source, name))
                    .unwrap_or_default()
                    .to_owned(),
                _ => String::new(),
            }
        }
        _ if is_property_name(source.arena.node(name).kind) => {
            node_util::try_get_text_of_property_name(source, name).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// tsgo `ast.IsPropertyName`.
fn is_property_name(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::ComputedPropertyName
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::BigIntLiteral
    )
}

/// tsgo `getMatchScore`: the characters of `name` the query's characters
/// (in order; an upper case one exactly, a lower case one in either case)
/// skip; none when they are not all found.
fn match_score(name: &str, query: &str) -> Option<usize> {
    let mut score = 0;
    let mut characters = name.chars();
    for query_character in query.chars() {
        let exact = query_character.is_uppercase();
        loop {
            let character = characters.next()?;
            if exact && character == query_character
                || !exact && character.to_lowercase().eq(query_character.to_lowercase())
            {
                break;
            }
            score += 1;
        }
    }
    Some(score)
}

/// tsgo `stringutil.CompareStringsCaseInsensitive`: by the characters'
/// lower case forms.
fn compare_strings_case_insensitive(left: &str, right: &str) -> Ordering {
    left.chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase))
}

/// tsgo `HasImplementationTSFileExtension`.
fn has_implementation_ts_file_extension(file_name: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| file_name.ends_with(extension))
        && !tsc_program::is_declaration_file_name(file_name)
}

/// A node's children (tsgo `ForEachChild`).
/// A node's children as tsgo's navigation bar walks them: with the JSDoc
/// types tsgo's reparser gives a JavaScript file's nodes — a parameter's
/// `@param` type after its name, and the type of a `/** @type {T} */ (x)`
/// cast (tsgo's `x as T`) after the expression — so an inline type
/// literal's members get symbols.
fn children(source: &SourceFile, node: NodeId) -> Vec<NodeId> {
    let mut children = Vec::new();
    tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
        children.push(child);
        false
    });
    if !source.is_source_file_js() {
        return children;
    }
    let hosted = tsc_binder::hosted::jsdoc_hosted(source);
    match &source.arena.node(node).data {
        NodeData::Parameter(data) => {
            if data.r#type.is_none() {
                if let Some(r#type) = hosted.type_of(node) {
                    let at = data
                        .name
                        .and_then(|name| children.iter().position(|&child| child == name))
                        .map_or(0, |index| index + 1);
                    children.insert(at, r#type);
                }
            }
        }
        NodeData::ParenthesizedExpression(data) => {
            if let Some(cast) = data
                .expression
                .and_then(|expression| hosted.cast_of(expression))
            {
                children.push(cast.type_node);
            }
        }
        // A `@template {C} T`'s constraint is the reparsed type parameter's,
        // walked with the type parameters before the parameters or members
        // (webpack's `SnapshotIterable`'s `{ keys(): … }`, `mergeToObject`'s
        // `{ name: string }`).
        NodeData::ClassDeclaration(_)
        | NodeData::ClassExpression(_)
        | NodeData::FunctionDeclaration(_)
        | NodeData::FunctionExpression(_)
        | NodeData::ArrowFunction(_)
        | NodeData::MethodDeclaration(_)
        | NodeData::GetAccessor(_)
        | NodeData::SetAccessor(_)
            if hosted.type_parameters_of(node).is_some() =>
        {
            let constraints: Vec<NodeId> = hosted
                .type_parameters_of(node)
                .map(|parameters| {
                    parameters
                        .tags
                        .iter()
                        .filter_map(|&tag| match &source.arena.node(tag).data {
                            NodeData::JSDocTemplateTag(data) => data.constraint,
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !constraints.is_empty() {
                let at = tsc_binder::node_util::name_field_of(source, node)
                    .and_then(|name| children.iter().position(|&child| child == name))
                    .map_or(0, |index| index + 1);
                children.splice(at..at, constraints);
            }
            // The `@returns` type below still applies to the functions.
            return children_with_return_type(source, hosted, node, children);
        }
        // An assignment declaration's binary expression gets its statement's
        // `@type` as its type (reparseHosted, KindExpressionStatement):
        // `/** @type {Map<string, { source: string }>} */ this.x = new Map()`
        // in webpack's constructor lists `source`.
        NodeData::BinaryExpression(data) => {
            if let Some(r#type) = hosted.type_of(node) {
                // Before the right operand: the type's members come before
                // the operand's symbols in tsgo's tree (webpack's
                // `compilationDependencies`'s `add` of the `@type` before the
                // object literal's `add`).
                let at = data
                    .right
                    .and_then(|right| children.iter().position(|&child| child == right))
                    .unwrap_or(children.len());
                children.insert(at, r#type);
            }
        }
        // A function's `@returns` type is its return type annotation, walked
        // before the body when the function is reached as a child (an
        // initializer's arrow): webpack's `getOptions` lists the `name` of
        // its `@returns {… & { name: string }}`.
        NodeData::FunctionDeclaration(_)
        | NodeData::FunctionExpression(_)
        | NodeData::ArrowFunction(_)
        | NodeData::MethodDeclaration(_)
        | NodeData::GetAccessor(_)
        | NodeData::SetAccessor(_) => {
            return children_with_return_type(source, hosted, node, children);
        }
        _ => {}
    }
    children
}

/// A function's `@returns` type as its return type annotation among its
/// children, before the body (tsgo's reparser; walked when the function is
/// reached as a child, as an initializer's arrow: webpack's `getOptions`
/// lists the `name` of its `@returns {… & { name: string }}`).
fn children_with_return_type(
    source: &SourceFile,
    hosted: &tsc_syntax::JsDocHosted,
    node: NodeId,
    mut children: Vec<NodeId>,
) -> Vec<NodeId> {
    let (written_type, body) = match &source.arena.node(node).data {
        NodeData::FunctionDeclaration(data) => (data.r#type, data.body),
        NodeData::FunctionExpression(data) => (data.r#type, data.body),
        NodeData::ArrowFunction(data) => (data.r#type, data.body),
        NodeData::MethodDeclaration(data) => (data.r#type, data.body),
        NodeData::GetAccessor(data) => (data.r#type, data.body),
        NodeData::SetAccessor(data) => (data.r#type, data.body),
        _ => return children,
    };
    if written_type.is_none() {
        if let Some(return_type) = hosted.type_of(node) {
            let at = body
                .and_then(|body| children.iter().position(|&child| child == body))
                .unwrap_or(children.len());
            children.insert(at, return_type);
        }
    }
    children
}

/// The nodes of a list.
fn list(source: &SourceFile, list: Option<tsc_syntax::NodeArrayId>) -> Vec<NodeId> {
    list.map(|list| source.arena.node_array(list).nodes.to_vec())
        .unwrap_or_default()
}

/// tsgo `getSymbolKindFromNode`.
pub(crate) fn symbol_kind_from_node(request: &Request<'_, '_>, node: NodeId) -> SymbolKind {
    let source = request.state.binder.source_of_node(node);
    match request.node_kind(node) {
        SyntaxKind::SourceFile => {
            if source.external_module_indicator.is_some() {
                SymbolKind::MODULE
            } else {
                SymbolKind::FILE
            }
        }
        SyntaxKind::ModuleDeclaration => SymbolKind::NAMESPACE,
        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => SymbolKind::CLASS,
        SyntaxKind::InterfaceDeclaration => SymbolKind::INTERFACE,
        SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::JSDocTypedefTag
        | SyntaxKind::JSDocCallbackTag => SymbolKind::CLASS,
        SyntaxKind::EnumDeclaration => SymbolKind::ENUM,
        SyntaxKind::VariableDeclaration => SymbolKind::VARIABLE,
        SyntaxKind::ArrowFunction
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression => SymbolKind::FUNCTION,
        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor => SymbolKind::PROPERTY,
        SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature => SymbolKind::METHOD,
        SyntaxKind::PropertyDeclaration
        | SyntaxKind::PropertySignature
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::ShorthandPropertyAssignment
        | SyntaxKind::SpreadAssignment
        | SyntaxKind::IndexSignature => SymbolKind::PROPERTY,
        SyntaxKind::CallSignature => SymbolKind::METHOD,
        SyntaxKind::ConstructSignature => SymbolKind::CONSTRUCTOR,
        SyntaxKind::Constructor | SyntaxKind::ClassStaticBlockDeclaration => {
            SymbolKind::CONSTRUCTOR
        }
        SyntaxKind::TypeParameter => SymbolKind::TYPE_PARAMETER,
        SyntaxKind::EnumMember => SymbolKind::ENUM_MEMBER,
        SyntaxKind::Parameter => {
            if node_util::has_syntactic_modifier(
                source,
                node,
                ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
            ) {
                SymbolKind::PROPERTY
            } else {
                SymbolKind::VARIABLE
            }
        }
        SyntaxKind::BinaryExpression | SyntaxKind::CallExpression => {
            match get_assignment_declaration_kind(source, node) {
                AssignmentDeclarationKind::ThisProperty
                | AssignmentDeclarationKind::Property
                | AssignmentDeclarationKind::ObjectDefinePropertyValue => SymbolKind::PROPERTY,
                _ => SymbolKind::VARIABLE,
            }
        }
        // String literals used as property names (e.g., in Object.defineProperty)
        SyntaxKind::StringLiteral
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::NumericLiteral => SymbolKind::PROPERTY,
        _ => SymbolKind::VARIABLE,
    }
}

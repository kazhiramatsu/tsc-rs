//! tsgo `ls/callhierarchy.go`: the call hierarchy of a declaration — its
//! item (`textDocument/prepareCallHierarchy`), the calls into it from its
//! references (`callHierarchy/incomingCalls`) and the calls out of its body
//! (`callHierarchy/outgoingCalls`).

use std::collections::HashMap;
use std::ops::Range;

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_emitter::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions, StandaloneWriter,
    TransformArena, TransformNode,
};
use tsc_lsproto::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsOrNull,
    CallHierarchyIncomingCallsResponse, CallHierarchyItem, CallHierarchyItemsOrNull,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsOrNull,
    CallHierarchyOutgoingCallsResponse, CallHierarchyPrepareResponse, DocumentUri, Position,
};
use tsc_program::uri::{file_name_to_document_uri, uri_to_file_name};
use tsc_syntax::{
    skip_trivia, skip_trivia_ex, NodeData, NodeId, SkipTriviaOptions, SourceFile, SyntaxKind,
};
use tsc_types::{JsStr, ModifierFlags, NodeFlags, SymbolFlags, SymbolId};

use crate::astnav::Navigator;
use crate::definition::Callee;
use crate::find_all_references::{EntryKind, ReferenceEntry};
use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::symbols::symbol_kind_from_node;
use crate::utilities::{token_start, touching, Touching};

/// tsgo `callSite`: a call of a declaration, at a range of a file.
struct CallSite {
    declaration: NodeId,
    file: usize,
    range: Range<usize>,
}

impl LanguageService<'_> {
    /// tsgo `ProvidePrepareCallHierarchy`: the items of the declarations at
    /// a position, each location once.
    pub fn provide_prepare_call_hierarchy(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
    ) -> Result<CallHierarchyPrepareResponse, LsError> {
        let file = self.file(uri);
        let items = self.with_request(file, position, false, |request, position| {
            let mut items: Vec<CallHierarchyItem> = Vec::new();
            for declaration in request.call_hierarchy_declarations(file, position, false)? {
                if let Some(item) = request.create_call_hierarchy_item(declaration)? {
                    if !items.iter().any(|known| {
                        known.uri == item.uri && known.selection_range == item.selection_range
                    }) {
                        items.push(item);
                    }
                }
            }
            Ok(items)
        })?;
        Ok(match items {
            Some(items) if !items.is_empty() => CallHierarchyItemsOrNull::CallHierarchyItems(items),
            _ => CallHierarchyItemsOrNull::Null,
        })
    }

    /// tsgo `ProvideCallHierarchyIncomingCalls` (one project; the other
    /// projects of the orchestrator come with L7e): the calls into the
    /// item's declarations, a caller's ranges merged.
    pub fn provide_call_hierarchy_incoming_calls(
        &mut self,
        item: &CallHierarchyItem,
    ) -> Result<CallHierarchyIncomingCallsResponse, LsError> {
        let Some(file) = self.item_file(item) else {
            return Ok(CallHierarchyIncomingCallsOrNull::Null);
        };
        let position = &item.selection_range.start;
        let calls = self.with_request(file, position, false, |request, position| {
            let mut calls: Vec<CallHierarchyIncomingCall> = Vec::new();
            for declaration in request.call_hierarchy_declarations(file, position, true)? {
                for call in request.incoming_calls(declaration)? {
                    match calls.iter_mut().find(|known| {
                        known.from.uri == call.from.uri
                            && known.from.selection_range == call.from.selection_range
                    }) {
                        Some(existing) => {
                            for range in call.from_ranges {
                                if !existing.from_ranges.contains(&range) {
                                    existing.from_ranges.push(range);
                                }
                            }
                        }
                        None => calls.push(call),
                    }
                }
            }
            Ok(calls)
        })?;
        Ok(match calls {
            Some(calls) if !calls.is_empty() => {
                CallHierarchyIncomingCallsOrNull::CallHierarchyIncomingCalls(calls)
            }
            _ => CallHierarchyIncomingCallsOrNull::Null,
        })
    }

    /// tsgo `ProvideCallHierarchyOutgoingCalls`: the calls out of the item's
    /// declarations, a callee's ranges merged.
    pub fn provide_call_hierarchy_outgoing_calls(
        &mut self,
        item: &CallHierarchyItem,
    ) -> Result<CallHierarchyOutgoingCallsResponse, LsError> {
        let Some(file) = self.item_file(item) else {
            return Ok(CallHierarchyOutgoingCallsOrNull::Null);
        };
        let position = &item.selection_range.start;
        let calls = self.with_request(file, position, false, |request, position| {
            let mut calls: Vec<CallHierarchyOutgoingCall> = Vec::new();
            for declaration in request.call_hierarchy_declarations(file, position, true)? {
                for call in request.outgoing_calls(declaration)? {
                    match calls.iter_mut().find(|known| {
                        known.to.uri == call.to.uri
                            && known.to.selection_range == call.to.selection_range
                    }) {
                        Some(existing) => {
                            for range in call.from_ranges {
                                if !existing.from_ranges.contains(&range) {
                                    existing.from_ranges.push(range);
                                }
                            }
                        }
                        None => calls.push(call),
                    }
                }
            }
            Ok(calls)
        })?;
        Ok(match calls {
            Some(calls) if !calls.is_empty() => {
                CallHierarchyOutgoingCallsOrNull::CallHierarchyOutgoingCalls(calls)
            }
            _ => CallHierarchyOutgoingCallsOrNull::Null,
        })
    }

    /// tsgo `program.GetSourceFile(item.Uri.FileName())`: none when the
    /// program has no such file.
    fn item_file(&self, item: &CallHierarchyItem) -> Option<usize> {
        let file_name = uri_to_file_name(item.uri.as_str()).ok()?;
        self.program.file_index(JsStr::from_str(&file_name))
    }
}

impl Request<'_, '_> {
    /// tsgo `callHierarchyDeclarations`: the declarations of the node at a
    /// position (the file at position 0, only where `allow_source_file`).
    fn call_hierarchy_declarations(
        &mut self,
        file: usize,
        position: u32,
        allow_source_file: bool,
    ) -> CheckResult<Vec<NodeId>> {
        let source = self.state.binder.source(file);
        let node = if position != 0 {
            match touching(Navigator::new(source).touching_property_name(position)) {
                Some(node) => node,
                None => return Ok(Vec::new()),
            }
        } else {
            Touching::Node(source.root)
        };
        if !allow_source_file && self.kind(node) == SyntaxKind::SourceFile {
            return Ok(Vec::new());
        }
        let mut declarations = Vec::new();
        for declaration in self.resolve_call_hierarchy_declaration(node)? {
            if !declarations.contains(&declaration) {
                declarations.push(declaration);
            }
        }
        Ok(declarations)
    }

    /// tsgo `resolveCallHierarchyDeclaration`: the declarations a call
    /// hierarchy item can be — a source file, a module, a class static
    /// block, or something callable with a name (classes, functions,
    /// function expressions with a name or assigned to a constant,
    /// constructors, methods, accessors); a call in something else belongs
    /// to the declaration around it. A reference follows its symbol (once).
    fn resolve_call_hierarchy_declaration(
        &mut self,
        location: Touching,
    ) -> CheckResult<Vec<NodeId>> {
        let mut location = location;
        let mut following_symbol = false;
        loop {
            if let Touching::Node(node) = location {
                if self.is_valid_call_hierarchy_declaration(node) {
                    return self.find_implementation_or_all_initial_declarations(node);
                }
                if self.is_possible_call_hierarchy_declaration(node) {
                    if let Some(ancestor) = self.find_ancestor(Some(node), |request, node| {
                        request.is_valid_call_hierarchy_declaration(node)
                    }) {
                        return self.find_implementation_or_all_initial_declarations(ancestor);
                    }
                }
                if self.is_declaration_name(node) {
                    let parent = self.parent_of(node).expect("a declaration name's parent");
                    if self.is_valid_call_hierarchy_declaration(parent) {
                        return self.find_implementation_or_all_initial_declarations(parent);
                    }
                    if self.is_possible_call_hierarchy_declaration(parent) {
                        if let Some(ancestor) = self.find_ancestor(Some(parent), |request, node| {
                            request.is_valid_call_hierarchy_declaration(node)
                        }) {
                            return self.find_implementation_or_all_initial_declarations(ancestor);
                        }
                    }
                    if self.is_variable_like(parent) {
                        if let Some(initializer) = self.initializer_of(parent) {
                            if self.is_assigned_expression(initializer) {
                                return Ok(vec![initializer]);
                            }
                        }
                    }
                    return Ok(Vec::new());
                }
                if self.node_kind(node) == SyntaxKind::Constructor {
                    let parent = self.parent_of(node).expect("a constructor's class");
                    if self.is_valid_call_hierarchy_declaration(parent) {
                        return Ok(vec![parent]);
                    }
                    return Ok(Vec::new());
                }
            }
            if self.kind(location) == SyntaxKind::StaticKeyword {
                if let Some(parent) = self.parent(location) {
                    if self.node_kind(parent) == SyntaxKind::ClassStaticBlockDeclaration {
                        location = Touching::Node(parent);
                        continue;
                    }
                }
            }
            // #39453
            if let Touching::Node(node) = location {
                if self.node_kind(node) == SyntaxKind::VariableDeclaration {
                    if let Some(initializer) = self.initializer_of(node) {
                        if self.is_assigned_expression(initializer) {
                            return Ok(vec![initializer]);
                        }
                    }
                }
            }
            if !following_symbol {
                if let Some(mut symbol) = self.symbol_at(location)? {
                    if self
                        .state
                        .symbol_flags(symbol)
                        .intersects(SymbolFlags::ALIAS)
                    {
                        symbol = self.state.get_aliased_symbol(symbol)?;
                    }
                    if let Some(declaration) = self.state.binder.symbol(symbol).value_declaration {
                        following_symbol = true;
                        location = Touching::Node(declaration);
                        continue;
                    }
                }
            }
            return Ok(Vec::new());
        }
    }

    /// tsgo `createCallHierarchyItem`: a declaration's name, kind, file,
    /// range (from its first token or comment) and name's range.
    fn create_call_hierarchy_item(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<CallHierarchyItem>> {
        let file = self.state.binder.file_index_of_node(node);
        let source = self.state.binder.source(file);
        let (name, name_range) = self.call_hierarchy_item_name(node)?;
        let container_name = self.call_hierarchy_item_container_name(node)?;
        let kind = symbol_kind_from_node(self, node);
        let record = self.state.binder.node_record(node);
        let full_start = skip_trivia_ex(
            source.text(),
            record.pos as usize,
            SkipTriviaOptions {
                stop_at_comments: true,
                ..SkipTriviaOptions::default()
            },
        );
        let range = self.lsp_range(source, full_start..record.end as usize);
        let selection_range = self.lsp_range(source, name_range);
        Ok(Some(CallHierarchyItem {
            name,
            kind,
            tags: None,
            detail: (!container_name.is_empty()).then_some(container_name),
            uri: DocumentUri(file_name_to_document_uri(
                &source.file_name.to_string_lossy(),
            )),
            range,
            selection_range,
            data: None,
        }))
    }

    /// tsgo `getCallHierarchyItemName`: a declaration's name and its range
    /// (`default`, `static {}` or `(anonymous)` without one).
    fn call_hierarchy_item_name(&mut self, node: NodeId) -> CheckResult<(String, Range<usize>)> {
        let source = self.state.binder.source_of_node(node);
        let kind = self.node_kind(node);
        if kind == SyntaxKind::SourceFile {
            return Ok((source.file_name.to_string_lossy().into_owned(), 0..0));
        }
        if matches!(
            kind,
            SyntaxKind::FunctionDeclaration | SyntaxKind::ClassDeclaration
        ) && self.name_of(node).is_none()
        {
            if let Some(modifier) = self.default_modifier(node) {
                let record = self.state.binder.node_record(modifier);
                let start = skip_trivia(source.text(), record.pos as usize);
                return Ok(("default".to_owned(), start..record.end as usize));
            }
        }
        if kind == SyntaxKind::ClassStaticBlockDeclaration {
            let start = skip_trivia(source.text(), self.pos_past_modifiers(node));
            let class = self.parent_of(node).expect("a static block's class");
            let prefix = match self.state.get_symbol_at_location(class)? {
                Some(symbol) => format!("{} ", self.symbol_to_string(symbol)?),
                None => String::new(),
            };
            // "static".length
            return Ok((format!("{prefix}static {{}}"), start..start + 6));
        }
        let name = if self.is_assigned_expression(node) {
            self.parent_of(node).and_then(|parent| self.name_of(parent))
        } else {
            node_util::get_name_of_declaration(source, node)
        };
        let Some(name) = name.filter(|&name| !node_util::node_is_missing(source, Some(name)))
        else {
            let keyword_start = skip_trivia(source.text(), self.pos_past_modifiers(node));
            return Ok(match kind {
                // "function".length
                SyntaxKind::FunctionDeclaration | SyntaxKind::FunctionExpression => {
                    ("(anonymous)".to_owned(), keyword_start..keyword_start + 8)
                }
                // "class".length
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                    ("(anonymous)".to_owned(), keyword_start..keyword_start + 5)
                }
                _ => panic!("Expected call hierarchy item to have a name"),
            });
        };
        let text = self.text_of_call_hierarchy_name(name, node)?;
        let record = self.state.binder.node_record(name);
        let start = skip_trivia(source.text(), record.pos as usize);
        Ok((text, start..record.end as usize))
    }

    /// tsgo `getTextOfCallHierarchyName`: a name's text, else its symbol's
    /// name, else `print_node` printed on one line.
    fn text_of_call_hierarchy_name(
        &mut self,
        name: NodeId,
        print_node: NodeId,
    ) -> CheckResult<String> {
        let source = self.state.binder.source_of_node(name);
        if self.node_kind(name) == SyntaxKind::Identifier
            || node_util::is_string_or_numeric_literal_like(source, name)
        {
            return Ok(self.node_text(Touching::Node(name)));
        }
        if let NodeData::ComputedPropertyName(computed) = &self.state.binder.node_record(name).data
        {
            if let Some(expression) = computed.expression {
                if node_util::is_string_or_numeric_literal_like(source, expression) {
                    return Ok(self.node_text(Touching::Node(expression)));
                }
            }
        }
        if let Some(symbol) = self.state.get_symbol_at_location(name)? {
            let text = self.symbol_to_string(symbol)?;
            if !text.is_empty() {
                return Ok(text);
            }
        }
        Ok(print_node_on_one_line(source, print_node))
    }

    /// tsgo `getCallHierarchyItemContainerName`: the name of the class,
    /// object literal or namespace a declaration is in.
    fn call_hierarchy_item_container_name(&mut self, node: NodeId) -> CheckResult<String> {
        if self.is_assigned_expression(node) {
            let parent = self
                .parent_of(node)
                .expect("an assigned expression's declaration");
            let grandparent = self.parent_of(parent);
            if self.node_kind(parent) == SyntaxKind::PropertyDeclaration {
                if let Some(class) = grandparent.filter(|&class| {
                    matches!(
                        self.node_kind(class),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                }) {
                    let source = self.state.binder.source_of_node(class);
                    if self.node_kind(class) == SyntaxKind::ClassExpression {
                        if let Some(assigned_name) = node_util::get_assigned_name(source, class) {
                            return self.text_of_call_hierarchy_name(assigned_name, assigned_name);
                        }
                    } else if let Some(name) = self.name_of(class) {
                        return self.text_of_call_hierarchy_name(name, name);
                    }
                }
            }
            let module_block = grandparent
                .and_then(|node| self.parent_of(node))
                .and_then(|node| self.parent_of(node));
            if let Some(module_block) =
                module_block.filter(|&block| self.node_kind(block) == SyntaxKind::ModuleBlock)
            {
                if let Some(module) = self.parent_of(module_block) {
                    if self.node_kind(module) == SyntaxKind::ModuleDeclaration {
                        if let Some(name) = self
                            .name_of(module)
                            .filter(|&name| self.node_kind(name) == SyntaxKind::Identifier)
                        {
                            return Ok(self.node_text(Touching::Node(name)));
                        }
                    }
                }
            }
            return Ok(String::new());
        }
        let parent = self.parent_of(node);
        match self.node_kind(node) {
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor | SyntaxKind::MethodDeclaration => {
                let parent = parent.expect("a member's container");
                let source = self.state.binder.source_of_node(parent);
                if self.node_kind(parent) == SyntaxKind::ObjectLiteralExpression {
                    if let Some(assigned_name) = node_util::get_assigned_name(source, parent) {
                        return self.text_of_call_hierarchy_name(assigned_name, assigned_name);
                    }
                }
                if let Some(name) = node_util::get_name_of_declaration(source, parent) {
                    return self.text_of_call_hierarchy_name(name, name);
                }
            }
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ModuleDeclaration => {
                if let Some(parent) =
                    parent.filter(|&parent| self.node_kind(parent) == SyntaxKind::ModuleBlock)
                {
                    if let Some(module) = self.parent_of(parent) {
                        if self.node_kind(module) == SyntaxKind::ModuleDeclaration {
                            if let Some(name) = self
                                .name_of(module)
                                .filter(|&name| self.node_kind(name) == SyntaxKind::Identifier)
                            {
                                return Ok(self.node_text(Touching::Node(name)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(String::new())
    }

    /// tsgo `getIncomingCalls`: the calls into a declaration — its
    /// references that call it, grouped by the declaration they are in, by
    /// file and first range.
    fn incoming_calls(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult<Vec<CallHierarchyIncomingCall>> {
        // Source files and modules have no incoming calls.
        if matches!(
            self.node_kind(declaration),
            SyntaxKind::SourceFile
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::ClassStaticBlockDeclaration
        ) {
            return Ok(Vec::new());
        }
        let Some(location) = self.call_hierarchy_declaration_reference_node(declaration) else {
            return Ok(Vec::new());
        };
        let file = self.state.binder.file_index_of_node(location);
        let source = self.state.binder.source(file);
        let position = token_start(source, location) as u32;
        let data = self.provide_symbols_and_entries(file, position, false, false)?;
        let entries: Vec<ReferenceEntry> = data
            .iter()
            .flat_map(|data| data.symbols_and_entries.iter())
            .flat_map(|entry| entry.references.iter().cloned())
            .collect();
        let call_sites: Vec<CallSite> = entries
            .iter()
            .filter_map(|entry| self.convert_entry_to_call_site(entry))
            .collect();
        let mut calls = Vec::new();
        for sites in group_call_sites(call_sites) {
            let from_ranges = self.sorted_ranges(&sites);
            let Some(from) = self.create_call_hierarchy_item(sites[0].declaration)? else {
                continue;
            };
            if from_ranges.is_empty() {
                continue;
            }
            calls.push(CallHierarchyIncomingCall { from, from_ranges });
        }
        calls.sort_by(|left, right| {
            left.from
                .uri
                .as_str()
                .cmp(right.from.uri.as_str())
                .then_with(|| compare_first_ranges(&left.from_ranges, &right.from_ranges))
        });
        Ok(calls)
    }

    /// tsgo `convertEntryToCallSite`: a reference that calls (or accesses)
    /// what it names, in the declaration around it (or its file).
    fn convert_entry_to_call_site(&self, entry: &ReferenceEntry) -> Option<CallSite> {
        if entry.kind != EntryKind::Node {
            return None;
        }
        let Some(Touching::Node(node)) = entry.node else {
            return None;
        };
        if !self.is_callee_worker(node, Callee::CallOrNew, true, true)
            && !self.is_callee_worker(node, Callee::TaggedTemplate, true, true)
            && !self.is_callee_worker(node, Callee::Decorator, true, true)
            && !self.is_callee_worker(node, Callee::JsxOpeningLikeElement, true, true)
            && !self.is_right_side_of_property_access(node)
            && !self.is_argument_expression_of_element_access(node)
        {
            return None;
        }
        let file = self.state.binder.file_index_of_node(node);
        let source = self.state.binder.source(file);
        let declaration = self
            .find_ancestor(Some(node), |request, node| {
                request.is_valid_call_hierarchy_declaration(node)
            })
            .unwrap_or(source.root);
        let record = self.state.binder.node_record(node);
        let start = skip_trivia(source.text(), record.pos as usize);
        Some(CallSite {
            declaration,
            file,
            range: start..record.end as usize,
        })
    }

    /// tsgo `getOutgoingCalls`: the calls in a declaration's body, grouped
    /// by the declaration they call, by file and first range.
    fn outgoing_calls(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult<Vec<CallHierarchyOutgoingCall>> {
        let source = self.state.binder.source_of_node(declaration);
        if node_util::node_flags(source, declaration).intersects(NodeFlags::AMBIENT)
            || self.node_kind(declaration) == SyntaxKind::MethodSignature
        {
            return Ok(Vec::new());
        }
        let call_sites = self.collect_call_sites(declaration)?;
        let mut calls = Vec::new();
        for sites in group_call_sites(call_sites) {
            let from_ranges = self.sorted_ranges(&sites);
            let Some(to) = self.create_call_hierarchy_item(sites[0].declaration)? else {
                continue;
            };
            if from_ranges.is_empty() {
                continue;
            }
            calls.push(CallHierarchyOutgoingCall { to, from_ranges });
        }
        calls.sort_by(|left, right| {
            left.to
                .uri
                .as_str()
                .cmp(right.to.uri.as_str())
                .then_with(|| compare_first_ranges(&left.from_ranges, &right.from_ranges))
        });
        Ok(calls)
    }

    /// The protocol ranges of a group's call sites, sorted (tsgo
    /// `lsproto.CompareRanges`).
    fn sorted_ranges(&self, sites: &[CallSite]) -> Vec<tsc_lsproto::Range> {
        let mut ranges: Vec<tsc_lsproto::Range> = sites
            .iter()
            .map(|site| self.lsp_range(self.state.binder.source(site.file), site.range.clone()))
            .collect();
        ranges.sort_by(compare_ranges);
        ranges
    }

    /// tsgo `collectCallSites`: the calls in a declaration's body (a file's
    /// or module's statements, a function's parameters and body, a class's
    /// decorators, base class, member initializers, constructor and static
    /// blocks).
    fn collect_call_sites(&mut self, node: NodeId) -> CheckResult<Vec<CallSite>> {
        let mut collector = Vec::new();
        let source = self.state.binder.source_of_node(node);
        match self.node_kind(node) {
            SyntaxKind::SourceFile => {
                for statement in self.node_list(node_util::statements_of(source, node)) {
                    self.collect(statement, &mut collector)?;
                }
            }
            SyntaxKind::ModuleDeclaration => {
                if !node_util::has_syntactic_modifier(source, node, ModifierFlags::AMBIENT) {
                    if let Some(body) = node_util::body_of(source, node)
                        .filter(|&body| self.node_kind(body) == SyntaxKind::ModuleBlock)
                    {
                        for statement in self.node_list(node_util::statements_of(source, body)) {
                            self.collect(statement, &mut collector)?;
                        }
                    }
                }
            }
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor => {
                if let Some(implementation) = self.find_implementation(node)? {
                    for parameter in crate::utilities::parameters_of(self.state, implementation) {
                        self.collect(parameter, &mut collector)?;
                    }
                    if let Some(body) = node_util::body_of(source, implementation) {
                        self.collect(body, &mut collector)?;
                    }
                }
            }
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                for modifier in self.modifiers_of(node) {
                    self.collect(modifier, &mut collector)?;
                }
                if let Some(heritage) = node_util::get_class_extends_heritage_element(source, node)
                {
                    if let Some(expression) = node_util::expression_of(source, heritage) {
                        self.collect(expression, &mut collector)?;
                    }
                }
                for member in self.members_of(node) {
                    for modifier in self.modifiers_of(member) {
                        self.collect(modifier, &mut collector)?;
                    }
                    match self.node_kind(member) {
                        SyntaxKind::PropertyDeclaration => {
                            if let Some(initializer) = self.initializer_of(member) {
                                self.collect(initializer, &mut collector)?;
                            }
                        }
                        SyntaxKind::Constructor => {
                            if let Some(body) = node_util::body_of(source, member) {
                                for parameter in crate::utilities::parameters_of(self.state, member)
                                {
                                    self.collect(parameter, &mut collector)?;
                                }
                                self.collect(body, &mut collector)?;
                            }
                        }
                        SyntaxKind::ClassStaticBlockDeclaration => {
                            self.collect(member, &mut collector)?;
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::ClassStaticBlockDeclaration => {
                if let Some(body) = node_util::body_of(source, node) {
                    self.collect(body, &mut collector)?;
                }
            }
            kind => panic!("unexpected call hierarchy declaration {kind:?}"),
        }
        Ok(collector)
    }

    /// tsgo `callSiteCollector.collect`: the calls in a node — not in
    /// ambient code, other call hierarchy declarations (but a class's
    /// computed member names), imports, exports, interfaces, type aliases
    /// or types.
    fn collect(&mut self, node: NodeId, sites: &mut Vec<CallSite>) -> CheckResult<()> {
        let source = self.state.binder.source_of_node(node);
        if node_util::node_flags(source, node).intersects(NodeFlags::AMBIENT) {
            return Ok(());
        }
        // do not descend into other call site declarations, other than class member names
        if self.is_valid_call_hierarchy_declaration(node) {
            if matches!(
                self.node_kind(node),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) {
                for member in self.members_of(node) {
                    if let Some(name) = self.name_of(member) {
                        if let NodeData::ComputedPropertyName(computed) =
                            &self.state.binder.node_record(name).data
                        {
                            if let Some(expression) = computed.expression {
                                self.collect(expression, sites)?;
                            }
                        }
                    }
                }
            }
            return Ok(());
        }
        let data = self.state.binder.node_record(node).data.clone();
        match self.node_kind(node) {
            // do not descend into nodes that cannot contain callable nodes
            SyntaxKind::Identifier
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::ImportDeclaration
            | SyntaxKind::ExportDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TypeAliasDeclaration => return Ok(()),
            SyntaxKind::ClassStaticBlockDeclaration => {
                self.record_call_site(node, sites)?;
                return Ok(());
            }
            // do not descend into the type side of an assertion
            SyntaxKind::TypeAssertionExpression
            | SyntaxKind::AsExpression
            | SyntaxKind::SatisfiesExpression => {
                if let Some(expression) = node_util::expression_of(source, node) {
                    self.collect(expression, sites)?;
                }
                return Ok(());
            }
            // do not descend into the type of a variable or parameter declaration
            SyntaxKind::VariableDeclaration | SyntaxKind::Parameter => {
                if let Some(name) = self.name_of(node) {
                    self.collect(name, sites)?;
                }
                if let Some(initializer) = self.initializer_of(node) {
                    self.collect(initializer, sites)?;
                }
                return Ok(());
            }
            // do not descend into the type arguments of a call or new expression
            SyntaxKind::CallExpression | SyntaxKind::NewExpression => {
                self.record_call_site(node, sites)?;
                let (expression, arguments) = match &data {
                    NodeData::CallExpression(call) => (call.expression, call.arguments),
                    NodeData::NewExpression(new) => (new.expression, new.arguments),
                    _ => (None, None),
                };
                if let Some(expression) = expression {
                    self.collect(expression, sites)?;
                }
                for argument in self.node_list(arguments) {
                    self.collect(argument, sites)?;
                }
                return Ok(());
            }
            // do not descend into the type arguments of a tagged template expression
            SyntaxKind::TaggedTemplateExpression => {
                self.record_call_site(node, sites)?;
                if let NodeData::TaggedTemplateExpression(tagged) = &data {
                    if let Some(tag) = tagged.tag {
                        self.collect(tag, sites)?;
                    }
                    if let Some(template) = tagged.template {
                        self.collect(template, sites)?;
                    }
                }
                return Ok(());
            }
            // do not descend into the type arguments of a JsxOpeningLikeElement
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement => {
                self.record_call_site(node, sites)?;
                let (tag_name, attributes) = match &data {
                    NodeData::JsxOpeningElement(element) => (element.tag_name, element.attributes),
                    NodeData::JsxSelfClosingElement(element) => {
                        (element.tag_name, element.attributes)
                    }
                    _ => (None, None),
                };
                if let Some(tag_name) = tag_name {
                    self.collect(tag_name, sites)?;
                }
                if let Some(attributes) = attributes {
                    self.collect(attributes, sites)?;
                }
                return Ok(());
            }
            SyntaxKind::Decorator => {
                self.record_call_site(node, sites)?;
                if let Some(expression) = node_util::expression_of(source, node) {
                    self.collect(expression, sites)?;
                }
                return Ok(());
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                self.record_call_site(node, sites)?;
                for child in self.children(node) {
                    self.collect(child, sites)?;
                }
                return Ok(());
            }
            _ => {}
        }
        if self.state.is_part_of_type_node(node) {
            // do not descend into types
            return Ok(());
        }
        for child in self.children(node) {
            self.collect(child, sites)?;
        }
        Ok(())
    }

    /// tsgo `callSiteCollector.recordCallSite`: a call's target and the
    /// declarations it calls.
    fn record_call_site(&mut self, node: NodeId, sites: &mut Vec<CallSite>) -> CheckResult<()> {
        let target = match &self.state.binder.node_record(node).data {
            NodeData::TaggedTemplateExpression(tagged) => tagged.tag,
            NodeData::JsxOpeningElement(element) => element.tag_name,
            NodeData::JsxSelfClosingElement(element) => element.tag_name,
            NodeData::PropertyAccessExpression(_)
            | NodeData::ElementAccessExpression(_)
            | NodeData::ClassStaticBlockDeclaration(_) => Some(node),
            NodeData::CallExpression(call) => call.expression,
            NodeData::NewExpression(new) => new.expression,
            NodeData::Decorator(decorator) => decorator.expression,
            _ => None,
        };
        let Some(target) = target else {
            return Ok(());
        };
        let declarations = self.resolve_call_hierarchy_declaration(Touching::Node(target))?;
        let file = self.state.binder.file_index_of_node(target);
        let source = self.state.binder.source(file);
        let record = self.state.binder.node_record(target);
        let start = skip_trivia(source.text(), record.pos as usize);
        for declaration in declarations {
            sites.push(CallSite {
                declaration,
                file,
                range: start..record.end as usize,
            });
        }
        Ok(())
    }

    /// tsgo `findImplementationOrAllInitialDeclarations`: a function's
    /// implementation, else the declarations of its symbol (one of each run
    /// of adjacent declarations), else the node.
    fn find_implementation_or_all_initial_declarations(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Vec<NodeId>> {
        if self.node_kind(node) == SyntaxKind::ClassStaticBlockDeclaration {
            return Ok(vec![node]);
        }
        if node_util::is_function_like_declaration_kind(self.node_kind(node)) {
            if let Some(implementation) = self.find_implementation(node)? {
                return Ok(vec![implementation]);
            }
        }
        let declarations = self.find_all_initial_declarations(node)?;
        if !declarations.is_empty() {
            return Ok(declarations);
        }
        Ok(vec![node])
    }

    /// tsgo `findImplementation`: the declaration with a body of a
    /// function-like declaration (a constructor's first with a body, a
    /// function's or method's value declaration with one), none without.
    fn find_implementation(&mut self, node: NodeId) -> CheckResult<Option<NodeId>> {
        let source = self.state.binder.source_of_node(node);
        let kind = self.node_kind(node);
        if !node_util::is_function_like_declaration_kind(kind)
            || node_util::body_of(source, node).is_some()
        {
            return Ok(Some(node));
        }
        if kind == SyntaxKind::Constructor {
            let class = self.parent_of(node).expect("a constructor's class");
            return Ok(self.members_of(class).into_iter().find(|&member| {
                self.node_kind(member) == SyntaxKind::Constructor
                    && node_util::body_of(source, member).is_some()
            }));
        }
        if matches!(
            kind,
            SyntaxKind::FunctionDeclaration | SyntaxKind::MethodDeclaration
        ) {
            if let Some(symbol) = self.symbol_of_call_hierarchy_declaration(node)? {
                if let Some(declaration) = self.state.binder.symbol(symbol).value_declaration {
                    let declaration_source = self.state.binder.source_of_node(declaration);
                    if node_util::is_function_like_declaration_kind(self.node_kind(declaration))
                        && node_util::body_of(declaration_source, declaration).is_some()
                    {
                        return Ok(Some(declaration));
                    }
                }
            }
            return Ok(None);
        }
        Ok(Some(node))
    }

    /// tsgo `findAllInitialDeclarations`: the call hierarchy declarations
    /// of a declaration's symbol by file and position, the first of each run
    /// of adjacent ones (overloads).
    fn find_all_initial_declarations(&mut self, node: NodeId) -> CheckResult<Vec<NodeId>> {
        if self.node_kind(node) == SyntaxKind::ClassStaticBlockDeclaration {
            return Ok(Vec::new());
        }
        let Some(symbol) = self.symbol_of_call_hierarchy_declaration(node)? else {
            return Ok(Vec::new());
        };
        let mut sorted = self.declarations_of(symbol);
        sorted.sort_by(|&left, &right| {
            let file_name = |node: NodeId| self.state.binder.source_of_node(node).file_name.clone();
            file_name(left).cmp(&file_name(right)).then_with(|| {
                self.state
                    .binder
                    .node_record(left)
                    .pos
                    .cmp(&self.state.binder.node_record(right).pos)
            })
        });
        let mut declarations = Vec::new();
        let mut last: Option<NodeId> = None;
        for declaration in sorted {
            if self.is_valid_call_hierarchy_declaration(declaration) {
                let adjacent = last.is_some_and(|last| {
                    self.parent_of(last) == self.parent_of(declaration)
                        && self.state.binder.node_record(last).end
                            == self.state.binder.node_record(declaration).pos
                });
                if !adjacent {
                    declarations.push(declaration);
                }
                last = Some(declaration);
            }
        }
        Ok(declarations)
    }

    /// tsgo `getSymbolOfCallHierarchyDeclaration`.
    fn symbol_of_call_hierarchy_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        if self.node_kind(node) == SyntaxKind::ClassStaticBlockDeclaration {
            return Ok(None);
        }
        match self.call_hierarchy_declaration_reference_node(node) {
            Some(location) => self.state.get_symbol_at_location(location),
            None => Ok(None),
        }
    }

    /// tsgo `getCallHierarchyDeclarationReferenceNode`: the node that
    /// references a call hierarchy declaration — its name (or the name of
    /// the declaration it is assigned to), its `default` modifier, or the
    /// file itself.
    fn call_hierarchy_declaration_reference_node(&self, node: NodeId) -> Option<NodeId> {
        if self.node_kind(node) == SyntaxKind::SourceFile {
            return Some(node);
        }
        if let Some(name) = self.name_of(node) {
            return Some(name);
        }
        if self.is_assigned_expression(node) {
            return self.parent_of(node).and_then(|parent| self.name_of(parent));
        }
        self.default_modifier(node)
    }

    /// tsgo `isPossibleCallHierarchyDeclaration`.
    fn is_possible_call_hierarchy_declaration(&self, node: NodeId) -> bool {
        matches!(
            self.node_kind(node),
            SyntaxKind::SourceFile
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::ClassStaticBlockDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        )
    }

    /// tsgo `isValidCallHierarchyDeclaration`.
    fn is_valid_call_hierarchy_declaration(&self, node: NodeId) -> bool {
        match self.node_kind(node) {
            SyntaxKind::SourceFile => true,
            SyntaxKind::ModuleDeclaration => self
                .name_of(node)
                .is_some_and(|name| self.node_kind(name) == SyntaxKind::Identifier),
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor => true,
            _ => self.is_named_expression(node) || self.is_assigned_expression(node),
        }
    }

    /// tsgo `isNamedExpression`: a function or class expression with a
    /// name.
    fn is_named_expression(&self, node: NodeId) -> bool {
        matches!(
            self.node_kind(node),
            SyntaxKind::FunctionExpression | SyntaxKind::ClassExpression
        ) && self
            .name_of(node)
            .is_some_and(|name| self.node_kind(name) == SyntaxKind::Identifier)
    }

    /// tsgo `isVariableLike`.
    fn is_variable_like(&self, node: NodeId) -> bool {
        matches!(
            self.node_kind(node),
            SyntaxKind::PropertyDeclaration | SyntaxKind::VariableDeclaration
        )
    }

    /// tsgo `isAssignedExpression`: a function, arrow or class expression
    /// without a name that initializes a constant variable or a class
    /// property with a name.
    fn is_assigned_expression(&self, node: NodeId) -> bool {
        if !matches!(
            self.node_kind(node),
            SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::ClassExpression
        ) || self.name_of(node).is_some()
        {
            return false;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        let named = self
            .name_of(parent)
            .is_some_and(|name| self.node_kind(name) == SyntaxKind::Identifier);
        if !(self.is_variable_like(parent) && self.initializer_of(parent) == Some(node) && named) {
            return false;
        }
        let source = self.state.binder.source_of_node(parent);
        node_util::get_combined_node_flags(source, parent).intersects(NodeFlags::CONST)
            || self.node_kind(parent) == SyntaxKind::PropertyDeclaration
    }

    /// The `default` modifier of a declaration.
    fn default_modifier(&self, node: NodeId) -> Option<NodeId> {
        self.modifiers_of(node)
            .into_iter()
            .find(|&modifier| self.node_kind(modifier) == SyntaxKind::DefaultKeyword)
    }

    /// tsgo `moveRangePastModifiers(node).Pos()`: the end of a declaration's
    /// last modifier, else its position.
    fn pos_past_modifiers(&self, node: NodeId) -> usize {
        match self.modifiers_of(node).last() {
            Some(&last) => self.state.binder.node_record(last).end as usize,
            None => self.state.binder.node_record(node).pos as usize,
        }
    }
}

/// tsgo's grouping of call sites by their declaration (a Go map: the order
/// is the sort's after).
fn group_call_sites(sites: Vec<CallSite>) -> Vec<Vec<CallSite>> {
    let mut groups: Vec<Vec<CallSite>> = Vec::new();
    let mut index: HashMap<NodeId, usize> = HashMap::new();
    for site in sites {
        match index.get(&site.declaration) {
            Some(&group) => groups[group].push(site),
            None => {
                index.insert(site.declaration, groups.len());
                groups.push(vec![site]);
            }
        }
    }
    groups
}

/// tsgo `lsproto.CompareRanges`: by start, then end.
fn compare_ranges(left: &tsc_lsproto::Range, right: &tsc_lsproto::Range) -> std::cmp::Ordering {
    let position = |position: &tsc_lsproto::Position| (position.line, position.character);
    position(&left.start)
        .cmp(&position(&right.start))
        .then_with(|| position(&left.end).cmp(&position(&right.end)))
}

/// The order of two calls by their first ranges (equal when either has
/// none).
fn compare_first_ranges(
    left: &[tsc_lsproto::Range],
    right: &[tsc_lsproto::Range],
) -> std::cmp::Ordering {
    match (left.first(), right.first()) {
        (Some(left), Some(right)) => compare_ranges(left, right),
        _ => std::cmp::Ordering::Equal,
    }
}

/// tsgo's printer with `RemoveComments` writing a node of its file on one
/// line.
fn print_node_on_one_line(source: &SourceFile, node: NodeId) -> String {
    let mut arena = TransformArena::new();
    let source_id = arena.add_source(source, None);
    let node = TransformNode::new(source_id, node);
    let Ok(mut transformation) = transform_nodes(arena, Vec::new(), Vec::new(), true) else {
        return String::new();
    };
    create_printer(PrinterOptions::new(NewLineKind::LineFeed).with_remove_comments(true))
        .print(
            &mut transformation,
            PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::SingleLine,
            },
            None,
        )
        .map(|printed| printed.text().to_owned())
        .unwrap_or_default()
}

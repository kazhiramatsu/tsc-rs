//! tsgo `ls/documenthighlights.go`: the highlights of what is at a position
//! — the references of its symbol in the file (or the files to search),
//! read or written, else the occurrences that belong with its keyword
//! (`if`/`else`, `return`/`throw`, `try`/`catch`/`finally`,
//! `switch`/`case`/`default`, loops with their `break`/`continue`,
//! accessors, constructors, `async`/`await`, `yield`, modifiers) — for
//! `textDocument/documentHighlight` and
//! `custom/textDocument/multiDocumentHighlight`.

use std::collections::{HashMap, HashSet};

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{
    CustomMultiDocumentHighlightResponse, DocumentHighlight, DocumentHighlightKind,
    DocumentHighlightResponse, DocumentHighlightsOrNull, DocumentUri, MultiDocumentHighlight,
    MultiDocumentHighlightParams, MultiDocumentHighlightsOrNull, Position,
};
use tsc_program::uri::{file_name_to_document_uri, uri_to_file_name};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{JsStr, ModifierFlags};

use crate::astnav::Navigator;
use crate::find_all_references::{
    get_referenced_symbols_for_node, EntryKind, RefOptions, ReferenceEntry, ReferenceUse,
};
use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::tree::View;
use crate::utilities::{is_function_like_kind, touching, Touching};

impl LanguageService<'_> {
    /// tsgo `ProvideDocumentHighlights`: the highlights in the document.
    pub fn provide_document_highlights(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
    ) -> Result<DocumentHighlightResponse, LsError> {
        let documents = self.document_highlights_worker(uri, position, &[])?;
        // Extract highlights for the current file only.
        let highlights = documents
            .into_iter()
            .filter(|document| document.uri == *uri)
            .flat_map(|document| document.highlights)
            .collect();
        Ok(DocumentHighlightsOrNull::DocumentHighlights(highlights))
    }

    /// tsgo `ProvideMultiDocumentHighlights`.
    pub fn provide_multi_document_highlights(
        &mut self,
        params: &MultiDocumentHighlightParams,
    ) -> Result<CustomMultiDocumentHighlightResponse, LsError> {
        let documents = self.document_highlights_worker(
            &params.text_document.uri,
            &params.position,
            &params.files_to_search,
        )?;
        Ok(MultiDocumentHighlightsOrNull::MultiDocumentHighlights(
            documents,
        ))
    }

    /// tsgo `provideDocumentHighlightsWorker` (a file without a content
    /// mapper: one position).
    fn document_highlights_worker(
        &mut self,
        uri: &DocumentUri,
        position: &Position,
        files_to_search: &[DocumentUri],
    ) -> Result<Vec<MultiDocumentHighlight>, LsError> {
        let file = self.file(uri);
        // Resolve the source files to search, deduplicating by file name.
        let mut seen_files = HashSet::new();
        let mut files = Vec::new();
        for uri in files_to_search {
            let file_name = uri_to_file_name(uri.as_str()).unwrap_or_default();
            if !seen_files.insert(file_name.clone()) {
                continue;
            }
            if let Some(index) = self.program.file_index(JsStr::from_str(&file_name)) {
                files.push(index);
            }
        }
        let result = self.with_request(file, position, false, |request, position| {
            request.document_highlights_at_position(uri, file, position, &files)
        })?;
        Ok(combine_multi_document_highlights(
            result.into_iter().collect(),
        ))
    }
}

/// tsgo `combineMultiDocumentHighlights`: the documents in order, each
/// range once.
fn combine_multi_document_highlights(
    results: Vec<Vec<MultiDocumentHighlight>>,
) -> Vec<MultiDocumentHighlight> {
    let mut combined: Vec<MultiDocumentHighlight> = Vec::new();
    let mut by_uri: HashMap<String, usize> = HashMap::new();
    let mut seen: HashMap<String, HashSet<(u32, u32, u32, u32)>> = HashMap::new();
    for result in results {
        for document in result {
            let index = *by_uri.entry(document.uri.0.clone()).or_insert_with(|| {
                combined.push(MultiDocumentHighlight {
                    uri: document.uri.clone(),
                    highlights: Vec::new(),
                });
                combined.len() - 1
            });
            let ranges = seen.entry(document.uri.0.clone()).or_default();
            for highlight in document.highlights {
                let range = &highlight.range;
                if ranges.insert((
                    range.start.line,
                    range.start.character,
                    range.end.line,
                    range.end.character,
                )) {
                    combined[index].highlights.push(highlight);
                }
            }
        }
    }
    combined
}

/// A highlight's read kind (tsgo `DocumentHighlightKindRead`).
fn read() -> Option<DocumentHighlightKind> {
    Some(DocumentHighlightKind::READ)
}

impl Request<'_, '_> {
    /// tsgo `provideDocumentHighlightsAtPosition`.
    fn document_highlights_at_position(
        &mut self,
        uri: &DocumentUri,
        file: usize,
        position: u32,
        files_to_search: &[usize],
    ) -> CheckResult<Vec<MultiDocumentHighlight>> {
        let source = self.state.binder.source(file);
        let Some(node) = touching(Navigator::new(source).touching_property_name(position)) else {
            return Ok(Vec::new());
        };

        // Cheap JSX check before resolving files to search.
        if let Some(parent) = self.parent(node) {
            let is_tag = match &self.state.binder.node_record(parent).data {
                NodeData::JsxClosingElement(_) => true,
                NodeData::JsxOpeningElement(data) => {
                    data.tag_name.map(Touching::Node) == Some(node)
                }
                _ => false,
            };
            if is_tag {
                let elements = self
                    .state
                    .binder
                    .node_record(parent)
                    .parent
                    .and_then(
                        |element| match &self.state.binder.node_record(element).data {
                            NodeData::JsxElement(data) => {
                                Some([data.opening_element, data.closing_element])
                            }
                            _ => None,
                        },
                    )
                    .unwrap_or_default();
                let highlights = elements
                    .into_iter()
                    .flatten()
                    .map(|element| DocumentHighlight {
                        range: self.node_highlight_range(Touching::Node(element)),
                        kind: read(),
                    })
                    .collect();
                return Ok(vec![MultiDocumentHighlight {
                    uri: uri.clone(),
                    highlights,
                }]);
            }
        }

        let files = if files_to_search.is_empty() {
            vec![file]
        } else {
            files_to_search.to_vec()
        };
        let mut documents = self.semantic_document_highlights(position, node, &files)?;
        if documents.is_empty() {
            // Fall back to syntactic highlights for the current file only.
            let highlights = self.syntactic_document_highlights(node);
            if !highlights.is_empty() {
                documents = vec![MultiDocumentHighlight {
                    uri: uri.clone(),
                    highlights,
                }];
            }
        }
        Ok(documents)
    }

    /// tsgo `getSemanticDocumentHighlights`: the references in the files,
    /// by file.
    fn semantic_document_highlights(
        &mut self,
        position: u32,
        node: Touching,
        files: &[usize],
    ) -> CheckResult<Vec<MultiDocumentHighlight>> {
        let options = RefOptions {
            use_: ReferenceUse::None,
            implementations: false,
            use_aliases_for_rename: false,
        };
        let entries = get_referenced_symbols_for_node(self, position, node, files, options)?;
        // Group highlights by file
        let mut by_file: HashMap<usize, Vec<DocumentHighlight>> = HashMap::new();
        for entry in &entries {
            for reference in &entry.references {
                let highlight = self.to_document_highlight(reference);
                by_file.entry(reference.file).or_default().push(highlight);
            }
        }
        let mut result = Vec::new();
        for &file in files {
            if let Some(highlights) = by_file.get(&file) {
                let source = self.state.binder.source(file);
                result.push(MultiDocumentHighlight {
                    uri: DocumentUri(file_name_to_document_uri(
                        &source.file_name.to_string_lossy(),
                    )),
                    highlights: highlights.clone(),
                });
            }
        }
        Ok(result)
    }

    /// tsgo `toDocumentHighlight`: a reference's range, written or read.
    fn to_document_highlight(&self, entry: &ReferenceEntry) -> DocumentHighlight {
        let range = self.location_of_entry(entry).range;
        let kind = match entry.node {
            Some(node)
                if entry.kind != EntryKind::Range && self.is_write_access_for_reference(node) =>
            {
                DocumentHighlightKind::WRITE
            }
            _ => DocumentHighlightKind::READ,
        };
        DocumentHighlight {
            range,
            kind: Some(kind),
        }
    }

    /// tsgo `createLspRangeFromNodeForFeature`: a node's or token's range
    /// from its token's start.
    fn node_highlight_range(&self, node: Touching) -> tsc_lsproto::Range {
        let (file, range) = match node {
            Touching::Node(id) => {
                let file = self.state.binder.file_index_of_node(id);
                let source = self.state.binder.source(file);
                let start = crate::utilities::token_start(source, id);
                (file, start..self.state.binder.node_record(id).end as usize)
            }
            token => self.range_of_touching(token),
        };
        self.lsp_range(self.state.binder.source(file), range)
    }

    /// tsgo `highlightSpans`.
    fn highlight_spans(&self, nodes: Vec<Touching>) -> Vec<DocumentHighlight> {
        nodes
            .into_iter()
            .map(|node| DocumentHighlight {
                range: self.node_highlight_range(node),
                kind: read(),
            })
            .collect()
    }

    /// tsgo `useParent`: the occurrences of a node of the kind the test
    /// accepts.
    fn use_parent(
        &self,
        node: Option<NodeId>,
        node_test: impl Fn(&Self, NodeId) -> bool,
        get_nodes: impl Fn(&Self, NodeId) -> Vec<Touching>,
    ) -> Vec<DocumentHighlight> {
        match node {
            Some(node) if node_test(self, node) => self.highlight_spans(get_nodes(self, node)),
            _ => Vec::new(),
        }
    }

    /// tsgo `getSyntacticDocumentHighlights`.
    fn syntactic_document_highlights(&self, node: Touching) -> Vec<DocumentHighlight> {
        let parent = self.parent(node);
        let kind = self.kind(node);
        let parent_kind = parent.map(|parent| self.node_kind(parent));
        match kind {
            SyntaxKind::IfKeyword | SyntaxKind::ElseKeyword => match parent {
                Some(parent) if parent_kind == Some(SyntaxKind::IfStatement) => {
                    self.if_else_occurrences(parent)
                }
                _ => Vec::new(),
            },
            SyntaxKind::ReturnKeyword => self.use_parent(
                parent,
                |request, node| request.node_kind(node) == SyntaxKind::ReturnStatement,
                Self::return_occurrences,
            ),
            SyntaxKind::ThrowKeyword => self.use_parent(
                parent,
                |request, node| request.node_kind(node) == SyntaxKind::ThrowStatement,
                Self::throw_occurrences,
            ),
            SyntaxKind::TryKeyword | SyntaxKind::CatchKeyword | SyntaxKind::FinallyKeyword => {
                let try_statement = if kind == SyntaxKind::CatchKeyword {
                    parent.and_then(|parent| self.state.binder.node_record(parent).parent)
                } else {
                    parent
                };
                self.use_parent(
                    try_statement,
                    |request, node| request.node_kind(node) == SyntaxKind::TryStatement,
                    Self::try_catch_finally_occurrences,
                )
            }
            SyntaxKind::SwitchKeyword => self.use_parent(
                parent,
                |request, node| request.node_kind(node) == SyntaxKind::SwitchStatement,
                Self::switch_case_default_occurrences,
            ),
            SyntaxKind::CaseKeyword | SyntaxKind::DefaultKeyword => {
                if matches!(
                    parent_kind,
                    Some(SyntaxKind::DefaultClause | SyntaxKind::CaseClause)
                ) {
                    let switch = parent
                        .and_then(|clause| self.state.binder.node_record(clause).parent)
                        .and_then(|block| self.state.binder.node_record(block).parent);
                    self.use_parent(
                        switch,
                        |request, node| request.node_kind(node) == SyntaxKind::SwitchStatement,
                        Self::switch_case_default_occurrences,
                    )
                } else {
                    Vec::new()
                }
            }
            SyntaxKind::BreakKeyword | SyntaxKind::ContinueKeyword => self.use_parent(
                parent,
                |request, node| {
                    matches!(
                        request.node_kind(node),
                        SyntaxKind::BreakStatement | SyntaxKind::ContinueStatement
                    )
                },
                Self::break_or_continue_statement_occurrences,
            ),
            SyntaxKind::ForKeyword | SyntaxKind::WhileKeyword | SyntaxKind::DoKeyword => self
                .use_parent(
                    parent,
                    |request, node| request.is_iteration_statement(node, true),
                    Self::loop_break_continue_occurrences,
                ),
            SyntaxKind::ConstructorKeyword => self.keywords_of_all_declarations(
                |request, node| request.node_kind(node) == SyntaxKind::Constructor,
                &[SyntaxKind::ConstructorKeyword],
                parent,
            ),
            SyntaxKind::GetKeyword | SyntaxKind::SetKeyword => self.keywords_of_all_declarations(
                |request, node| {
                    matches!(
                        request.node_kind(node),
                        SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
                    )
                },
                &[SyntaxKind::GetKeyword, SyntaxKind::SetKeyword],
                parent,
            ),
            SyntaxKind::AwaitKeyword => self.use_parent(
                parent,
                |request, node| request.node_kind(node) == SyntaxKind::AwaitExpression,
                |request, node| request.async_and_await_occurrences(Touching::Node(node)),
            ),
            SyntaxKind::AsyncKeyword => {
                self.highlight_spans(self.async_and_await_occurrences(node))
            }
            SyntaxKind::YieldKeyword => self.highlight_spans(self.yield_occurrences(node)),
            SyntaxKind::InKeyword | SyntaxKind::OutKeyword => Vec::new(),
            _ => {
                let Some(parent) = parent else {
                    return Vec::new();
                };
                let source = self.state.binder.source_of_node(parent);
                if node_util::is_modifier_kind(kind)
                    && (node_util::is_declaration_node(source, parent)
                        || self.node_kind(parent) == SyntaxKind::VariableStatement)
                {
                    self.highlight_spans(self.modifier_occurrences(kind, parent))
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// tsgo `getFromAllDeclarations`: the keyword of each declaration of the
    /// symbol of the keyword's declaration.
    fn keywords_of_all_declarations(
        &self,
        node_test: impl Fn(&Self, NodeId) -> bool + Copy,
        keywords: &[SyntaxKind],
        parent: Option<NodeId>,
    ) -> Vec<DocumentHighlight> {
        self.use_parent(parent, node_test, |request, declaration| {
            let mut found = Vec::new();
            if !crate::utilities::can_have_symbol(request.node_kind(declaration)) {
                return found;
            }
            let Some(symbol) = request.state.binder.node_symbol(declaration) else {
                return found;
            };
            for other in request.declarations_of(symbol) {
                if !node_test(request, other) {
                    continue;
                }
                let child = request
                    .children_from_non_js_doc(other)
                    .into_iter()
                    .find(|&child| keywords.contains(&request.kind(child)));
                found.extend(child);
            }
            found
        })
    }

    /// tsgo `getIfElseOccurrences`: the `if` and `else` keywords, an `else`
    /// and the `if` after it together when only whitespace is between them.
    fn if_else_occurrences(&self, if_statement: NodeId) -> Vec<DocumentHighlight> {
        let keywords = self.if_else_keywords(if_statement);
        let mut highlights = Vec::new();
        let mut index = 0;
        // We'd like to highlight else/ifs together if they are only separated by whitespace
        // (i.e. the keywords are separated by no comments, no newlines).
        while index < keywords.len() {
            if self.kind(keywords[index]) == SyntaxKind::ElseKeyword && index < keywords.len() - 1 {
                let else_keyword = keywords[index];
                let if_keyword = keywords[index + 1]; // this *should* always be an 'if' keyword.
                let (file, if_range) = self.range_of_touching(if_keyword);
                let source = self.state.binder.source(file);
                let text = source.text().as_bytes();
                let else_end = self.touching_end(else_keyword) as usize;
                // The bytes between the keywords, each read as a character.
                let should_combine = (else_end..if_range.start).rev().all(|position| {
                    crate::format::is_white_space_single_line(char::from(text[position]))
                });
                if should_combine {
                    let start =
                        tsc_syntax::skip_trivia(source.text(), self.pos(else_keyword) as usize);
                    highlights.push(DocumentHighlight {
                        range: self
                            .lsp_range(source, start..self.touching_end(if_keyword) as usize),
                        kind: read(),
                    });
                    index += 2; // skip the next keyword
                    continue;
                }
            }
            // Ordinary case: just highlight the keyword.
            highlights.push(DocumentHighlight {
                range: self.node_highlight_range(keywords[index]),
                kind: read(),
            });
            index += 1;
        }
        highlights
    }

    /// tsgo `getIfElseKeywords`: the `if` and `else` keywords of an if
    /// statement and the if statements of its `else` branches, from the
    /// outermost `if` of the chain.
    fn if_else_keywords(&self, if_statement: NodeId) -> Vec<Touching> {
        let mut if_statement = if_statement;
        // Traverse upwards through all parent if-statements linked by their else-branches.
        while let Some(parent) = self.state.binder.node_record(if_statement).parent {
            match &self.state.binder.node_record(parent).data {
                NodeData::IfStatement(data) if data.else_statement == Some(if_statement) => {
                    if_statement = parent;
                }
                _ => break,
            }
        }
        let mut keywords = Vec::new();
        // Traverse back down through the else branches, aggregating if/else keywords of if-statements.
        loop {
            let children = self.children_from_non_js_doc(if_statement);
            if let Some(&first) = children.first() {
                if self.kind(first) == SyntaxKind::IfKeyword {
                    keywords.push(first);
                }
            }
            // Generally the 'else' keyword is second-to-last, so traverse backwards.
            if let Some(&else_keyword) = children
                .iter()
                .rev()
                .find(|&&child| self.kind(child) == SyntaxKind::ElseKeyword)
            {
                keywords.push(else_keyword);
            }
            let NodeData::IfStatement(data) = &self.state.binder.node_record(if_statement).data
            else {
                break;
            };
            match data.else_statement {
                Some(else_statement)
                    if self.node_kind(else_statement) == SyntaxKind::IfStatement =>
                {
                    if_statement = else_statement;
                }
                _ => break,
            }
        }
        keywords
    }

    /// tsgo `getReturnOccurrences`: the `return` and owned `throw` keywords
    /// of the function.
    fn return_occurrences(&self, node: NodeId) -> Vec<Touching> {
        let Some(function) = self.find_ancestor(
            self.state.binder.node_record(node).parent,
            |request, node| is_function_like_kind(request.node_kind(node)),
        ) else {
            return Vec::new();
        };
        let mut keywords = Vec::new();
        let source = self.state.binder.source_of_node(function);
        if let Some(body) = node_util::body_of(source, function) {
            node_util::for_each_return_statement(source, body, &mut |statement| {
                keywords.extend(self.child_keyword(statement, SyntaxKind::ReturnKeyword));
                false
            });
            // Get all throw statements not in a try block
            for throw in self.aggregate_owned_throw_statements(body) {
                keywords.extend(self.child_keyword(throw, SyntaxKind::ThrowKeyword));
            }
        }
        keywords
    }

    /// tsgo `aggregateOwnedThrowStatements`: the throw statements a node
    /// owns (not caught by a try statement in it, not in a nested function).
    fn aggregate_owned_throw_statements(&self, node: NodeId) -> Vec<NodeId> {
        match &self.state.binder.node_record(node).data {
            NodeData::ThrowStatement(_) => return vec![node],
            NodeData::TryStatement(data) => {
                // Exceptions thrown within a try block lacking a catch clause are "owned" in the current context.
                let mut result = if let Some(catch_clause) = data.catch_clause {
                    self.aggregate_owned_throw_statements(catch_clause)
                } else if let Some(try_block) = data.try_block {
                    self.aggregate_owned_throw_statements(try_block)
                } else {
                    Vec::new()
                };
                if let Some(finally_block) = data.finally_block {
                    result.extend(self.aggregate_owned_throw_statements(finally_block));
                }
                return result;
            }
            _ => {}
        }
        // Do not cross function boundaries.
        if is_function_like_kind(self.node_kind(node)) {
            return Vec::new();
        }
        self.flat_map_children(node, Self::aggregate_owned_throw_statements)
    }

    /// tsgo `flatMapChildren`.
    fn flat_map_children(
        &self,
        node: NodeId,
        f: impl Fn(&Self, NodeId) -> Vec<NodeId>,
    ) -> Vec<NodeId> {
        let source = self.state.binder.source_of_node(node);
        let mut children = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
            .into_iter()
            .flat_map(|child| f(self, child))
            .collect()
    }

    /// tsgo `getThrowOccurrences`: the `throw` keywords of the throw
    /// statement's owner, and the `return` keywords of a function owner.
    fn throw_occurrences(&self, node: NodeId) -> Vec<Touching> {
        let Some(owner) = self.throw_statement_owner(node) else {
            return Vec::new();
        };
        let mut keywords = Vec::new();
        // Aggregate all throw statements "owned" by this owner.
        for throw in self.aggregate_owned_throw_statements(owner) {
            keywords.extend(self.child_keyword(throw, SyntaxKind::ThrowKeyword));
        }
        // If the "owner" is a function, then we equate 'return' and 'throw' statements in their
        // ability to "jump out" of the function, and include occurrences for both
        if self.is_function_block(owner) {
            let source = self.state.binder.source_of_node(owner);
            node_util::for_each_return_statement(source, owner, &mut |statement| {
                keywords.extend(self.child_keyword(statement, SyntaxKind::ReturnKeyword));
                false
            });
        }
        keywords
    }

    /// tsgo `getThrowStatementOwner`: the nearest function block or file, or
    /// try block (of a try statement with a catch clause) around the throw
    /// statement.
    fn throw_statement_owner(&self, throw_statement: NodeId) -> Option<NodeId> {
        let mut child = throw_statement;
        while let Some(parent) = self.state.binder.node_record(child).parent {
            if self.is_function_block(parent) || self.node_kind(parent) == SyntaxKind::SourceFile {
                return Some(parent);
            }
            // A throw-statement is only owned by a try-statement if the try-statement has
            // a catch clause, and if the throw-statement occurs within the try block.
            if let NodeData::TryStatement(data) = &self.state.binder.node_record(parent).data {
                if data.try_block == Some(child) && data.catch_clause.is_some() {
                    return Some(child);
                }
            }
            child = parent;
        }
        None
    }

    /// tsgo `ast.IsFunctionBlock`.
    fn is_function_block(&self, node: NodeId) -> bool {
        self.node_kind(node) == SyntaxKind::Block
            && self
                .state
                .binder
                .node_record(node)
                .parent
                .is_some_and(|parent| is_function_like_kind(self.node_kind(parent)))
    }

    /// tsgo `getTryCatchFinallyOccurrences`.
    fn try_catch_finally_occurrences(&self, node: NodeId) -> Vec<Touching> {
        let NodeData::TryStatement(data) = &self.state.binder.node_record(node).data else {
            return Vec::new();
        };
        let mut keywords = Vec::new();
        if let Some(token) = self.first_token(node) {
            if self.kind(token) == SyntaxKind::TryKeyword {
                keywords.push(token);
            }
        }
        if data.catch_clause.is_some() {
            keywords.extend(self.child_keyword(node, SyntaxKind::CatchKeyword));
        }
        if data.finally_block.is_some() {
            keywords.extend(self.child_keyword(node, SyntaxKind::FinallyKeyword));
        }
        keywords
    }

    /// tsgo `getSwitchCaseDefaultOccurrences`: the `switch`, `case` and
    /// `default` keywords, and the `break`s the switch owns.
    fn switch_case_default_occurrences(&self, node: NodeId) -> Vec<Touching> {
        let mut keywords = Vec::new();
        if let Some(token) = self.first_token(node) {
            if self.kind(token) == SyntaxKind::SwitchKeyword {
                keywords.push(token);
            }
        }
        let NodeData::SwitchStatement(data) = &self.state.binder.node_record(node).data else {
            return keywords;
        };
        let clauses = data
            .case_block
            .and_then(|block| match &self.state.binder.node_record(block).data {
                NodeData::CaseBlock(block) => block.clauses,
                _ => None,
            })
            .map(|clauses| self.state.binder.node_array(clauses).nodes.to_vec())
            .unwrap_or_default();
        for clause in clauses {
            if let Some(token) = self.first_token(clause) {
                if matches!(
                    self.kind(token),
                    SyntaxKind::CaseKeyword | SyntaxKind::DefaultKeyword
                ) {
                    keywords.push(token);
                }
            }
            for statement in self.aggregate_all_break_and_continue_statements(clause) {
                if self.node_kind(statement) == SyntaxKind::BreakStatement
                    && self.owns_break_or_continue_statement(node, statement)
                {
                    keywords.extend(self.first_token(statement));
                }
            }
        }
        keywords
    }

    /// tsgo `aggregateAllBreakAndContinueStatements`.
    fn aggregate_all_break_and_continue_statements(&self, node: NodeId) -> Vec<NodeId> {
        let kind = self.node_kind(node);
        if matches!(
            kind,
            SyntaxKind::BreakStatement | SyntaxKind::ContinueStatement
        ) {
            return vec![node];
        }
        if is_function_like_kind(kind) {
            return Vec::new();
        }
        self.flat_map_children(node, Self::aggregate_all_break_and_continue_statements)
    }

    /// tsgo `ownsBreakOrContinueStatement`.
    fn owns_break_or_continue_statement(&self, owner: NodeId, statement: NodeId) -> bool {
        self.break_or_continue_owner(statement) == Some(owner)
    }

    /// tsgo `getBreakOrContinueOwner`: the loop (or, for a `break`, the
    /// switch) a jump targets, by its label if it has one.
    fn break_or_continue_owner(&self, statement: NodeId) -> Option<NodeId> {
        let label = match &self.state.binder.node_record(statement).data {
            NodeData::BreakStatement(data) => data.label,
            NodeData::ContinueStatement(data) => data.label,
            _ => None,
        };
        let is_continue = self.node_kind(statement) == SyntaxKind::ContinueStatement;
        let mut current = Some(statement);
        while let Some(node) = current {
            match self.node_kind(node) {
                SyntaxKind::SwitchStatement if is_continue => {}
                SyntaxKind::SwitchStatement
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::WhileStatement
                | SyntaxKind::DoStatement => {
                    // If the statement is labeled, check if the node is labeled by the statement's label.
                    let labeled = label.is_none_or(|label| {
                        let source = self.state.binder.source_of_node(label);
                        self.is_labeled_by(
                            node,
                            node_util::id_text(source, label).unwrap_or_default(),
                        )
                    });
                    if labeled {
                        return Some(node);
                    }
                }
                // Don't cross function boundaries.
                kind if is_function_like_kind(kind) => return None,
                _ => {}
            }
            current = self.state.binder.node_record(node).parent;
        }
        None
    }

    /// tsgo `isLabeledBy`: the node is the statement of a label of the name
    /// (through nested labels).
    fn is_labeled_by(&self, node: NodeId, label_name: &str) -> bool {
        let mut owner = self.state.binder.node_record(node).parent;
        while let Some(id) = owner {
            let NodeData::LabeledStatement(data) = &self.state.binder.node_record(id).data else {
                return false;
            };
            let source = self.state.binder.source_of_node(id);
            if data
                .label
                .and_then(|label| node_util::id_text(source, label))
                == Some(label_name)
            {
                return true;
            }
            owner = self.state.binder.node_record(id).parent;
        }
        false
    }

    /// tsgo `getBreakOrContinueStatementOccurrences`.
    fn break_or_continue_statement_occurrences(&self, node: NodeId) -> Vec<Touching> {
        let Some(owner) = self.break_or_continue_owner(node) else {
            return Vec::new();
        };
        match self.node_kind(owner) {
            SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::DoStatement
            | SyntaxKind::WhileStatement => self.loop_break_continue_occurrences(owner),
            SyntaxKind::SwitchStatement => self.switch_case_default_occurrences(owner),
            _ => Vec::new(),
        }
    }

    /// tsgo `getLoopBreakContinueOccurrences`: the loop's keywords and the
    /// `break`s and `continue`s it owns.
    fn loop_break_continue_occurrences(&self, node: NodeId) -> Vec<Touching> {
        let mut keywords = Vec::new();
        if let Some(token) = self.first_token(node) {
            if matches!(
                self.kind(token),
                SyntaxKind::ForKeyword | SyntaxKind::DoKeyword | SyntaxKind::WhileKeyword
            ) {
                keywords.push(token);
                if self.node_kind(node) == SyntaxKind::DoStatement {
                    if let Some(&while_keyword) = self
                        .children_from_non_js_doc(node)
                        .iter()
                        .rev()
                        .find(|&&child| self.kind(child) == SyntaxKind::WhileKeyword)
                    {
                        keywords.push(while_keyword);
                    }
                }
            }
        }
        for statement in self.aggregate_all_break_and_continue_statements(node) {
            if let Some(token) = self.first_token(statement) {
                if self.owns_break_or_continue_statement(node, statement)
                    && matches!(
                        self.kind(token),
                        SyntaxKind::BreakKeyword | SyntaxKind::ContinueKeyword
                    )
                {
                    keywords.push(token);
                }
            }
        }
        keywords
    }

    /// tsgo `getAsyncAndAwaitOccurrences`: the containing function's `async`
    /// modifiers and `await` keywords.
    fn async_and_await_occurrences(&self, node: Touching) -> Vec<Touching> {
        // tsgo `ast.GetContainingFunction`.
        let Some(function) = self.find_ancestor(self.parent(node), |request, node| {
            is_function_like_kind(request.node_kind(node))
        }) else {
            return Vec::new();
        };
        let source = self.state.binder.source_of_node(function);
        let mut keywords: Vec<Touching> = node_util::modifiers_of(source, function)
            .map(|modifiers| self.state.binder.node_array(modifiers).nodes.to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|&modifier| self.node_kind(modifier) == SyntaxKind::AsyncKeyword)
            .map(Touching::Node)
            .collect();
        for child in self.children_of(function) {
            self.traverse_without_crossing_function(child, &mut |request, child| {
                if request.node_kind(child) == SyntaxKind::AwaitExpression {
                    if let Some(token) = request.first_token(child) {
                        if request.kind(token) == SyntaxKind::AwaitKeyword {
                            keywords.push(token);
                        }
                    }
                }
            });
        }
        keywords
    }

    /// tsgo `getYieldOccurrences`: the containing function's `yield`
    /// keywords.
    fn yield_occurrences(&self, node: Touching) -> Vec<Touching> {
        let Some(function) = self.find_ancestor(self.parent(node), |request, node| {
            is_function_like_kind(request.node_kind(node))
        }) else {
            return Vec::new();
        };
        let mut keywords = Vec::new();
        for child in self.children_of(function) {
            self.traverse_without_crossing_function(child, &mut |request, child| {
                if request.node_kind(child) == SyntaxKind::YieldExpression {
                    if let Some(token) = request.first_token(child) {
                        if request.kind(token) == SyntaxKind::YieldKeyword {
                            keywords.push(token);
                        }
                    }
                }
            });
        }
        keywords
    }

    /// tsgo `traverseWithoutCrossingFunction`.
    fn traverse_without_crossing_function(&self, node: NodeId, f: &mut dyn FnMut(&Self, NodeId)) {
        f(self, node);
        let kind = self.node_kind(node);
        if !is_function_like_kind(kind)
            && !matches!(
                kind,
                SyntaxKind::ClassDeclaration
                    | SyntaxKind::ClassExpression
                    | SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::ModuleDeclaration
                    | SyntaxKind::TypeAliasDeclaration
            )
            && !crate::completions::is_type_node_kind(kind)
        {
            for child in self.children_of(node) {
                self.traverse_without_crossing_function(child, f);
            }
        }
    }

    /// tsgo `getModifierOccurrences`: the modifiers of the kind in the
    /// declarations around the declaration.
    fn modifier_occurrences(&self, kind: SyntaxKind, declaration: NodeId) -> Vec<Touching> {
        self.nodes_to_search_for_modifier(declaration, node_util::modifier_to_flag(kind))
            .into_iter()
            .filter_map(|node| self.find_modifier(node, kind))
            .map(Touching::Node)
            .collect()
    }

    /// tsgo `getNodesToSearchForModifier`.
    fn nodes_to_search_for_modifier(
        &self,
        declaration: NodeId,
        modifier_flag: ModifierFlags,
    ) -> Vec<NodeId> {
        let Some(container) = self.state.binder.node_record(declaration).parent else {
            return Vec::new();
        };
        let list = |list: Option<tsc_syntax::NodeArrayId>| {
            list.map(|list| self.state.binder.node_array(list).nodes.to_vec())
                .unwrap_or_default()
        };
        // Types of node whose children might have modifiers.
        match &self.state.binder.node_record(container).data {
            NodeData::ModuleBlock(_)
            | NodeData::SourceFile(_)
            | NodeData::Block(_)
            | NodeData::CaseClause(_)
            | NodeData::DefaultClause(_) => {
                // Container is either a class declaration or the declaration is a classDeclaration
                if modifier_flag.intersects(ModifierFlags::ABSTRACT)
                    && self.node_kind(declaration) == SyntaxKind::ClassDeclaration
                {
                    let mut result = self.members_of(declaration);
                    result.push(declaration);
                    result
                } else {
                    list(match &self.state.binder.node_record(container).data {
                        NodeData::ModuleBlock(data) => data.statements,
                        NodeData::SourceFile(data) => data.statements,
                        NodeData::Block(data) => data.statements,
                        NodeData::CaseClause(data) => data.statements,
                        NodeData::DefaultClause(data) => data.statements,
                        _ => None,
                    })
                }
            }
            NodeData::Constructor(_)
            | NodeData::MethodDeclaration(_)
            | NodeData::FunctionDeclaration(_) => {
                // Parameters and, if inside a class, also class members
                let mut result = crate::utilities::parameters_of(self.state, container);
                if let Some(class) =
                    self.state
                        .binder
                        .node_record(container)
                        .parent
                        .filter(|&class| {
                            matches!(
                                self.node_kind(class),
                                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                            )
                        })
                {
                    result.extend(self.members_of(class));
                }
                result
            }
            NodeData::ClassDeclaration(_)
            | NodeData::ClassExpression(_)
            | NodeData::InterfaceDeclaration(_)
            | NodeData::TypeLiteral(_) => {
                let nodes = self.members_of(container);
                let mut result = nodes.clone();
                // If we're an accessibility modifier, we're in an instance member and should search
                // the constructor's parameter list for instance members as well.
                if modifier_flag
                    .intersects(ModifierFlags::ACCESSIBILITY_MODIFIER | ModifierFlags::READONLY)
                {
                    if let Some(&constructor) = nodes
                        .iter()
                        .find(|&&member| self.node_kind(member) == SyntaxKind::Constructor)
                    {
                        result.extend(crate::utilities::parameters_of(self.state, constructor));
                    }
                } else if modifier_flag.intersects(ModifierFlags::ABSTRACT) {
                    result.push(container);
                }
                result
            }
            // Syntactically invalid positions or unsupported containers
            _ => Vec::new(),
        }
    }

    /// tsgo `Node.Members` of a class, interface or type literal.
    fn members_of(&self, node: NodeId) -> Vec<NodeId> {
        let members = match &self.state.binder.node_record(node).data {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            NodeData::InterfaceDeclaration(data) => data.members,
            NodeData::TypeLiteral(data) => data.members,
            _ => None,
        };
        members
            .map(|members| self.state.binder.node_array(members).nodes.to_vec())
            .unwrap_or_default()
    }

    /// tsgo `findModifier`.
    fn find_modifier(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        let source = self.state.binder.source_of_node(node);
        node_util::modifiers_of(source, node).and_then(|modifiers| {
            self.state
                .binder
                .node_array(modifiers)
                .nodes
                .iter()
                .copied()
                .find(|&modifier| self.node_kind(modifier) == kind)
        })
    }

    /// tsgo `ast.IsIterationStatement`.
    fn is_iteration_statement(&self, node: NodeId, look_in_labeled_statements: bool) -> bool {
        match &self.state.binder.node_record(node).data {
            NodeData::ForStatement(_)
            | NodeData::ForInStatement(_)
            | NodeData::ForOfStatement(_)
            | NodeData::DoStatement(_)
            | NodeData::WhileStatement(_) => true,
            NodeData::LabeledStatement(data) => {
                look_in_labeled_statements
                    && data
                        .statement
                        .is_some_and(|statement| self.is_iteration_statement(statement, true))
            }
            _ => false,
        }
    }

    /// The child nodes (tsgo `Node.ForEachChild`).
    fn children_of(&self, node: NodeId) -> Vec<NodeId> {
        let source = self.state.binder.source_of_node(node);
        let mut children = Vec::new();
        tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
    }

    /// tsgo `getChildrenFromNonJSDocNode`.
    fn children_from_non_js_doc(&self, node: NodeId) -> Vec<Touching> {
        let source = self.state.binder.source_of_node(node);
        Navigator::new(source)
            .children_from_non_js_doc(View::Node(node))
            .into_iter()
            .filter_map(touching)
            .collect()
    }

    /// tsgo `lsutil.GetFirstToken`.
    fn first_token(&self, node: NodeId) -> Option<Touching> {
        let source = self.state.binder.source_of_node(node);
        Navigator::new(source)
            .first_token(View::Node(node))
            .and_then(touching)
    }

    /// tsgo `astnav.FindChildOfKind` of a keyword.
    fn child_keyword(&self, node: NodeId, kind: SyntaxKind) -> Option<Touching> {
        let source = self.state.binder.source_of_node(node);
        Navigator::new(source)
            .child_of_kind(View::Node(node), kind)
            .and_then(touching)
    }

    /// A node's or token's end.
    fn touching_end(&self, node: Touching) -> u32 {
        match node {
            Touching::Node(id) => self.state.binder.node_record(id).end,
            Touching::Token { end, .. } => end,
        }
    }
}

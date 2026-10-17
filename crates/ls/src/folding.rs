//! tsgo `ls/folding.go`: a file's folding ranges (`textDocument/foldingRange`)
//! — the blocks, lists, literals and JSX of its nodes, its runs of imports
//! and comments, and its `#region`s.

use tsc_binder::node_util;
use tsc_diagnostics::spanmap::FEATURE_FOLDING_RANGES;
use tsc_emitter::source_mapper::compute_ecma_line_starts;
use tsc_lsproto::{
    DocumentUri, FoldingRange, FoldingRangeKind, FoldingRangeResponse, FoldingRangesOrNull,
    Position,
};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};

use crate::astnav::{Found, Navigator};
use crate::language_service::{LanguageService, LsError};
use crate::lsconv::Converters;
use crate::tree::View;
use crate::utilities::{is_in_comment, script_of};

/// tsgo's outlining depth limit.
const MAX_DEPTH: i32 = 40;

impl LanguageService<'_> {
    /// tsgo `ProvideFoldingRange`: the nodes' and regions' ranges of the
    /// file and its content mapper's supplemental outputs (their ends kept
    /// off the closing line for a client that folds whole lines; a mapped
    /// output's only where they map to folding segments), in order, each
    /// once.
    pub fn provide_folding_ranges(
        &mut self,
        uri: &DocumentUri,
    ) -> Result<FoldingRangeResponse, LsError> {
        let file = self.file(uri);
        let capabilities = &self.capabilities.text_document.folding_range;
        let collapsed_text = capabilities.folding_range.collapsed_text;
        let line_folding_only = capabilities.line_folding_only;
        let converters = self.converters;
        let mut ranges = self
            .with_program_request(false, |request| {
                // The file's and its content mapper's supplemental outputs'.
                let mut ranges = Vec::new();
                for projection in request.source_projections(file) {
                    let source = request.state.binder.source(projection);
                    let outlining = Outlining::new(source, converters, collapsed_text);
                    let mut projection_ranges = outlining.node_outlining_spans();
                    projection_ranges.extend(outlining.region_outlining_spans());
                    if line_folding_only {
                        outlining.adjust_folding_end(&mut projection_ranges);
                    }
                    ranges.extend(projection_ranges);
                }
                Ok(ranges)
            })?
            .unwrap_or_default();
        ranges.sort_by_key(|range| {
            (
                range.start_line,
                range.start_character,
                range.end_line,
                range.end_character,
            )
        });
        let mut seen = std::collections::HashSet::new();
        ranges.retain(|range| {
            seen.insert((
                range.start_line,
                range.start_character,
                range.end_line,
                range.end_character,
                range.kind.clone().map(|kind| kind.0),
                range.collapsed_text.clone(),
            ))
        });
        Ok(FoldingRangesOrNull::FoldingRanges(ranges))
    }
}

/// tsgo's region delimiter: `// #region name` or `// #endregion`.
struct RegionDelimiter {
    is_start: bool,
    name: String,
}

/// One file's outlining.
struct Outlining<'a> {
    source: &'a SourceFile,
    navigator: Navigator<'a>,
    converters: &'a Converters,
    line_starts: Vec<usize>,
    /// tsgo `supportsCollapsedText`.
    collapsed_text: bool,
}

impl<'a> Outlining<'a> {
    fn new(source: &'a SourceFile, converters: &'a Converters, collapsed_text: bool) -> Self {
        Self {
            source,
            navigator: Navigator::new(source),
            converters,
            line_starts: compute_ecma_line_starts(source.text()),
            collapsed_text,
        }
    }

    fn text(&self) -> &'a str {
        self.source.text()
    }

    fn kind(&self, node: NodeId) -> SyntaxKind {
        self.source.arena.node(node).kind
    }

    fn data(&self, node: NodeId) -> &'a NodeData {
        &self.source.arena.node(node).data
    }

    fn pos(&self, node: NodeId) -> usize {
        self.source.arena.node(node).pos as usize
    }

    fn end(&self, node: NodeId) -> usize {
        self.source.arena.node(node).end as usize
    }

    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.source.arena.node(node).parent
    }

    fn list(&self, list: Option<NodeArrayId>) -> &'a [NodeId] {
        list.map_or(&[], |list| self.source.arena.node_array(list).nodes)
    }

    fn list_range(&self, list: NodeArrayId) -> (usize, usize) {
        let array = self.source.arena.node_array(list);
        (array.pos as usize, array.end as usize)
    }

    /// tsgo `GetStartOfNode(found, file, false)`.
    fn start(&self, found: Found) -> usize {
        self.navigator.start_of_node(found, false) as usize
    }

    /// tsgo `FindChildOfKind`.
    fn child_of_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<Found> {
        self.navigator.child_of_kind(View::Node(node), kind)
    }

    fn line_of(&self, position: usize) -> usize {
        crate::utilities::line_of_position(&self.line_starts, position)
    }

    /// tsgo `printer.PositionsAreOnSameLine`.
    fn on_same_line(&self, first: usize, second: usize) -> bool {
        self.line_of(first) == self.line_of(second)
    }

    /// tsgo `createFoldingRange`: the range, its kind and (for a client
    /// that shows it) its collapsed text.
    fn folding_range(
        &self,
        start: usize,
        end: usize,
        kind: Option<FoldingRangeKind>,
        collapsed_text: &str,
    ) -> Option<FoldingRange> {
        // tsgo `createFoldingRangeFromBounds`: none where a content mapper's
        // output's range is not in folding segments.
        let (range, fidelity) = self.converters.to_lsp_range_for_feature(
            &script_of(self.source),
            start..end,
            FEATURE_FOLDING_RANGES,
        );
        if fidelity.is_none() {
            return None;
        }
        Some(FoldingRange {
            start_line: range.start.line,
            start_character: Some(range.start.character),
            end_line: range.end.line,
            end_character: Some(range.end.character),
            kind,
            collapsed_text: (!collapsed_text.is_empty() && self.collapsed_text)
                .then(|| collapsed_text.to_owned()),
        })
    }

    /// tsgo `addNodeOutliningSpans`: each statement's spans, a run of two
    /// or more imports from the first's `import` keyword, and the comments
    /// before the end of the file.
    fn node_outlining_spans(&self) -> Vec<FoldingRange> {
        let mut ranges = Vec::new();
        let NodeData::SourceFile(data) = self.data(self.source.root) else {
            return ranges;
        };
        let statements = self.list(data.statements);
        let is_import = |statement: NodeId| {
            matches!(
                self.kind(statement),
                SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
            )
        };
        let mut current = 0;
        while current < statements.len() {
            while current < statements.len() && !is_import(statements[current]) {
                self.visit_node(statements[current], MAX_DEPTH, &mut ranges);
                current += 1;
            }
            if current == statements.len() {
                break;
            }
            let first_import = current;
            self.visit_node(statements[current], MAX_DEPTH, &mut ranges);
            current += 1;
            while current < statements.len()
                && is_import(statements[current])
                && !self.reparses_declarations(statements[current])
            {
                self.visit_node(statements[current], MAX_DEPTH, &mut ranges);
                current += 1;
            }
            let last_import = current - 1;
            if last_import != first_import {
                let keyword = self
                    .child_of_kind(statements[first_import], SyntaxKind::ImportKeyword)
                    .map_or(self.pos(statements[first_import]), |keyword| {
                        self.start(keyword)
                    });
                ranges.extend(self.folding_range(
                    keyword,
                    self.end(statements[last_import]),
                    Some(FoldingRangeKind::IMPORTS),
                    "",
                ));
            }
        }
        // The end of the file, for the comments no statement has.
        if let Some(end_of_file) = data.end_of_file_token {
            self.visit_node(end_of_file, MAX_DEPTH, &mut ranges);
        }
        ranges
    }

    /// Whether tsgo's parser reparses declarations from the statement's
    /// JSDoc in a JavaScript file (a `@typedef` or `@callback` with a type, an
    /// `@import` with an import clause): it lists them before the statement,
    /// and they end a run of imports.
    fn reparses_declarations(&self, statement: NodeId) -> bool {
        let root = self.source.arena.node(self.source.root);
        if !tsc_types::NodeFlags::from_bits(root.flags)
            .contains(tsc_types::NodeFlags::JAVA_SCRIPT_FILE)
        {
            return false;
        }
        let Some(js_doc) = self.source.arena.node(statement).js_doc else {
            return false;
        };
        self.source
            .arena
            .node_array(js_doc)
            .nodes
            .iter()
            .filter_map(|&comment| match self.data(comment) {
                NodeData::JSDoc(data) => Some(self.list(data.tags)),
                _ => None,
            })
            .flatten()
            .any(|&tag| match self.data(tag) {
                NodeData::JSDocTypedefTag(data) => data.type_expression.is_some(),
                NodeData::JSDocCallbackTag(data) => data.type_expression.is_some(),
                NodeData::JSDocImportTag(data) => data.import_clause.is_some(),
                _ => false,
            })
    }

    /// tsgo `addRegionOutliningSpans`: each `// #region` (not in a comment)
    /// to its `// #endregion`, named by its text or `#region`.
    fn region_outlining_spans(&self) -> Vec<FoldingRange> {
        let mut regions: Vec<(usize, String)> = Vec::new();
        let mut ranges = Vec::new();
        for &line_start in &self.line_starts {
            let line_end =
                crate::utilities::line_end_of_position(self.source, &self.line_starts, line_start);
            let line_text = &self.text()[line_start..line_end];
            let Some(delimiter) = parse_region_delimiter(line_text) else {
                continue;
            };
            let token = self.navigator.token_at(line_start as u32);
            if is_in_comment(&self.navigator, line_start as u32, token) {
                continue;
            }
            if delimiter.is_start {
                let position = line_text.find("//").unwrap_or(0) + line_start;
                let name = if delimiter.name.is_empty() {
                    "#region".to_owned()
                } else {
                    delimiter.name
                };
                regions.push((position, name));
            } else if let Some((position, name)) = regions.pop() {
                if let Some(mut range) =
                    self.folding_range(position, line_end, Some(FoldingRangeKind::REGION), "")
                {
                    range.collapsed_text = self.collapsed_text.then_some(name);
                    ranges.push(range);
                }
            }
        }
        ranges
    }

    /// tsgo `adjustFoldingEnd`: for a client that folds whole lines, a
    /// range that ends with a closing bracket, brace, paren, backtick or `>`
    /// ends a line earlier (the closing line stays visible).
    fn adjust_folding_end(&self, ranges: &mut [FoldingRange]) {
        let text = self.text().as_bytes();
        for range in ranges {
            let Some(end_character) = range.end_character.filter(|&character| character > 0) else {
                continue;
            };
            // tsgo: the end's first projection into this output that maps.
            let script = script_of(self.source);
            let Some(position) = self
                .converters
                .from_lsp_position_for_scripts(
                    &[&script as &dyn crate::lsconv::Script],
                    &Position {
                        line: range.end_line,
                        character: end_character,
                    },
                    FEATURE_FOLDING_RANGES,
                )
                .into_iter()
                .find(|projection| !projection.fidelity.is_none())
                .map(|projection| projection.position)
            else {
                continue;
            };
            if position > 0
                && position <= text.len()
                && matches!(text[position - 1], b'}' | b']' | b')' | b'`' | b'>')
                && range.end_line > range.start_line
            {
                range.end_line -= 1;
            }
        }
    }

    /// tsgo `visitNode`: a node's leading comments and span, then its
    /// children's (a call's expression at its depth, an `else if` at its
    /// `if`'s), down to the depth limit.
    fn visit_node(&self, node: NodeId, depth_remaining: i32, ranges: &mut Vec<FoldingRange>) {
        if depth_remaining == 0 {
            return;
        }
        let kind = self.kind(node);
        if kind != SyntaxKind::BinaryExpression && node_util::is_declaration_node(self.source, node)
            || matches!(
                kind,
                SyntaxKind::VariableStatement
                    | SyntaxKind::ReturnStatement
                    | SyntaxKind::CallExpression
                    | SyntaxKind::NewExpression
                    | SyntaxKind::EndOfFileToken
            )
        {
            self.leading_comments_of_node(node, ranges);
        }
        if crate::utilities::is_function_like_kind(kind) {
            if let Some(parent) = self.parent(node) {
                if let NodeData::BinaryExpression(binary) = self.data(parent) {
                    if let Some(left) = binary
                        .left
                        .filter(|&left| self.kind(left) == SyntaxKind::PropertyAccessExpression)
                    {
                        self.leading_comments_of_node(left, ranges);
                    }
                }
            }
        }
        match self.data(node) {
            NodeData::Block(data) => {
                if let Some(statements) = data.statements {
                    self.leading_comments_at(self.list_range(statements).1, ranges);
                }
            }
            NodeData::ModuleBlock(data) => {
                if let Some(statements) = data.statements {
                    self.leading_comments_at(self.list_range(statements).1, ranges);
                }
            }
            NodeData::ClassDeclaration(data) => {
                if let Some(members) = data.members {
                    self.leading_comments_at(self.list_range(members).1, ranges);
                }
            }
            NodeData::ClassExpression(data) => {
                if let Some(members) = data.members {
                    self.leading_comments_at(self.list_range(members).1, ranges);
                }
            }
            NodeData::InterfaceDeclaration(data) => {
                if let Some(members) = data.members {
                    self.leading_comments_at(self.list_range(members).1, ranges);
                }
            }
            _ => {}
        }
        if let Some(span) = self.outlining_span_for_node(node) {
            ranges.push(span);
        }
        let depth = depth_remaining - 1;
        match self.data(node) {
            NodeData::CallExpression(data) => {
                if let Some(expression) = data.expression {
                    self.visit_node(expression, depth + 1, ranges);
                }
                for &argument in self.list(data.arguments) {
                    self.visit_node(argument, depth, ranges);
                }
                for &type_argument in self.list(data.type_arguments) {
                    self.visit_node(type_argument, depth, ranges);
                }
            }
            NodeData::IfStatement(data)
                if data
                    .else_statement
                    .is_some_and(|other| self.kind(other) == SyntaxKind::IfStatement) =>
            {
                // An `else if` is at the `if`'s depth.
                if let Some(expression) = data.expression {
                    self.visit_node(expression, depth, ranges);
                }
                if let Some(then) = data.then_statement {
                    self.visit_node(then, depth, ranges);
                }
                if let Some(other) = data.else_statement {
                    self.visit_node(other, depth + 1, ranges);
                }
            }
            _ => {
                let mut children = Vec::new();
                tsc_syntax::for_each_child(
                    &self.source.arena,
                    self.source.arena.node(node),
                    |child| {
                        children.push(child);
                        false
                    },
                );
                for child in children {
                    self.visit_node(child, depth, ranges);
                }
            }
        }
    }

    /// tsgo `addOutliningForLeadingCommentsForNode`.
    fn leading_comments_of_node(&self, node: NodeId, ranges: &mut Vec<FoldingRange>) {
        if self.kind(node) == SyntaxKind::JsxText {
            return;
        }
        self.leading_comments_at(self.pos(node), ranges);
    }

    /// tsgo `addOutliningForLeadingCommentsForPos`: each multi-line comment,
    /// and each run of two or more single-line comments (a region delimiter
    /// ends a run and joins none).
    fn leading_comments_at(&self, pos: usize, ranges: &mut Vec<FoldingRange>) {
        let text = self.text();
        let mut first_start = 0;
        let mut last_end = 0;
        let mut count = 0;
        let flush =
            |count: usize, first_start: usize, last_end: usize, ranges: &mut Vec<FoldingRange>| {
                // Only two or more consecutive single-line comments fold.
                if count > 1 {
                    ranges.extend(self.folding_range(
                        first_start,
                        last_end,
                        Some(FoldingRangeKind::COMMENT),
                        "",
                    ));
                }
            };
        for (start, end) in tsc_syntax::comment_ranges(text, pos, false) {
            if text[start..].starts_with("//") {
                // Never fold region delimiters into single-line comment
                // regions.
                if parse_region_delimiter(&text[start..end]).is_some() {
                    flush(count, first_start, last_end, ranges);
                    count = 0;
                    continue;
                }
                if count == 0 {
                    first_start = start;
                }
                last_end = end;
                count += 1;
            } else {
                flush(count, first_start, last_end, ranges);
                ranges.extend(self.folding_range(start, end, Some(FoldingRangeKind::COMMENT), ""));
                count = 0;
            }
        }
        flush(count, first_start, last_end, ranges);
    }

    /// tsgo `getOutliningSpanForNode`.
    fn outlining_span_for_node(&self, node: NodeId) -> Option<FoldingRange> {
        let parent = self.parent(node);
        let parent_kind = parent.map(|parent| self.kind(parent));
        match self.kind(node) {
            SyntaxKind::Block => {
                let parent = parent?;
                if crate::utilities::is_function_like_kind(self.kind(parent)) {
                    return self.function_span(parent, node);
                }
                // A block attached to a statement collapses with its own
                // span; a standalone block only its own.
                match self.data(parent) {
                    NodeData::DoStatement(_)
                    | NodeData::ForInStatement(_)
                    | NodeData::ForOfStatement(_)
                    | NodeData::ForStatement(_)
                    | NodeData::IfStatement(_)
                    | NodeData::WhileStatement(_)
                    | NodeData::WithStatement(_)
                    | NodeData::CatchClause(_) => {
                        return self.span_for_node(node, SyntaxKind::OpenBraceToken, true);
                    }
                    NodeData::TryStatement(data) if data.try_block == Some(node) => {
                        return self.span_for_node(node, SyntaxKind::OpenBraceToken, true);
                    }
                    NodeData::TryStatement(data) if data.finally_block == Some(node) => {
                        if let Some(span) =
                            self.span_for_node(node, SyntaxKind::OpenBraceToken, true)
                        {
                            return Some(span);
                        }
                    }
                    _ => {}
                }
                let start = self.start(Found::Node(View::Node(node)));
                self.folding_range(start, self.end(node), None, "")
            }
            SyntaxKind::ModuleBlock
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::CaseBlock
            | SyntaxKind::TypeLiteral
            | SyntaxKind::ObjectBindingPattern => {
                self.span_for_node(node, SyntaxKind::OpenBraceToken, true)
            }
            SyntaxKind::TupleType => self.span_for_node(
                node,
                SyntaxKind::OpenBracketToken,
                parent_kind != Some(SyntaxKind::TupleType),
            ),
            SyntaxKind::CaseClause | SyntaxKind::DefaultClause => {
                let statements = match self.data(node) {
                    NodeData::CaseClause(data) => data.statements,
                    NodeData::DefaultClause(data) => data.statements,
                    _ => None,
                }?;
                if self.list(Some(statements)).is_empty() {
                    return None;
                }
                let (pos, end) = self.list_range(statements);
                self.folding_range(pos, end, None, "")
            }
            SyntaxKind::ObjectLiteralExpression => self.span_for_node(
                node,
                SyntaxKind::OpenBraceToken,
                !matches!(
                    parent_kind,
                    Some(SyntaxKind::ArrayLiteralExpression | SyntaxKind::CallExpression)
                ),
            ),
            SyntaxKind::ArrayLiteralExpression => self.span_for_node(
                node,
                SyntaxKind::OpenBracketToken,
                !matches!(
                    parent_kind,
                    Some(SyntaxKind::ArrayLiteralExpression | SyntaxKind::CallExpression)
                ),
            ),
            SyntaxKind::JsxElement | SyntaxKind::JsxFragment => self.span_for_jsx_element(node),
            SyntaxKind::JsxSelfClosingElement | SyntaxKind::JsxOpeningElement => {
                self.span_for_jsx_attributes(node)
            }
            SyntaxKind::TemplateExpression | SyntaxKind::NoSubstitutionTemplateLiteral => {
                if let NodeData::NoSubstitutionTemplateLiteral(data) = self.data(node) {
                    if data.text.is_empty() {
                        return None;
                    }
                }
                let start = self.start(Found::Node(View::Node(node)));
                self.folding_range(start, self.end(node), None, "")
            }
            SyntaxKind::ArrayBindingPattern => self.span_for_node(
                node,
                SyntaxKind::OpenBracketToken,
                parent_kind != Some(SyntaxKind::BindingElement),
            ),
            SyntaxKind::ArrowFunction => {
                let NodeData::ArrowFunction(data) = self.data(node) else {
                    return None;
                };
                let body = data.body?;
                if matches!(
                    self.kind(body),
                    SyntaxKind::Block | SyntaxKind::ParenthesizedExpression
                ) || self.on_same_line(self.pos(body), self.end(body))
                {
                    return None;
                }
                self.folding_range(self.pos(body), self.end(body), None, "")
            }
            SyntaxKind::CallExpression => {
                let NodeData::CallExpression(data) = self.data(node) else {
                    return None;
                };
                if self.list(data.arguments).is_empty() {
                    return None;
                }
                let open = self.child_of_kind(node, SyntaxKind::OpenParenToken)?;
                let close = self.child_of_kind(node, SyntaxKind::CloseParenToken)?;
                if self.on_same_line(
                    self.navigator.pos(open) as usize,
                    self.navigator.pos(close) as usize,
                ) {
                    return None;
                }
                self.range_between_tokens(open, close, true)
            }
            SyntaxKind::ParenthesizedExpression => {
                let start = self.start(Found::Node(View::Node(node)));
                if self.on_same_line(start, self.end(node)) {
                    return None;
                }
                self.folding_range(start, self.end(node), None, "")
            }
            SyntaxKind::NamedImports | SyntaxKind::NamedExports | SyntaxKind::ImportAttributes => {
                let elements = match self.data(node) {
                    NodeData::NamedImports(data) => data.elements,
                    NodeData::NamedExports(data) => data.elements,
                    NodeData::ImportAttributes(data) => data.elements,
                    _ => None,
                };
                if self.list(elements).is_empty() {
                    return None;
                }
                let open = self.child_of_kind(node, SyntaxKind::OpenBraceToken)?;
                let close = self.child_of_kind(node, SyntaxKind::CloseBraceToken)?;
                if self.on_same_line(
                    self.navigator.pos(open) as usize,
                    self.navigator.pos(close) as usize,
                ) {
                    return None;
                }
                self.range_between_tokens(open, close, false)
            }
            _ => None,
        }
    }

    /// tsgo `spanForJSXElement`: an element from its opening tag's start to
    /// its closing tag's end, collapsed as `<tag>...</tag>` (`<>...</>`).
    fn span_for_jsx_element(&self, node: NodeId) -> Option<FoldingRange> {
        match self.data(node) {
            NodeData::JsxElement(data) => {
                let opening = data.opening_element?;
                let closing = data.closing_element?;
                let tag_name = match self.data(opening) {
                    NodeData::JsxOpeningElement(opening) => opening.tag_name,
                    _ => None,
                }?;
                let start = self.start(Found::Node(View::Node(opening)));
                let tag = self.text_of_node(tag_name);
                let banner = format!("<{tag}>...</{tag}>");
                self.folding_range(start, self.end(closing), None, &banner)
            }
            NodeData::JsxFragment(data) => {
                let opening = data.opening_fragment?;
                let closing = data.closing_fragment?;
                let start = self.start(Found::Node(View::Node(opening)));
                self.folding_range(start, self.end(closing), None, "<>...</>")
            }
            _ => None,
        }
    }

    /// tsgo `spanForJSXAttributes`: an opening or self-closing element with
    /// attributes.
    fn span_for_jsx_attributes(&self, node: NodeId) -> Option<FoldingRange> {
        let attributes = match self.data(node) {
            NodeData::JsxSelfClosingElement(data) => data.attributes,
            NodeData::JsxOpeningElement(data) => data.attributes,
            _ => None,
        }?;
        let NodeData::JsxAttributes(data) = self.data(attributes) else {
            return None;
        };
        if self.list(data.properties).is_empty() {
            return None;
        }
        let start = self.start(Found::Node(View::Node(node)));
        self.folding_range(start, self.end(node), None, "")
    }

    /// tsgo `spanForNode`: from the open token (its full start with
    /// `use_full_start`) to the matching close token.
    fn span_for_node(
        &self,
        node: NodeId,
        open: SyntaxKind,
        use_full_start: bool,
    ) -> Option<FoldingRange> {
        let close = if open == SyntaxKind::OpenBraceToken {
            SyntaxKind::CloseBraceToken
        } else {
            SyntaxKind::CloseBracketToken
        };
        let open = self.child_of_kind(node, open)?;
        let close = self.child_of_kind(node, close)?;
        self.range_between_tokens(open, close, use_full_start)
    }

    /// tsgo `rangeBetweenTokens`.
    fn range_between_tokens(
        &self,
        open: Found,
        close: Found,
        use_full_start: bool,
    ) -> Option<FoldingRange> {
        let start = if use_full_start {
            self.navigator.pos(open) as usize
        } else {
            self.start(open)
        };
        self.folding_range(start, self.navigator.end(close) as usize, None, "")
    }

    /// tsgo `functionSpan`: a function's body from its open brace (its
    /// open paren when its parameters take several lines).
    fn function_span(&self, function: NodeId, body: NodeId) -> Option<FoldingRange> {
        let parameters = crate::utilities::parameters_in(self.source, function);
        // tsgo's reparser puts a parameter made of a JavaScript function's
        // `@this` tag first in the list, at the tag name's position
        // (reparser.go:491-512), so with a parameter on the function's line
        // the list takes several lines.
        let this_tag = tsc_binder::hosted::jsdoc_hosted(self.source)
            .this_tag_of(function)
            .map(|tag| match &self.source.arena.node(tag).data {
                NodeData::JSDocThisTag(data) => data.tag_name.unwrap_or(tag),
                _ => tag,
            });
        let first = this_tag.or(parameters.first().copied());
        let last = parameters.last().copied().or(this_tag);
        let multi_line = match (first, last) {
            (Some(first), Some(last)) => !self.on_same_line(self.pos(first), self.end(last)),
            _ => false,
        };
        let open = if multi_line {
            self.child_of_kind(function, SyntaxKind::OpenParenToken)
        } else {
            None
        }
        .or_else(|| self.child_of_kind(body, SyntaxKind::OpenBraceToken))?;
        let close = self.child_of_kind(body, SyntaxKind::CloseBraceToken)?;
        self.range_between_tokens(open, close, true)
    }

    /// tsgo `scanner.GetTextOfNode` (a missing node's is empty).
    fn text_of_node(&self, node: NodeId) -> &'a str {
        if self.pos(node) == self.end(node) {
            return "";
        }
        let start = self.start(Found::Node(View::Node(node)));
        &self.text()[start..self.end(node)]
    }
}

/// tsgo `parseRegionDelimiter`: `// #region name` or `// #endregion`.
fn parse_region_delimiter(line_text: &str) -> Option<RegionDelimiter> {
    let line_text = line_text.trim_start();
    let line_text = line_text.strip_prefix("//")?;
    let line_text = line_text.trim();
    let line_text = line_text.strip_suffix('\r').unwrap_or(line_text);
    let line_text = line_text.strip_prefix('#')?;
    let (is_start, line_text) = match line_text.strip_prefix("end") {
        Some(rest) => (false, rest),
        None => (true, line_text),
    };
    let name = line_text.strip_prefix("region")?;
    Some(RegionDelimiter {
        is_start,
        name: name.trim().to_owned(),
    })
}

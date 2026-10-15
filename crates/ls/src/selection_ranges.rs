//! tsgo `ls/selectionranges.go`: the nested ranges around positions
//! (`textDocument/selectionRange`) — each node that holds the position, its
//! lists, its strings' insides, its single-line comments, from the file down
//! to the innermost.

use tsc_lsproto::{
    Range, SelectionRange, SelectionRangeParams, SelectionRangeResponse, SelectionRangesOrNull,
};
use tsc_syntax::{SourceFile, SyntaxKind};

use crate::astnav::{Found, Navigator};
use crate::language_service::{LanguageService, LsError};
use crate::lsconv::Converters;
use crate::tree::{Property, View};
use crate::utilities::script_of;

/// A mapped type's selection child: a node or token, or a syntax list of
/// them (tsgo `createSyntaxList`).
#[derive(Clone)]
enum SelectionChild {
    Item(Found),
    List(Vec<SelectionChild>),
}

/// tsgo `groupChildren`: each run of children `group_on` takes, as a syntax
/// list.
fn group_children(
    children: Vec<SelectionChild>,
    group_on: impl Fn(&SelectionChild) -> bool,
) -> Vec<SelectionChild> {
    let mut result = Vec::new();
    let mut group = Vec::new();
    for child in children {
        if group_on(&child) {
            group.push(child);
        } else {
            if !group.is_empty() {
                result.push(SelectionChild::List(std::mem::take(&mut group)));
            }
            result.push(child);
        }
    }
    if !group.is_empty() {
        result.push(SelectionChild::List(group));
    }
    result
}

/// tsgo `splitChildren` (no trailing semicolon apart): the children before
/// the first pivot as a syntax list, the pivot, and the rest as one.
fn split_children(
    children: Vec<SelectionChild>,
    pivot_on: impl Fn(&SelectionChild) -> bool,
) -> Vec<SelectionChild> {
    if children.len() < 2 {
        return children;
    }
    let Some(split) = children.iter().position(&pivot_on) else {
        return children;
    };
    let mut children = children;
    let right: Vec<SelectionChild> = children.split_off(split + 1);
    let pivot = children.pop().expect("the pivot");
    let mut result = Vec::new();
    if !children.is_empty() {
        result.push(SelectionChild::List(children));
    }
    result.push(pivot);
    if !right.is_empty() {
        result.push(SelectionChild::List(right));
    }
    result
}

/// A child as tsgo's visitor visits it: a node, or a list's elements.
enum Child {
    Node(View),
    List(Vec<View>),
}

/// tsgo `maxSelectionRangeDepth`.
const MAX_SELECTION_RANGE_DEPTH: usize = 1000;

impl LanguageService<'_> {
    /// tsgo `ProvideSelectionRanges` (a file without a content mapper).
    pub fn provide_selection_ranges(
        &mut self,
        params: &SelectionRangeParams,
    ) -> Result<SelectionRangeResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let converters = self.converters;
        let results = self
            .with_request_at(file, 0, false, |request, _| {
                let source = request.state.binder.source(file);
                let script = script_of(source);
                let mut results = Vec::new();
                for position in &params.positions {
                    let pos = converters.from_lsp_position(&script, position);
                    results.push(SmartSelection::new(source, converters, pos).range());
                }
                Ok(results)
            })?
            .unwrap_or_default();
        Ok(SelectionRangesOrNull::SelectionRanges(results))
    }
}

/// tsgo `getSmartSelectionRange`'s walk for one position.
struct SmartSelection<'a> {
    source: &'a SourceFile,
    navigator: Navigator<'a>,
    converters: &'a Converters,
    line_starts: Vec<usize>,
    pos: usize,
    /// tsgo `selectionRangeBuilder`: the newest ranges, at most
    /// `maxSelectionRangeDepth - 1` of them.
    ranges: std::collections::VecDeque<Range>,
    last_range: Range,
}

impl<'a> SmartSelection<'a> {
    fn new(source: &'a SourceFile, converters: &'a Converters, pos: usize) -> Self {
        let full = source.arena.node(source.root);
        let last_range =
            converters.to_lsp_range(&script_of(source), full.pos as usize..full.end as usize);
        Self {
            source,
            navigator: Navigator::new(source),
            converters,
            line_starts: tsc_emitter::source_mapper::compute_ecma_line_starts(source.text()),
            pos,
            ranges: std::collections::VecDeque::new(),
            last_range,
        }
    }

    /// The ranges from the innermost to the file's.
    fn range(mut self) -> SelectionRange {
        let root = self.last_range.clone();
        let mut current = Some(View::Node(self.source.root));
        while let Some(view) = current {
            current = self.step(view);
        }
        let mut result = SelectionRange {
            range: root,
            parent: None,
        };
        for range in self.ranges {
            result = SelectionRange {
                range,
                parent: Some(Box::new(result)),
            };
        }
        result
    }

    /// One level of the walk: the node's JSDoc comments, then its children
    /// and lists; the child that holds the position is the next level.
    fn step(&mut self, current: View) -> Option<View> {
        let mut next = None;
        for child in self.children_of(current) {
            match child {
                Child::Node(node) => self.visit(node, current, &mut next),
                Child::List(elements) => self.visit_nodes(&elements, current, &mut next),
            }
        }
        next
    }

    /// A node's JSDoc comments, then its children as tsgo's visitor sees
    /// them: nodes, lists, and a modifier list's modifiers one by one.
    fn children_of(&self, current: View) -> Vec<Child> {
        let tree = self.navigator.tree();
        let mut children = Vec::new();
        let list_elements = |view: View| {
            let mut elements = Vec::new();
            tree.children(view, &mut elements);
            elements
        };
        match current {
            View::Node(id) => {
                let mut js_docs = Vec::new();
                tree.js_doc_views(id, &mut js_docs);
                children.extend(js_docs.into_iter().map(Child::Node));
                tree.properties(tree.node(id), |property| match property {
                    Property::Node(Some(child)) => children.push(Child::Node(View::Node(child))),
                    Property::List(Some(list)) => children.push(Child::List(list_elements(
                        View::List(list, crate::tree::Elements::Plain),
                    ))),
                    Property::HeritageTypes(Some(list)) => children.push(Child::List(
                        list_elements(View::List(list, crate::tree::Elements::HeritageTypes)),
                    )),
                    Property::Modifiers(Some(list)) | Property::Slice(Some(list)) => {
                        children.extend(
                            tree.arena
                                .node_array(list)
                                .nodes
                                .iter()
                                .map(|&element| Child::Node(View::Node(element))),
                        );
                    }
                    Property::Comment(Some(_)) => {
                        children.push(Child::List(list_elements(View::Comment(id))));
                    }
                    Property::ImplicitModifiers => {
                        children.push(Child::Node(View::ImplicitExport(tree.node(id).pos)));
                    }
                    _ => {}
                });
            }
            _ => {
                let mut views = Vec::new();
                tree.children(current, &mut views);
                for view in views {
                    children.push(match view {
                        View::List(..) | View::Comment(_) | View::ImplicitModifiers(_) => {
                            Child::List(list_elements(view))
                        }
                        view => Child::Node(view),
                    });
                }
            }
        }
        children
    }

    fn kind(&self, view: View) -> Option<SyntaxKind> {
        self.navigator.tree().kind(view)
    }

    fn start(&self, view: View, include_js_doc: bool) -> usize {
        self.navigator
            .start_of_node(Found::Node(view), include_js_doc) as usize
    }

    fn end(&self, view: View) -> usize {
        self.navigator.end(Found::Node(view)) as usize
    }

    fn line_of(&self, position: usize) -> usize {
        crate::utilities::line_of_position(&self.line_starts, position)
    }

    /// tsgo's `visit`: a single-line comment after the node, then — when the
    /// node holds the position — a function's multi-line body, a template
    /// span's `${ … }`, the node (unless skipped) and a string's inside; the
    /// node is the next level.
    fn visit(&mut self, node: View, parent: View, next: &mut Option<View>) {
        if next.is_some() {
            return;
        }
        let text = self.source.text();
        let end = self.end(node);
        if let Some(&(comment_start, comment_end)) =
            tsc_syntax::comment_ranges(text, end, true).first()
        {
            if text[comment_start..].starts_with("//") {
                self.push_comment_range(comment_start, comment_end);
            }
        }
        let start_with_js_doc = self.start(node, true);
        if !(start_with_js_doc <= self.pos && self.pos < end) {
            return;
        }
        let kind = self.kind(node);
        let parent_kind = self.kind(parent);
        // A multi-line function body has a range before the block is
        // skipped.
        if kind == Some(SyntaxKind::Block)
            && parent_kind.is_some_and(tsc_binder::node_util::is_function_like_declaration_kind)
        {
            let start = self.start(node, false);
            if self.line_of(start) != self.line_of(end) {
                self.push_range(start, end);
            }
        }
        // A stop for `${ … }`, whose `${` and `}` belong to its siblings.
        if parent_kind == Some(SyntaxKind::TemplateSpan) {
            if let Some(literal) = self.template_span_literal(parent) {
                let span_start = self.navigator.tree().range(node).0 as usize;
                if span_start >= 2 {
                    let span_start = span_start - 2;
                    let span_end = self.start(literal, false) + 1;
                    if span_end <= text.len() && span_start < span_end {
                        self.push_range(span_start, span_end);
                    }
                }
            }
        }
        if !self.should_skip_node(node, parent) {
            let start = self.start(node, false);
            self.push_range(start, end);
            if kind == Some(SyntaxKind::MappedType) {
                self.push_mapped_type_ranges(node);
            }
            // Strings have a stop both inside and outside their quotes.
            if matches!(
                kind,
                Some(
                    SyntaxKind::StringLiteral
                        | SyntaxKind::TemplateExpression
                        | SyntaxKind::NoSubstitutionTemplateLiteral
                )
            ) && start + 1 < end.saturating_sub(1)
            {
                self.push_range(start + 1, end - 1);
            }
        }
        *next = Some(node);
    }

    /// tsgo's walk down a mapped type's selection children: the child that
    /// holds the position (or ends at a touching name), down each syntax list.
    fn push_mapped_type_ranges(&mut self, node: View) {
        let mut children = self.mapped_type_selection_children(node);
        loop {
            let mut selected = None;
            for child in children {
                let child_start = self.child_start(&child);
                if child_start > self.pos {
                    break;
                }
                if self.position_should_snap_to(&child) {
                    let child_end = self.child_end(&child);
                    self.push_range(child_start, child_end);
                    selected = Some(child);
                    break;
                }
            }
            match selected {
                Some(SelectionChild::List(list)) => children = list,
                _ => break,
            }
        }
    }

    /// tsgo `getSelectionChildren` of a mapped type: `{`, a syntax list of
    /// the parts before the colon (the type parameter grouped with its
    /// brackets, `-readonly` and `-?` grouped) and after it, then `}`.
    fn mapped_type_selection_children(&self, node: View) -> Vec<SelectionChild> {
        let children: Vec<SelectionChild> = self
            .navigator
            .children_from_non_js_doc(node)
            .into_iter()
            .map(SelectionChild::Item)
            .collect();
        if children.len() < 2 {
            return children;
        }
        let first_kind = self.child_kind(&children[0]);
        let last_kind = self.child_kind(&children[children.len() - 1]);
        if first_kind != Some(SyntaxKind::OpenBraceToken)
            || last_kind != Some(SyntaxKind::CloseBraceToken)
        {
            return children;
        }
        let (readonly_token, question_token) =
            match node.node().map(|id| &self.navigator.tree().node(id).data) {
                Some(tsc_syntax::NodeData::MappedType(data)) => {
                    (data.readonly_token, data.question_token)
                }
                _ => (None, None),
            };
        let is_token = |child: &SelectionChild, token: Option<tsc_syntax::NodeId>| matches!(child, SelectionChild::Item(Found::Node(View::Node(id))) if Some(*id) == token);
        let middle = children[1..children.len() - 1].to_vec();
        // Group `-/+readonly` and `-/+?`.
        let grouped = group_children(middle, |child| {
            is_token(child, readonly_token)
                || self.child_kind(child) == Some(SyntaxKind::ReadonlyKeyword)
                || is_token(child, question_token)
                || self.child_kind(child) == Some(SyntaxKind::QuestionToken)
        });
        // Group the type parameter with its brackets.
        let grouped = group_children(grouped, |child| {
            matches!(
                self.child_kind(child),
                Some(
                    SyntaxKind::OpenBracketToken
                        | SyntaxKind::TypeParameter
                        | SyntaxKind::CloseBracketToken
                )
            )
        });
        let split = split_children(grouped, |child| {
            self.child_kind(child) == Some(SyntaxKind::ColonToken)
        });
        vec![
            children[0].clone(),
            SelectionChild::List(split),
            children[children.len() - 1].clone(),
        ]
    }

    fn child_kind(&self, child: &SelectionChild) -> Option<SyntaxKind> {
        match child {
            SelectionChild::Item(found) => Some(self.navigator.kind(*found)),
            SelectionChild::List(_) => None,
        }
    }

    /// tsgo `GetTokenPosOfNode(child, file, true)` (a syntax list's is its
    /// first child's).
    fn child_start(&self, child: &SelectionChild) -> usize {
        match child {
            SelectionChild::Item(found) => self.navigator.start_of_node(*found, true) as usize,
            SelectionChild::List(children) => self.child_start(&children[0]),
        }
    }

    fn child_end(&self, child: &SelectionChild) -> usize {
        match child {
            SelectionChild::Item(found) => self.navigator.end(*found) as usize,
            SelectionChild::List(children) => self.child_end(&children[children.len() - 1]),
        }
    }

    /// tsgo's `positionShouldSnapToNode`: the position is before the child's
    /// end, or at it with a name touching it that starts inside.
    fn position_should_snap_to(&self, child: &SelectionChild) -> bool {
        let end = self.child_end(child);
        if self.pos < end {
            return true;
        }
        if self.pos == end {
            let touching = self.navigator.touching_property_name(self.pos as u32);
            return (self.navigator.pos(touching) as usize) < end;
        }
        false
    }

    /// tsgo's `visitNodes`: a list's range (not a variable declaration
    /// list's or a template's), then its elements.
    fn visit_nodes(&mut self, elements: &[View], parent: View, next: &mut Option<View>) {
        if let (Some(&first), Some(&last)) = (elements.first(), elements.last()) {
            let skip = matches!(
                self.kind(parent),
                Some(SyntaxKind::VariableDeclarationList | SyntaxKind::TemplateExpression)
            );
            if !skip {
                let start = self.start(first, false);
                let end = self.end(last);
                if start <= self.pos && self.pos < end {
                    self.push_range(start, end);
                }
            }
        }
        for &element in elements {
            self.visit(element, parent, next);
        }
    }

    /// tsgo's `shouldSkipNode`: blocks, template pieces, a variable
    /// statement's declaration list, a lone variable declaration and JSDoc
    /// type wrappers have no range of their own.
    fn should_skip_node(&self, node: View, parent: View) -> bool {
        let tree = self.navigator.tree();
        match self.kind(node) {
            Some(
                SyntaxKind::Block
                | SyntaxKind::TemplateSpan
                | SyntaxKind::TemplateHead
                | SyntaxKind::TemplateTail
                | SyntaxKind::JSDocTypeExpression
                | SyntaxKind::JSDocSignature
                | SyntaxKind::JSDocTypeLiteral,
            ) => true,
            Some(SyntaxKind::VariableDeclarationList) => {
                self.kind(parent) == Some(SyntaxKind::VariableStatement)
            }
            // A lone variable declaration.
            Some(SyntaxKind::VariableDeclaration) => {
                match parent.node().map(|id| &tree.node(id).data) {
                    Some(tsc_syntax::NodeData::VariableDeclarationList(data)) => data
                        .declarations
                        .is_some_and(|list| tree.arena.node_array(list).nodes.len() == 1),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// A template span's literal (its middle or tail).
    fn template_span_literal(&self, span: View) -> Option<View> {
        let tree = self.navigator.tree();
        let id = span.node()?;
        match &tree.node(id).data {
            tsc_syntax::NodeData::TemplateSpan(data) => data.literal.map(View::Node),
            _ => None,
        }
    }

    /// tsgo's `pushSelectionRange`: a non-empty range around the position,
    /// unless it is the last one again.
    fn push_range(&mut self, start: usize, end: usize) {
        if start == end || !(start <= self.pos && self.pos <= end) {
            return;
        }
        let range = self
            .converters
            .to_lsp_range(&script_of(self.source), start..end);
        if range == self.last_range {
            return;
        }
        self.last_range = range.clone();
        if self.ranges.len() == MAX_SELECTION_RANGE_DEPTH - 1 {
            self.ranges.pop_front();
        }
        self.ranges.push_back(range);
    }

    /// tsgo's `pushSelectionCommentRange`: the comment, then its text after
    /// the slashes.
    fn push_comment_range(&mut self, start: usize, end: usize) {
        self.push_range(start, end);
        let text = self.source.text().as_bytes();
        let mut comment_pos = start;
        while comment_pos < end && comment_pos < text.len() && text[comment_pos] == b'/' {
            comment_pos += 1;
        }
        self.push_range(comment_pos, end);
    }
}

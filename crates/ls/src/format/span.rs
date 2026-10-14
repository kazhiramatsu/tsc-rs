//! tsgo `format/span.go`: the format span worker walks the nodes of a span
//! with the formatting scanner, applies the rules to each pair of tokens and
//! indents the lines.

use tsc_syntax::SyntaxKind;

use super::api::{FormatRequestKind, TextChange};
use super::context::FormattingContext;
use super::file::{
    decode_rune, is_token_kind, is_white_space_single_line, Child, File, List, Node,
};
use super::indent::{
    argument_starts_on_same_line_as_previous_argument,
    child_is_unindented_branch_of_conditional_expression,
    child_starts_on_the_same_line_with_else_in_if_statement,
    find_first_non_whitespace_character_and_column, find_first_non_whitespace_column,
    node_will_indent_child, should_indent_child_node,
};
use super::rule::{Rule, RuleAction, RuleFlags};
use super::rulesmap::get_rules;
use super::scanner::{FormattingScanner, TextRangeWithKind, TokenInfo};
use super::util::{
    get_close_token_for_open_token, get_line_start_position_for_position, get_open_token_for_list,
    is_member_list_element,
};
use crate::lsutil::FormatCodeSettings;

/// tsgo `core.TextRange.Overlaps`.
pub(super) fn overlaps((pos, end): (usize, usize), (other_pos, other_end): (usize, usize)) -> bool {
    pos.max(other_pos) < end.min(other_end)
}

/// tsgo `core.TextRange.ContainedBy`.
pub(super) fn contained_by(
    (pos, end): (usize, usize),
    (other_pos, other_end): (usize, usize),
) -> bool {
    other_pos <= pos && end <= other_end
}

/// tsgo `findEnclosingNode`: the node that fully contains the range.
pub(super) fn find_enclosing_node(file: &File<'_>, range: (usize, usize)) -> Node {
    fn find(file: &File<'_>, range: (usize, usize), n: Node) -> Node {
        let candidate = file
            .each_child(n)
            .into_iter()
            .filter(|&child| !file.is_reparsed(child))
            .find(|&child| contained_by(range, file.with_token_start(child)));
        match candidate {
            Some(candidate) => find(file, range, candidate),
            None => n,
        }
    }
    find(file, range, file.root())
}

/// tsgo `getScanStartPosition`: the start of the original range might fall
/// inside a comment, where the scanner would not yield the right tokens, so
/// the scan starts at the end of the token before the range.
pub(super) fn get_scan_start_position(
    file: &File<'_>,
    enclosing_node: Node,
    original_range: (usize, usize),
) -> usize {
    let (start, _) = file.with_token_start(enclosing_node);
    if start == original_range.0 && file.end(enclosing_node) == original_range.1 {
        return start;
    }

    // exclude JSDoc so the scan never starts inside a JSDoc comment
    let Some(preceding_token) = file.find_preceding_token_excluding_js_doc(original_range.0) else {
        // no preceding token found - start from the beginning of enclosing node
        return file.pos(enclosing_node);
    };

    // preceding token ends after the start of original range (i.e when originalRange.pos falls in the middle of literal)
    // start from the beginning of enclosingNode to handle the entire 'originalRange'
    if file.end(preceding_token) >= original_range.0 {
        return file.pos(enclosing_node);
    }

    file.end(preceding_token)
}

/// tsgo `getOwnOrInheritedDelta`: the indentation the node, or a parent on
/// its line, gives its children. For
///
/// ```text
/// if (a ||
///     b ||$
///     c) {...}
/// ```
///
/// Enter at `$` indents `    b ||`: the node that encloses the two lines is
/// the binary expression `a ||...`, whose initial indentation is 0, but the
/// if statement on its line introduces an indentation scope.
pub(super) fn get_own_or_inherited_delta(
    file: &File<'_>,
    n: Node,
    options: &FormatCodeSettings,
) -> i64 {
    let mut previous_line = None;
    let mut child = None;
    let mut n = Some(n);
    while let Some(node) = n {
        let line = file.line_of(file.token_start(node));
        if previous_line.is_some_and(|previous| line != previous) {
            break;
        }

        if should_indent_child_node(options, file, node, child, true, false) {
            return options.editor_settings.indent_size;
        }

        previous_line = Some(line);
        child = Some(node);
        n = file.parent(node);
    }
    0
}

/// tsgo `prepareRangeContainsErrorFunction`: whether a range overlaps a
/// parse error, asked of ranges in increasing order.
pub(super) struct ErrorRanges {
    sorted: Vec<(usize, usize)>,
    index: usize,
}

impl ErrorRanges {
    pub(super) fn new(errors: &[(usize, usize)], original_range: (usize, usize)) -> Self {
        // pick only errors that fall in range
        let mut sorted: Vec<_> = errors
            .iter()
            .copied()
            .filter(|&error| overlaps(original_range, error))
            .collect();
        sorted.sort_by_key(|&(pos, _)| pos);
        Self { sorted, index: 0 }
    }

    fn contains(&mut self, range: (usize, usize)) -> bool {
        // in current implementation sequence of arguments [r1, r2...] is monotonically increasing.
        // 'index' tracks the index of the most recent error that was checked.
        loop {
            let Some(&error) = self.sorted.get(self.index) else {
                // all errors in the range were already checked -> no error in specified range
                return false;
            };

            if range.1 <= error.0 {
                // specified range ends before the error referred by 'index' - no error in range
                return false;
            }

            if overlaps(range, error) {
                // specified range overlaps with error range
                return true;
            }

            self.index += 1;
        }
    }
}

/// tsgo `LineAction`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LineAction {
    None,
    LineAdded,
    LineRemoved,
}

/// tsgo `dynamicIndenter`, kept in the worker and named by its index.
#[derive(Clone, Copy)]
struct DynamicIndenter {
    node: Node,
    node_start_line: usize,
    indentation: i64,
    delta: i64,
}

type Indenter = usize;

/// tsgo `formatSpanWorker`.
pub(super) struct FormatSpanWorker<'a> {
    original_range: (usize, usize),
    enclosing_node: Node,
    initial_indentation: i64,
    delta: i64,
    range_contains_error: ErrorRanges,
    file: &'a File<'a>,
    options: &'a FormatCodeSettings,
    new_line: &'a str,

    formatting_scanner: FormattingScanner<'a>,
    formatting_context: FormattingContext<'a>,

    edits: Vec<TextChange>,
    previous_range: TextRangeWithKind,
    previous_range_trivia_end: usize,
    previous_parent: Node,
    previous_range_start_line: usize,

    child_context_node: Node,
    last_indented_line: Option<usize>,
    indentation_on_last_indented_line: i64,

    indenters: Vec<DynamicIndenter>,
    current_rules: Vec<&'static Rule>,
}

impl<'a> FormatSpanWorker<'a> {
    /// tsgo `newFormatSpanWorker` with the scanner of `newFormattingScanner`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        file: &'a File<'a>,
        options: &'a FormatCodeSettings,
        new_line: &'a str,
        scan_start: usize,
        original_range: (usize, usize),
        enclosing_node: Node,
        initial_indentation: i64,
        delta: i64,
        request_kind: FormatRequestKind,
        range_contains_error: ErrorRanges,
    ) -> Self {
        let root = file.root();
        Self {
            original_range,
            enclosing_node,
            initial_indentation,
            delta,
            range_contains_error,
            file,
            options,
            new_line,
            formatting_scanner: FormattingScanner::new(file, scan_start, original_range.1),
            formatting_context: FormattingContext::new(file, request_kind, options),
            edits: Vec::new(),
            previous_range: TextRangeWithKind::EMPTY,
            previous_range_trivia_end: 0,
            previous_parent: root,
            previous_range_start_line: 0,
            child_context_node: root,
            last_indented_line: None,
            indentation_on_last_indented_line: -1,
            indenters: Vec::new(),
            current_rules: Vec::with_capacity(32),
        }
    }

    fn indent_size(&self) -> i64 {
        self.options.editor_settings.indent_size
    }

    pub(super) fn execute(mut self) -> Vec<TextChange> {
        let file = self.file;
        self.indentation_on_last_indented_line = -1;
        self.last_indented_line = None;

        self.formatting_scanner.advance();

        if self.formatting_scanner.is_on_token() {
            let start_line = file.line_of(file.token_start(self.enclosing_node));
            let mut undecorated_start_line = start_line;
            if file.has_decorators(self.enclosing_node) {
                undecorated_start_line = file.line_of(get_non_decorator_token_pos_of_node(
                    file,
                    self.enclosing_node,
                ));
            }

            self.process_node(
                self.enclosing_node,
                self.enclosing_node,
                start_line,
                undecorated_start_line,
                self.initial_indentation,
                self.delta,
            );
        }

        // Leading trivia items get attached to and processed with the token that proceeds them. If the
        // range ends in the middle of some leading trivia, the token that proceeds them won't be in the
        // range and thus won't get processed. So we process those remaining trivia items here.
        let remaining_trivia = self
            .formatting_scanner
            .get_current_leading_trivia()
            .to_vec();
        if !remaining_trivia.is_empty() {
            let mut indentation = self.initial_indentation;
            if node_will_indent_child(self.options, file, self.enclosing_node, None, true, false) {
                indentation += self.indent_size(); // !!! TODO: nil check???
            }

            self.indent_trivia_items(&remaining_trivia, indentation, true, |worker, item| {
                let (start_line, start_char) = worker.file.line_and_offset(item.pos);
                worker.process_range(
                    item,
                    start_line,
                    start_char,
                    worker.enclosing_node,
                    worker.enclosing_node,
                    None,
                );
                worker.insert_indentation(item.pos, indentation, false);
            });

            if self.options.editor_settings.trim_trailing_whitespace == Some(true) {
                self.trim_trailing_whitespaces_for_remaining_range(&remaining_trivia);
            }
        }

        if self.previous_range != TextRangeWithKind::EMPTY
            && self.formatting_scanner.get_token_full_start() >= self.original_range.1
        {
            // Formatting edits happen by looking at pairs of contiguous tokens (see `processPair`),
            // typically inserting or deleting whitespace between them. The recursive `processNode`
            // logic above bails out as soon as it encounters a token that is beyond the end of the
            // range we're supposed to format (or if we reach the end of the file). But this potentially
            // leaves out an edit that would occur *inside* the requested range but cannot be discovered
            // without looking at one token *beyond* the end of the range: consider the line `x = { }`
            // with a selection from the beginning of the line to the space inside the curly braces,
            // inclusive. We would expect a format-selection would delete the space (if rules apply),
            // but in order to do that, we need to process the pair ["{", "}"], but we stopped processing
            // just before getting there. This block handles this trailing edit.
            let token_info = if self.formatting_scanner.is_on_eof() {
                self.formatting_scanner.read_eof_token_range()
            } else if self.formatting_scanner.is_on_token() {
                self.formatting_scanner
                    .read_token_info(self.enclosing_node)
                    .token
            } else {
                TextRangeWithKind::EMPTY
            };

            if token_info.pos == self.previous_range_trivia_end {
                // We need to check that tokenInfo and previousRange are contiguous: the `originalRange`
                // may have ended in the middle of a token, which means we will have stopped formatting
                // on that token, leaving `previousRange` pointing to the token before it, but already
                // having moved the formatting scanner (where we just got `tokenInfo`) to the next token.
                // If this happens, our supposed pair [previousRange, tokenInfo] actually straddles the
                // token that intersects the end of the range we're supposed to format, so the pair will
                // produce bogus edits if we try to `processPair`. Recall that the point of this logic is
                // to perform a trailing edit at the end of the selection range: but there can be no valid
                // edit in the middle of a token where the range ended, so if we have a non-contiguous
                // pair here, we're already done and we can ignore it.
                let parent = file
                    .find_preceding_token(token_info.end)
                    .and_then(|token| file.parent(token))
                    .unwrap_or(self.previous_parent);
                let line = file.line_of(token_info.pos);
                self.process_pair(
                    token_info,
                    line,
                    parent,
                    self.previous_range,
                    self.previous_range_start_line,
                    self.previous_parent,
                    parent,
                    None,
                );
            }
        }

        self.edits
    }

    #[allow(clippy::too_many_arguments)]
    fn process_child_node(
        &mut self,
        node: Node,
        child: Node,
        mut inherited_indentation: i64,
        parent: Node,
        parent_dynamic_indentation: Indenter,
        parent_start_line: usize,
        undecorated_parent_start_line: usize,
        is_list_item: bool,
        is_first_list_item: bool,
    ) -> i64 {
        let file = self.file;
        if file.node_is_missing(child) || file.is_reparsed(child) {
            return inherited_indentation;
        }
        let child_start_pos = file.token_start(child);
        let child_start_line = file.line_of(child_start_pos);

        let mut undecorated_child_start_line = child_start_line;
        if file.has_decorators(child) {
            undecorated_child_start_line =
                file.line_of(get_non_decorator_token_pos_of_node(file, child));
        }

        let is_error_member_list_element =
            file.this_node_has_error(child) && is_member_list_element(file, parent, child);
        // if child is a list item - try to get its indentation, only if parent is within the original range.
        let mut child_indentation_amount = -1;

        if !is_error_member_list_element
            && is_list_item
            && contained_by((file.pos(parent), file.end(parent)), self.original_range)
        {
            child_indentation_amount = self.try_compute_indentation_for_list_item(
                child_start_pos,
                file.end(child),
                parent_start_line,
                self.original_range,
                inherited_indentation,
            );
            if child_indentation_amount != -1 {
                inherited_indentation = child_indentation_amount;
            }
        }

        // child node is outside the target range - do not dive inside
        if !overlaps(self.original_range, (file.pos(child), file.end(child))) {
            if file.end(child) < self.original_range.0 {
                self.formatting_scanner.skip_to_end_of(file.end(child));
            }
            return inherited_indentation;
        }

        if file.pos(child) == file.end(child) {
            return inherited_indentation;
        }

        while self.formatting_scanner.is_on_token()
            && self.formatting_scanner.get_token_full_start() < self.original_range.1
        {
            // proceed any parent tokens that are located prior to child.getStart()
            let token_info = self.formatting_scanner.read_token_info(node);
            if token_info.token.end > self.original_range.1 {
                return inherited_indentation;
            }
            if token_info.token.end > child_start_pos {
                if token_info.token.pos > child_start_pos {
                    self.formatting_scanner.skip_to_start_of(file.pos(child));
                }
                // stop when formatting scanner advances past the beginning of the child
                break;
            }

            self.consume_token_and_advance_scanner(
                token_info,
                node,
                parent_dynamic_indentation,
                node,
                false,
            );
        }

        if !self.formatting_scanner.is_on_token()
            || self.formatting_scanner.get_token_full_start() >= self.original_range.1
        {
            return inherited_indentation;
        }

        if is_token_kind(file.kind(child)) {
            // if child node is a token, it does not impact indentation, proceed it using parent indentation scope rules
            let token_info = self.formatting_scanner.read_token_info(child);
            // JSX text shouldn't affect indenting
            if file.kind(child) != SyntaxKind::JsxText {
                debug_assert_eq!(
                    token_info.token.end,
                    file.end(child),
                    "Token end is child end"
                );
                self.consume_token_and_advance_scanner(
                    token_info,
                    node,
                    parent_dynamic_indentation,
                    child,
                    false,
                );
                return inherited_indentation;
            }
        }

        let effective_parent_start_line = if file.kind(child) == SyntaxKind::Decorator {
            child_start_line
        } else {
            undecorated_parent_start_line
        };
        let (child_indentation, delta) = if is_error_member_list_element {
            (self.get_current_indentation_at_position(child_start_pos), 0)
        } else {
            self.compute_indentation(
                child,
                child_start_line,
                child_indentation_amount,
                node,
                parent_dynamic_indentation,
                effective_parent_start_line,
            )
        };

        self.process_node(
            child,
            self.child_context_node,
            child_start_line,
            undecorated_child_start_line,
            child_indentation,
            delta,
        );

        self.child_context_node = node;

        if is_first_list_item
            && file.kind(parent) == SyntaxKind::ArrayLiteralExpression
            && inherited_indentation == -1
        {
            inherited_indentation = child_indentation;
        }

        inherited_indentation
    }

    fn process_child_nodes(
        &mut self,
        node: Node,
        nodes: List,
        parent: Node,
        parent_start_line: usize,
        parent_dynamic_indentation: Indenter,
    ) {
        let file = self.file;
        let list_start_token = get_open_token_for_list(file, parent, nodes);

        let mut list_dynamic_indentation = parent_dynamic_indentation;
        let mut start_line = parent_start_line;

        let list_range = file.list_range(nodes.id);
        let elements = file.list_nodes(nodes);

        // node range is outside the target range - do not dive inside
        if !overlaps(self.original_range, list_range) {
            if list_range.1 < self.original_range.0
                && elements
                    .first()
                    .is_none_or(|&first| !file.is_reparsed(first))
            {
                self.formatting_scanner.skip_to_end_of(list_range.1);
            }
            return;
        }

        if list_start_token != SyntaxKind::Unknown {
            // introduce a new indentation scope for lists (including list start and end tokens)
            while self.formatting_scanner.is_on_token()
                && self.formatting_scanner.get_token_full_start() < self.original_range.1
            {
                let token_info = self.formatting_scanner.read_token_info(parent);
                if token_info.token.end > list_range.0 {
                    // stop when formatting scanner moves past the beginning of node list
                    break;
                } else if token_info.token.kind == list_start_token {
                    // consume list start token
                    start_line = file.line_of(token_info.token.pos);
                    let token_pos = token_info.token.pos;

                    self.consume_token_and_advance_scanner(
                        token_info,
                        parent,
                        parent_dynamic_indentation,
                        parent,
                        false,
                    );

                    let indentation_on_list_start_token =
                        if self.indentation_on_last_indented_line != -1 {
                            // scanner just processed list start token so consider last indentation as list indentation
                            // function foo(): { // last indentation was 0, list item will be indented based on this value
                            //   foo: number;
                            // }: {};
                            self.indentation_on_last_indented_line
                        } else {
                            self.get_current_indentation_at_position(token_pos)
                        };

                    list_dynamic_indentation = self.get_dynamic_indentation(
                        parent,
                        parent_start_line,
                        indentation_on_list_start_token,
                        self.indent_size(),
                    );
                } else {
                    // consume any tokens that precede the list as child elements of 'node' using its indentation scope
                    self.consume_token_and_advance_scanner(
                        token_info,
                        parent,
                        parent_dynamic_indentation,
                        parent,
                        false,
                    );
                }
            }
        }

        let mut inherited_indentation = -1;
        for (i, &child) in elements.iter().enumerate() {
            inherited_indentation = self.process_child_node(
                node,
                child,
                inherited_indentation,
                node,
                list_dynamic_indentation,
                start_line,
                start_line,
                true,
                i == 0,
            );
        }

        let list_end_token = get_close_token_for_open_token(list_start_token);
        if list_end_token != SyntaxKind::Unknown
            && self.formatting_scanner.is_on_token()
            && self.formatting_scanner.get_token_full_start() < self.original_range.1
        {
            let mut token_info = self.formatting_scanner.read_token_info(parent);
            if token_info.token.kind == SyntaxKind::CommaToken {
                // consume the comma
                self.consume_token_and_advance_scanner(
                    token_info,
                    parent,
                    list_dynamic_indentation,
                    parent,
                    false,
                );
                if self.formatting_scanner.is_on_token() {
                    token_info = self.formatting_scanner.read_token_info(parent);
                } else {
                    return;
                }
            }

            // consume the list end token only if it is still belong to the parent
            // there might be the case when current token matches end token but does not considered as one
            // function (x: function) <--
            // without this check close paren will be interpreted as list end token for function expression which is wrong
            if token_info.token.kind == list_end_token
                && token_info
                    .token
                    .contained_by(file.pos(parent), file.end(parent))
            {
                // consume list end token
                self.consume_token_and_advance_scanner(
                    token_info,
                    parent,
                    list_dynamic_indentation,
                    parent,
                    true,
                );
            }
        }
    }

    /// tsgo `executeProcessNodeVisitor`: each child, and each NodeList as a
    /// whole.
    fn execute_process_node_visitor(
        &mut self,
        node: Node,
        indenter: Indenter,
        node_start_line: usize,
        undecorated_node_start_line: usize,
    ) {
        for child in self.file.children(node) {
            match child {
                Child::Node(child) => {
                    self.process_child_node(
                        node,
                        child,
                        -1,
                        node,
                        indenter,
                        node_start_line,
                        undecorated_node_start_line,
                        false,
                        false,
                    );
                }
                Child::List(list) => {
                    self.process_child_nodes(node, list, node, node_start_line, indenter);
                }
            }
        }
    }

    fn get_current_indentation_at_position(&self, pos: usize) -> i64 {
        let start_line_position = get_line_start_position_for_position(self.file, pos);
        find_first_non_whitespace_column(self.file, start_line_position, pos, self.options)
    }

    fn compute_indentation(
        &mut self,
        node: Node,
        start_line: usize,
        inherited_indentation: i64,
        parent: Node,
        parent_dynamic_indentation: Indenter,
        effective_parent_start_line: usize,
    ) -> (i64, i64) {
        let file = self.file;
        let mut delta = 0;
        if should_indent_child_node(self.options, file, node, None, false, false) {
            delta = self.indent_size();
        }

        if effective_parent_start_line == start_line {
            // if node is located on the same line with the parent
            // - inherit indentation from the parent
            // - push children if either parent of node itself has non-zero delta
            let mut indentation = self.indentation_on_last_indented_line;
            if Some(start_line) != self.last_indented_line {
                indentation = self.indenters[parent_dynamic_indentation].indentation;
            }
            delta = self
                .indent_size()
                .min(self.get_delta(parent_dynamic_indentation, node) + delta);
            return (indentation, delta);
        } else if inherited_indentation == -1 {
            if file.kind(node) == SyntaxKind::OpenParenToken
                && Some(start_line) == self.last_indented_line
            {
                // the is used for chaining methods formatting
                // - we need to get the indentation on last line and the delta of parent
                return (
                    self.indentation_on_last_indented_line,
                    self.get_delta(parent_dynamic_indentation, node),
                );
            } else if child_starts_on_the_same_line_with_else_in_if_statement(
                file, parent, node, start_line,
            ) || child_is_unindented_branch_of_conditional_expression(
                file, parent, node, start_line,
            ) || argument_starts_on_same_line_as_previous_argument(
                file, parent, node, start_line,
            ) {
                return (
                    self.indenters[parent_dynamic_indentation].indentation,
                    delta,
                );
            } else {
                let i = self.indenters[parent_dynamic_indentation].indentation;
                if i == -1 {
                    return (i, delta);
                }
                return (i + self.get_delta(parent_dynamic_indentation, node), delta);
            }
        }

        (inherited_indentation, delta)
    }

    /// tsgo `tryComputeIndentationForListItem`: a list element out of the
    /// range keeps its actual indentation, which it passes on to the next
    /// ones; one in the range has the indentation inherited from those
    /// before it.
    fn try_compute_indentation_for_list_item(
        &self,
        start_pos: usize,
        end_pos: usize,
        parent_start_line: usize,
        range: (usize, usize),
        inherited_indentation: i64,
    ) -> i64 {
        let r2 = (start_pos, end_pos);
        if overlaps(range, r2) || contained_by(r2, range) {
            /* Not to miss zero-range nodes e.g. JsxText */
            if inherited_indentation != -1 {
                return inherited_indentation;
            }
        } else {
            let start_line = self.file.line_of(start_pos);
            let column = self.get_current_indentation_at_position(start_pos);
            if start_line != parent_start_line || start_pos as i64 == column {
                // Use the base indent size if it is greater than
                // the indentation of the inherited predecessor.
                let base_indent_size = self.options.editor_settings.base_indent_size;
                if base_indent_size > column {
                    return base_indent_size;
                }
                return column;
            }
        }
        -1
    }

    fn process_node(
        &mut self,
        node: Node,
        context_node: Node,
        node_start_line: usize,
        undecorated_node_start_line: usize,
        indentation: i64,
        delta: i64,
    ) {
        let file = self.file;
        if !overlaps(self.original_range, file.with_token_start(node)) {
            return;
        }

        let node_dynamic_indentation =
            self.get_dynamic_indentation(node, node_start_line, indentation, delta);

        // a useful observations when tracking context node
        //        /
        //      [a]
        //   /   |   \
        //  [b] [c] [d]
        // node 'a' is a context node for nodes 'b', 'c', 'd'
        // except for the leftmost leaf token in [b] - in this case context node ('e') is located somewhere above 'a'
        // this rule can be applied recursively to child nodes of 'a'.
        //
        // context node is set to parent node value after processing every child node
        // context node is set to parent of the token after processing every token

        self.child_context_node = context_node;

        // if there are any tokens that logically belong to node and interleave child nodes
        // such tokens will be consumed in processChildNode for the child that follows them
        self.execute_process_node_visitor(
            node,
            node_dynamic_indentation,
            node_start_line,
            undecorated_node_start_line,
        );

        // proceed any tokens in the node that are located after child nodes
        while self.formatting_scanner.is_on_token()
            && self.formatting_scanner.get_token_full_start() < self.original_range.1
        {
            let token_info = self.formatting_scanner.read_token_info(node);
            if token_info.token.end > file.end(node).min(self.original_range.1) {
                break;
            }
            self.consume_token_and_advance_scanner(
                token_info,
                node,
                node_dynamic_indentation,
                node,
                false,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_pair(
        &mut self,
        current_item: TextRangeWithKind,
        current_start_line: usize,
        current_parent: Node,
        previous_item: TextRangeWithKind,
        previous_start_line: usize,
        previous_parent: Node,
        context_node: Node,
        dynamic_indentation: Option<Indenter>,
    ) -> LineAction {
        self.formatting_context.update_context(
            previous_item,
            previous_parent,
            current_item,
            current_parent,
            context_node,
        );

        let mut current_rules = std::mem::take(&mut self.current_rules);
        current_rules.clear();
        get_rules(&self.formatting_context, &mut current_rules);

        let mut trim_trailing_whitespaces =
            self.options.editor_settings.trim_trailing_whitespace != Some(false);
        let mut line_action = LineAction::None;

        if !current_rules.is_empty() {
            // Apply rules in reverse order so that higher priority rules (which are first in the array)
            // win in a conflict with lower priority rules.
            for rule in current_rules.iter().rev() {
                line_action = self.apply_rule_edits(
                    rule,
                    previous_item,
                    previous_start_line,
                    current_item,
                    current_start_line,
                );
                if let Some(dynamic_indentation) = dynamic_indentation {
                    match line_action {
                        LineAction::LineRemoved => {
                            // Handle the case where the next line is moved to be the end of this line.
                            // In this case we don't indent the next line in the next pass.
                            if self.file.token_start(current_parent) == current_item.pos {
                                self.recompute_indentation(
                                    dynamic_indentation,
                                    false,
                                    context_node,
                                );
                            }
                        }
                        LineAction::LineAdded => {
                            // Handle the case where token2 is moved to the new line.
                            // In this case we indent token2 in the next pass but we set
                            // sameLineIndent flag to notify the indenter that the indentation is within the line.
                            if self.file.token_start(current_parent) == current_item.pos {
                                self.recompute_indentation(dynamic_indentation, true, context_node);
                            }
                        }
                        LineAction::None => {}
                    }
                }

                // We need to trim trailing whitespace between the tokens if they were on different lines, and no rule was applied to put them on the same line
                trim_trailing_whitespaces = trim_trailing_whitespaces
                    && !rule.action.intersects(RuleAction::DELETE_SPACE)
                    && rule.flags != RuleFlags::CanDeleteNewLines;
            }
        } else {
            trim_trailing_whitespaces =
                trim_trailing_whitespaces && current_item.kind != SyntaxKind::EndOfFileToken;
        }
        self.current_rules = current_rules;

        if current_start_line != previous_start_line && trim_trailing_whitespaces {
            // We need to trim trailing whitespace between the tokens if they were on different lines, and no rule was applied to put them on the same line
            self.trim_trailing_whitespaces_for_lines(
                previous_start_line,
                current_start_line,
                previous_item,
            );
        }

        line_action
    }

    fn apply_rule_edits(
        &mut self,
        rule: &Rule,
        previous_range: TextRangeWithKind,
        previous_start_line: usize,
        current_range: TextRangeWithKind,
        current_start_line: usize,
    ) -> LineAction {
        let on_later_line = current_start_line != previous_start_line;
        let action = rule.action;
        if action == RuleAction::STOP_PROCESSING_SPACE_ACTIONS {
            // no action required
            return LineAction::None;
        } else if action == RuleAction::DELETE_SPACE {
            if previous_range.end != current_range.pos {
                // delete characters starting from t1.end up to t2.pos exclusive
                self.record_delete(
                    previous_range.end,
                    span_length(previous_range.end, current_range.pos),
                );
                if on_later_line {
                    return LineAction::LineRemoved;
                }
                return LineAction::None;
            }
        } else if action == RuleAction::DELETE_TOKEN {
            self.record_delete(
                previous_range.pos,
                span_length(previous_range.pos, previous_range.end),
            );
        } else if action == RuleAction::INSERT_NEW_LINE {
            // exit early if we on different lines and rule cannot change number of newlines
            // if line1 and line2 are on subsequent lines then no edits are required - ok to exit
            // if line1 and line2 are separated with more than one newline - ok to exit since we cannot delete extra new lines
            if rule.flags != RuleFlags::CanDeleteNewLines
                && previous_start_line != current_start_line
            {
                return LineAction::None;
            }

            // edit should not be applied if we have one line feed between elements
            let line_delta = current_start_line as i64 - previous_start_line as i64;
            if line_delta != 1 {
                self.record_replace(
                    previous_range.end,
                    span_length(previous_range.end, current_range.pos),
                    self.new_line,
                );
                if on_later_line {
                    return LineAction::None;
                }
                return LineAction::LineAdded;
            }
        } else if action == RuleAction::INSERT_SPACE {
            // exit early if we on different lines and rule cannot change number of newlines
            if rule.flags != RuleFlags::CanDeleteNewLines
                && previous_start_line != current_start_line
            {
                return LineAction::None;
            }

            let pos_delta = span_length(previous_range.end, current_range.pos);
            if pos_delta != 1 || self.file.text().as_bytes().get(previous_range.end) != Some(&b' ')
            {
                self.record_replace(previous_range.end, pos_delta, " ");
                if on_later_line {
                    return LineAction::LineRemoved;
                }
                return LineAction::None;
            }
        } else if action == RuleAction::INSERT_TRAILING_SEMICOLON {
            self.record_insert(previous_range.end, ";");
        }
        LineAction::None
    }

    fn process_range(
        &mut self,
        r: TextRangeWithKind,
        range_start_line: usize,
        _range_start_character: usize,
        parent: Node,
        context_node: Node,
        dynamic_indentation: Option<Indenter>,
    ) -> LineAction {
        let range_has_error = self.range_contains_error.contains((r.pos, r.end));
        let mut line_action = LineAction::None;
        if !range_has_error {
            if self.previous_range == TextRangeWithKind::EMPTY {
                // trim whitespaces starting from the beginning of the span up to the current line
                let original_start_line = self.file.line_of(self.original_range.0);
                self.trim_trailing_whitespaces_for_lines(
                    original_start_line,
                    range_start_line,
                    TextRangeWithKind::EMPTY,
                );
            } else {
                line_action = self.process_pair(
                    r,
                    range_start_line,
                    parent,
                    self.previous_range,
                    self.previous_range_start_line,
                    self.previous_parent,
                    context_node,
                    dynamic_indentation,
                );
            }
        }

        self.previous_range = r;
        self.previous_range_trivia_end = r.end;
        self.previous_parent = parent;
        self.previous_range_start_line = range_start_line;

        line_action
    }

    fn process_trivia(
        &mut self,
        trivia: &[TextRangeWithKind],
        parent: Node,
        context_node: Node,
        dynamic_indentation: Option<Indenter>,
    ) {
        for &trivia_item in trivia {
            if is_comment(trivia_item.kind)
                && contained_by((trivia_item.pos, trivia_item.end), self.original_range)
            {
                let (trivia_item_start_line, trivia_item_start_character) =
                    self.file.line_and_offset(trivia_item.pos);
                self.process_range(
                    trivia_item,
                    trivia_item_start_line,
                    trivia_item_start_character,
                    parent,
                    context_node,
                    dynamic_indentation,
                );
            }
        }
    }

    /// tsgo `trimTrailingWhitespacesForRemainingRange`: trimming is done for
    /// the lines after the previous range, the comments excluded as they
    /// had been processed.
    fn trim_trailing_whitespaces_for_remaining_range(&mut self, trivias: &[TextRangeWithKind]) {
        let mut start_pos = self.original_range.0;
        if self.previous_range != TextRangeWithKind::EMPTY {
            start_pos = self.previous_range.end;
        }

        for trivia in trivias {
            if is_comment(trivia.kind) {
                if start_pos < trivia.pos {
                    self.trim_trailing_witespaces_for_positions(
                        start_pos,
                        trivia.pos - 1,
                        self.previous_range,
                    );
                }

                start_pos = trivia.end + 1;
            }
        }

        if start_pos < self.original_range.1 {
            self.trim_trailing_witespaces_for_positions(
                start_pos,
                self.original_range.1,
                self.previous_range,
            );
        }
    }

    fn trim_trailing_witespaces_for_positions(
        &mut self,
        start_pos: usize,
        end_pos: usize,
        previous_range: TextRangeWithKind,
    ) {
        let start_line = self.file.line_of(start_pos);
        let end_line = self.file.line_of(end_pos);

        self.trim_trailing_whitespaces_for_lines(start_line, end_line + 1, previous_range);
    }

    fn trim_trailing_whitespaces_for_lines(
        &mut self,
        line1: usize,
        line2: usize,
        r: TextRangeWithKind,
    ) {
        for line in line1..line2 {
            let line_start_position = self.file.line_starts()[line];
            let line_end_position = self.file.end_line_position(line);

            // do not trim whitespaces in comments or template expression
            if r != TextRangeWithKind::EMPTY
                && (is_comment(r.kind)
                    || is_string_or_regular_expression_or_template_literal(r.kind))
                && r.pos as isize <= line_end_position
                && r.end as isize > line_end_position
            {
                continue;
            }

            if let Some(whitespace_start) =
                self.get_trailing_whitespace_start_position(line_start_position, line_end_position)
            {
                self.record_delete(
                    whitespace_start,
                    line_end_position + 1 - whitespace_start as isize,
                );
            }
        }
    }

    /// tsgo `getTrailingWhitespaceStartPosition`: from the line's first
    /// character to its last.
    fn get_trailing_whitespace_start_position(&self, start: usize, end: isize) -> Option<usize> {
        let mut pos = end;
        let text = self.file.text();
        while pos >= start as isize {
            let (ch, size) = decode_rune(text, pos as usize);
            if size == 0 {
                pos -= 1; // multibyte character, rewind more
                continue;
            }
            if !is_white_space_single_line(ch) {
                break;
            }
            pos -= 1;
        }
        (pos != end).then(|| (pos + 1) as usize)
    }

    fn insert_indentation(&mut self, pos: usize, indentation: i64, line_added: bool) {
        let indentation_string = get_indentation_string(indentation, self.options);
        if line_added {
            // new line is added before the token by the formatting rules
            // insert indentation string at the very beginning of the token
            self.record_replace(pos, 0, &indentation_string);
        } else {
            let (token_start_line, token_start_character) = self.file.line_and_offset(pos);
            let start_line_position = self.file.line_starts()[token_start_line];
            if indentation != self.character_to_column(start_line_position, token_start_character)
                || self.indentation_is_different(&indentation_string, start_line_position)
            {
                self.record_replace(
                    start_line_position,
                    token_start_character as isize,
                    &indentation_string,
                );
            }
        }
    }

    fn character_to_column(&self, start_line_position: usize, character_in_line: usize) -> i64 {
        let mut column = 0;
        let text = self.file.text().as_bytes();
        let tab_size = self.options.editor_settings.tab_size;
        for i in 0..character_in_line {
            if text[start_line_position + i] == b'\t' {
                if tab_size > 0 {
                    column += tab_size - (column % tab_size);
                }
            } else {
                column += 1;
            }
        }
        column
    }

    fn indentation_is_different(
        &self,
        indentation_string: &str,
        start_line_position: usize,
    ) -> bool {
        let text = self.file.text().as_bytes();
        let end = start_line_position + indentation_string.len();
        if end > text.len() {
            return true;
        }
        indentation_string.as_bytes() != &text[start_line_position..end]
    }

    fn indent_trivia_items(
        &mut self,
        trivia: &[TextRangeWithKind],
        comment_indentation: i64,
        mut indent_next_token_or_trivia: bool,
        mut indent_single_line: impl FnMut(&mut Self, TextRangeWithKind),
    ) -> bool {
        for &trivia_item in trivia {
            let trivia_in_range =
                contained_by((trivia_item.pos, trivia_item.end), self.original_range);
            match trivia_item.kind {
                SyntaxKind::MultiLineCommentTrivia => {
                    if trivia_in_range {
                        self.indent_multiline_comment(
                            (trivia_item.pos, trivia_item.end),
                            comment_indentation,
                            !indent_next_token_or_trivia,
                            true,
                        );
                    }
                    indent_next_token_or_trivia = false;
                }
                SyntaxKind::SingleLineCommentTrivia => {
                    if indent_next_token_or_trivia && trivia_in_range {
                        indent_single_line(self, trivia_item);
                    }
                    indent_next_token_or_trivia = false;
                }
                SyntaxKind::NewLineTrivia => indent_next_token_or_trivia = true,
                _ => {}
            }
        }
        indent_next_token_or_trivia
    }

    fn indent_multiline_comment(
        &mut self,
        (comment_pos, comment_end): (usize, usize),
        indentation: i64,
        first_line_is_indented: bool,
        indent_final_line: bool,
    ) {
        let file = self.file;
        // split comment in lines
        let mut start_line = file.line_of(comment_pos);
        let end_line = file.line_of(comment_end);

        if start_line == end_line {
            if !first_line_is_indented {
                // treat as single line comment
                self.insert_indentation(comment_pos, indentation, false);
            }
            return;
        }

        let mut parts: Vec<(usize, isize)> = Vec::new();
        let mut start_pos = comment_pos;
        for line in start_line..end_line {
            let end_of_line = file.end_line_position(line);
            parts.push((start_pos, end_of_line));
            start_pos = file.line_starts()[line + 1];
        }

        if indent_final_line {
            parts.push((start_pos, comment_end as isize));
        }

        if parts.is_empty() {
            return;
        }

        let start_line_pos = file.line_starts()[start_line];

        let (non_whitespace_in_first_part_character, non_whitespace_in_first_part_column) =
            find_first_non_whitespace_character_and_column(
                file,
                start_line_pos,
                parts[0].0,
                self.options,
            );

        let mut start_index = 0;

        if first_line_is_indented {
            start_index = 1;
            start_line += 1;
        }

        // shift all parts on the delta size
        let delta = indentation - non_whitespace_in_first_part_column;
        for (i, &(part_pos, part_end)) in parts.iter().enumerate().skip(start_index) {
            let start_line_pos = file.line_starts()[start_line];
            let (non_whitespace_character, non_whitespace_column) = if i != 0 {
                // A part that ends before it starts has no white space.
                let part_end = usize::try_from(part_end).map_or(part_pos, |end| end.max(part_pos));
                find_first_non_whitespace_character_and_column(
                    file,
                    part_pos,
                    part_end,
                    self.options,
                )
            } else {
                (
                    non_whitespace_in_first_part_character,
                    non_whitespace_in_first_part_column,
                )
            };
            let new_indentation = non_whitespace_column + delta;
            if new_indentation > 0 {
                let indentation_string = get_indentation_string(new_indentation, self.options);
                self.record_replace(
                    start_line_pos,
                    non_whitespace_character as isize,
                    &indentation_string,
                );
            } else {
                self.record_delete(start_line_pos, non_whitespace_character as isize);
            }

            start_line += 1;
        }
    }

    fn record_delete(&mut self, start: usize, length: isize) {
        if length != 0 {
            self.edits
                .push(create_text_change_from_start_length(start, length, ""));
        }
    }

    fn record_replace(&mut self, start: usize, length: isize, new_text: &str) {
        if length != 0 || !new_text.is_empty() {
            self.edits.push(create_text_change_from_start_length(
                start, length, new_text,
            ));
        }
    }

    fn record_insert(&mut self, start: usize, text: &str) {
        if !text.is_empty() {
            self.edits
                .push(create_text_change_from_start_length(start, 0, text));
        }
    }

    fn consume_token_and_advance_scanner(
        &mut self,
        current_token_info: TokenInfo,
        parent: Node,
        dynamic_indenation: Indenter,
        container: Node,
        is_list_end_token: bool,
    ) {
        let file = self.file;
        let last_trivia_was_new_line = self.formatting_scanner.last_trailing_trivia_was_new_line();
        let mut indent_token = false;

        if !current_token_info.leading_trivia.is_empty() {
            self.process_trivia(
                &current_token_info.leading_trivia,
                parent,
                self.child_context_node,
                Some(dynamic_indenation),
            );
        }

        let mut line_action = LineAction::None;
        let token = current_token_info.token;
        let is_token_in_range = contained_by((token.pos, token.end), self.original_range);

        let (token_start_line, token_start_char) = file.line_and_offset(token.pos);

        if is_token_in_range {
            let range_has_error = self.range_contains_error.contains((token.pos, token.end));
            // save previousRange since processRange will overwrite this value with current one
            let save_previous_range = self.previous_range;
            line_action = self.process_range(
                token,
                token_start_line,
                token_start_char,
                parent,
                self.child_context_node,
                Some(dynamic_indenation),
            );
            // do not indent comments\token if token range overlaps with some error
            if !range_has_error {
                if line_action == LineAction::None {
                    // indent token only if end line of previous range does not match start line of the token
                    if save_previous_range != TextRangeWithKind::EMPTY {
                        let prev_end_line = file.line_of(save_previous_range.end);
                        indent_token =
                            last_trivia_was_new_line && token_start_line != prev_end_line;
                    } else {
                        // When there's no previous range (first token), TS sets prevEndLine to undefined.
                        // tokenStart.line !== undefined is always true in JS, so indentToken = lastTriviaWasNewLine.
                        indent_token = last_trivia_was_new_line;
                    }
                } else {
                    indent_token = line_action == LineAction::LineAdded;
                }
            }
        }

        if let Some(last) = current_token_info.trailing_trivia.last() {
            self.previous_range_trivia_end = last.end;
            // If any trailing comment trivia extends past the original range, it won't be
            // processed by processTrivia (which skips comments not contained by originalRange).
            // Cap previousRangeTriviaEnd before such comments so the trailing edit contiguity
            // check in execute() won't pair across unprocessed comment content.
            for trivia in &current_token_info.trailing_trivia {
                if is_comment(trivia.kind)
                    && !contained_by((trivia.pos, trivia.end), self.original_range)
                {
                    self.previous_range_trivia_end = trivia.pos;
                    break;
                }
            }
            self.process_trivia(
                &current_token_info.trailing_trivia,
                parent,
                self.child_context_node,
                Some(dynamic_indenation),
            );
        }

        if indent_token {
            let mut token_indentation = -1;
            if is_token_in_range && !self.range_contains_error.contains((token.pos, token.end)) {
                token_indentation = self.get_indentation_for_token(
                    dynamic_indenation,
                    token_start_line,
                    token.kind,
                    container,
                    is_list_end_token,
                );
            }
            let mut indent_next_token_or_trivia = true;
            if !current_token_info.leading_trivia.is_empty() {
                let comment_indentation = self.get_indentation_for_comment(
                    dynamic_indenation,
                    token.kind,
                    token_indentation,
                    container,
                );
                indent_next_token_or_trivia = self.indent_trivia_items(
                    &current_token_info.leading_trivia,
                    comment_indentation,
                    indent_next_token_or_trivia,
                    |worker, item| worker.insert_indentation(item.pos, comment_indentation, false),
                );
            }

            // indent token only if is it is in target range and does not overlap with any error ranges
            if token_indentation != -1 && indent_next_token_or_trivia {
                self.insert_indentation(
                    token.pos,
                    token_indentation,
                    line_action == LineAction::LineAdded,
                );

                self.last_indented_line = Some(token_start_line);
                self.indentation_on_last_indented_line = token_indentation;
            }
        }

        self.formatting_scanner.advance();

        self.child_context_node = parent;
    }

    /// tsgo `getDynamicIndentation`.
    fn get_dynamic_indentation(
        &mut self,
        node: Node,
        node_start_line: usize,
        indentation: i64,
        delta: i64,
    ) -> Indenter {
        self.indenters.push(DynamicIndenter {
            node,
            node_start_line,
            indentation,
            delta,
        });
        self.indenters.len() - 1
    }

    /// tsgo `dynamicIndenter.getIndentationForComment`.
    fn get_indentation_for_comment(
        &self,
        indenter: Indenter,
        kind: SyntaxKind,
        token_indentation: i64,
        container: Node,
    ) -> i64 {
        let indentation = self.indenters[indenter].indentation;
        match kind {
            // preceding comment to the token that closes the indentation scope inherits the indentation from the scope
            // ..  {
            //     // comment
            // }
            SyntaxKind::CloseBraceToken
            | SyntaxKind::CloseBracketToken
            | SyntaxKind::CloseParenToken => indentation + self.get_delta(indenter, container),
            _ if token_indentation != -1 => token_indentation,
            _ => indentation,
        }
    }

    /// tsgo `dynamicIndenter.getIndentationForToken`: a `>` that ends a list
    /// suppresses its delta, so that a `<` as a binary operator can still
    /// be indented.
    fn get_indentation_for_token(
        &self,
        indenter: Indenter,
        line: usize,
        kind: SyntaxKind,
        container: Node,
        suppress_delta: bool,
    ) -> i64 {
        let indentation = self.indenters[indenter].indentation;
        if !suppress_delta && self.should_add_delta(indenter, line, kind, container) {
            return indentation + self.get_delta(indenter, container);
        }
        indentation
    }

    /// tsgo `dynamicIndenter.getDelta`.
    fn get_delta(&self, indenter: Indenter, child: Node) -> i64 {
        let indenter = self.indenters[indenter];
        // Delta value should be zero when the node explicitly prevents indentation of the child node
        if node_will_indent_child(
            self.options,
            self.file,
            indenter.node,
            Some(child),
            true,
            true,
        ) {
            return indenter.delta;
        }
        0
    }

    /// tsgo `dynamicIndenter.recomputeIndentation`.
    fn recompute_indentation(&mut self, indenter: Indenter, line_added: bool, parent: Node) {
        let node = self.indenters[indenter].node;
        if should_indent_child_node(self.options, self.file, parent, Some(node), true, false) {
            let indent_size = self.indent_size();
            let delta =
                if should_indent_child_node(self.options, self.file, node, None, false, false) {
                    indent_size
                } else {
                    0
                };
            let indenter = &mut self.indenters[indenter];
            if line_added {
                indenter.indentation += indent_size; // !!! no nil check???
            } else {
                indenter.indentation -= indent_size; // !!! no nil check???
            }
            indenter.delta = delta;
        }
    }

    /// tsgo `dynamicIndenter.shouldAddDelta`.
    fn should_add_delta(
        &self,
        indenter: Indenter,
        line: usize,
        kind: SyntaxKind,
        container: Node,
    ) -> bool {
        let file = self.file;
        match kind {
            // open and close brace, 'else' and 'while' (in do statement) tokens has indentation of the parent
            SyntaxKind::OpenBraceToken
            | SyntaxKind::CloseBraceToken
            | SyntaxKind::CloseParenToken
            | SyntaxKind::ElseKeyword
            | SyntaxKind::WhileKeyword
            | SyntaxKind::AtToken => return false,
            SyntaxKind::SlashToken | SyntaxKind::GreaterThanToken => {
                if matches!(
                    file.kind(container),
                    SyntaxKind::JsxOpeningElement
                        | SyntaxKind::JsxClosingElement
                        | SyntaxKind::JsxSelfClosingElement
                ) {
                    return false;
                }
            }
            SyntaxKind::OpenBracketToken | SyntaxKind::CloseBracketToken => {
                if file.kind(container) != SyntaxKind::MappedType {
                    return false;
                }
            }
            _ => {}
        }
        let indenter = self.indenters[indenter];
        // if token line equals to the line of containing node (this is a first token in the node) - use node indentation
        indenter.node_start_line != line
            // if this token is the first token following the list of decorators, we do not need to indent
            && !(file.has_decorators(indenter.node)
                && kind == get_first_non_decorator_token_of_node(file, indenter.node))
    }
}

/// The length of a span, negative when it ends before it starts.
fn span_length(start: usize, end: usize) -> isize {
    end as isize - start as isize
}

fn create_text_change_from_start_length(start: usize, length: isize, new_text: &str) -> TextChange {
    TextChange {
        pos: start,
        end: (start as isize + length) as usize,
        new_text: new_text.to_owned(),
    }
}

/// tsgo `getNonDecoratorTokenPosOfNode`.
fn get_non_decorator_token_pos_of_node(file: &File<'_>, node: Node) -> usize {
    let last_decorator = if file.has_decorators(node) {
        file.modifier_nodes(node)
            .into_iter()
            .rev()
            .find(|&modifier| file.kind(modifier) == SyntaxKind::Decorator)
    } else {
        None
    };
    match last_decorator {
        None => file.with_token_start(node).0,
        Some(decorator) => tsc_syntax::skip_trivia(file.text(), file.end(decorator)),
    }
}

/// tsgo `getFirstNonDecoratorTokenOfNode`.
fn get_first_non_decorator_token_of_node(file: &File<'_>, node: Node) -> SyntaxKind {
    let kind = file.kind(node);
    if super::file::can_have_modifiers(kind) {
        let modifiers = file.modifier_nodes(node);
        let first_decorator = modifiers
            .iter()
            .position(|&modifier| file.kind(modifier) == SyntaxKind::Decorator)
            .unwrap_or(0);
        if let Some(&modifier) = modifiers[first_decorator..]
            .iter()
            .find(|&&modifier| super::file::is_modifier_kind(file.kind(modifier)))
        {
            return file.kind(modifier);
        }
    }

    match kind {
        SyntaxKind::ClassDeclaration => SyntaxKind::ClassKeyword,
        SyntaxKind::InterfaceDeclaration => SyntaxKind::InterfaceKeyword,
        SyntaxKind::FunctionDeclaration => SyntaxKind::FunctionKeyword,
        SyntaxKind::EnumDeclaration => SyntaxKind::EnumDeclaration,
        SyntaxKind::GetAccessor => SyntaxKind::GetKeyword,
        SyntaxKind::SetAccessor => SyntaxKind::SetKeyword,
        SyntaxKind::MethodDeclaration if file.method_has_asterisk(node) => {
            SyntaxKind::AsteriskToken
        }
        SyntaxKind::MethodDeclaration | SyntaxKind::PropertyDeclaration | SyntaxKind::Parameter => {
            file.name_of_declaration(node)
                .map_or(SyntaxKind::Unknown, |name| file.kind(name))
        }
        _ => SyntaxKind::Unknown,
    }
}

/// tsgo `getIndentationString`.
pub(crate) fn get_indentation_string(indentation: i64, options: &FormatCodeSettings) -> String {
    let repeat = |text: &str, count: i64| {
        text.repeat(usize::try_from(count).expect("strings: negative Repeat count"))
    };
    let tab_size = options.editor_settings.tab_size;
    if options.editor_settings.convert_tabs_to_spaces != Some(true) {
        if tab_size == 0 {
            return String::new();
        }
        let tabs = (indentation as f64 / tab_size as f64).floor() as i64;
        let spaces = indentation - (tabs * tab_size);
        let mut res = repeat("\t", tabs);
        if spaces > 0 {
            res.push_str(&repeat(" ", spaces));
        }
        res
    } else {
        repeat(" ", indentation)
    }
}

fn is_string_or_regular_expression_or_template_literal(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::StringLiteral
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead
            | SyntaxKind::TemplateMiddle
            | SyntaxKind::TemplateTail
    )
}

fn is_comment(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::SingleLineCommentTrivia || kind == SyntaxKind::MultiLineCommentTrivia
}

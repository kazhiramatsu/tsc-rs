//! tsgo `format/context.go`: the pair of tokens a rule looks at, their
//! parents and the facts about their lines the rules ask for.

use std::cell::Cell;

use tsc_syntax::SyntaxKind;

use super::api::FormatRequestKind;
use super::file::{File, Node};
use super::scanner::TextRangeWithKind;
use crate::lsutil::FormatCodeSettings;

/// tsgo `FormattingContext`.
pub(crate) struct FormattingContext<'a> {
    pub(super) current_token_span: TextRangeWithKind,
    pub(super) next_token_span: TextRangeWithKind,
    pub(super) context_node: Node,
    pub(super) current_token_parent: Node,
    pub(super) next_token_parent: Node,

    context_node_all_on_same_line: Cell<Option<bool>>,
    next_node_all_on_same_line: Cell<Option<bool>>,
    tokens_are_on_same_line: Cell<Option<bool>>,
    context_node_block_is_on_one_line: Cell<Option<bool>>,
    next_node_block_is_on_one_line: Cell<Option<bool>>,

    pub(super) file: &'a File<'a>,
    pub(super) formatting_request_kind: FormatRequestKind,
    pub(super) options: &'a FormatCodeSettings,
}

impl<'a> FormattingContext<'a> {
    /// tsgo `NewFormattingContext`.
    pub(super) fn new(
        file: &'a File<'a>,
        kind: FormatRequestKind,
        options: &'a FormatCodeSettings,
    ) -> Self {
        let root = file.root();
        Self {
            current_token_span: TextRangeWithKind::EMPTY,
            next_token_span: TextRangeWithKind::EMPTY,
            context_node: root,
            current_token_parent: root,
            next_token_parent: root,
            context_node_all_on_same_line: Cell::new(None),
            next_node_all_on_same_line: Cell::new(None),
            tokens_are_on_same_line: Cell::new(None),
            context_node_block_is_on_one_line: Cell::new(None),
            next_node_block_is_on_one_line: Cell::new(None),
            file,
            formatting_request_kind: kind,
            options,
        }
    }

    /// tsgo `UpdateContext`.
    pub(super) fn update_context(
        &mut self,
        current: TextRangeWithKind,
        current_parent: Node,
        next: TextRangeWithKind,
        next_parent: Node,
        common_parent: Node,
    ) {
        self.current_token_span = current;
        self.current_token_parent = current_parent;
        self.next_token_span = next;
        self.next_token_parent = next_parent;
        self.context_node = common_parent;

        // drop cached results
        self.context_node_all_on_same_line.set(None);
        self.next_node_all_on_same_line.set(None);
        self.tokens_are_on_same_line.set(None);
        self.context_node_block_is_on_one_line.set(None);
        self.next_node_block_is_on_one_line.set(None);
    }

    fn range_is_on_one_line(&self, pos: usize, end: usize) -> bool {
        self.file.line_of(pos) == self.file.line_of(end)
    }

    fn node_is_on_one_line(&self, node: Node) -> bool {
        let (pos, end) = self.file.with_token_start(node);
        self.range_is_on_one_line(pos, end)
    }

    fn block_is_on_one_line(&self, node: Node) -> bool {
        let open_brace = self
            .file
            .find_child_of_kind(node, SyntaxKind::OpenBraceToken);
        let close_brace = self
            .file
            .find_child_of_kind(node, SyntaxKind::CloseBraceToken);
        if let (Some(open_brace), Some(close_brace)) = (open_brace, close_brace) {
            let close_brace_start = self.file.token_start(close_brace);
            return self.range_is_on_one_line(self.file.end(open_brace), close_brace_start);
        }
        false
    }

    pub(super) fn context_node_all_on_same_line(&self) -> bool {
        cached(&self.context_node_all_on_same_line, || {
            self.node_is_on_one_line(self.context_node)
        })
    }

    pub(super) fn next_node_all_on_same_line(&self) -> bool {
        cached(&self.next_node_all_on_same_line, || {
            self.node_is_on_one_line(self.next_token_parent)
        })
    }

    pub(super) fn tokens_are_on_same_line(&self) -> bool {
        cached(&self.tokens_are_on_same_line, || {
            self.range_is_on_one_line(self.current_token_span.pos, self.next_token_span.pos)
        })
    }

    pub(super) fn context_node_block_is_on_one_line(&self) -> bool {
        cached(&self.context_node_block_is_on_one_line, || {
            self.block_is_on_one_line(self.context_node)
        })
    }

    pub(super) fn next_node_block_is_on_one_line(&self) -> bool {
        cached(&self.next_node_block_is_on_one_line, || {
            self.block_is_on_one_line(self.next_token_parent)
        })
    }
}

fn cached(cell: &Cell<Option<bool>>, compute: impl FnOnce() -> bool) -> bool {
    if let Some(value) = cell.get() {
        return value;
    }
    let value = compute();
    cell.set(Some(value));
    value
}

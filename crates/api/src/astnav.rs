//! tsgo `astnav/tokens.go` (19dadef8): the token or node at a position of a
//! source file (`GetTouchingPropertyName` and the searches it uses), over
//! tsgo's tree as the encoder presents tsc-rs's ([`View`]) and the tokens
//! the scanner reads between its nodes. Positions are byte offsets, as
//! tsgo's are.

use tsc_syntax::{
    skip_trivia_ex, NodeData, SkipTriviaOptions, SourceFile, SyntaxKind, TokenScanner,
};
use tsc_types::NodeFlags;

use crate::encoder::{Tree, View};

/// What a search finds: a node of tsgo's tree, or a token the tree does not
/// keep, which tsgo's source file creates for the scanner's token (tsgo
/// `GetOrCreateToken`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Found {
    Node(View),
    Token(Token),
}

/// A token the scanner read: its kind, full start and end, and the node it
/// was read in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: SyntaxKind,
    pub(crate) pos: u32,
    pub(crate) end: u32,
    pub(crate) parent: View,
}

/// A child a visitor sees: a node, or a NodeList it takes as a whole.
#[derive(Clone, Copy)]
enum Child {
    Node(View),
    List(View),
}

/// tsgo's `includePrecedingTokenAtEndPosition`: whether a token that ends
/// at the position is the answer.
type IncludePrecedingToken<'f> = &'f dyn Fn(&Navigator<'_>, Found) -> bool;

/// Token navigation over one source file.
pub(crate) struct Navigator<'a> {
    file: &'a SourceFile,
    tree: Tree<'a>,
}

impl<'a> Navigator<'a> {
    pub(crate) fn new(file: &'a SourceFile) -> Self {
        Self {
            file,
            tree: Tree::new(file, None),
        }
    }

    /// tsgo `GetTouchingPropertyName`: the token at `position`, or the
    /// property name, keyword or private identifier that ends there.
    pub(crate) fn touching_property_name(&self, position: u32) -> Found {
        self.token_at_position(
            position,
            false,
            Some(&|navigator: &Navigator<'_>, found: Found| {
                let kind = navigator.kind(found);
                is_property_name_literal(kind)
                    || is_keyword_kind(kind)
                    || kind == SyntaxKind::PrivateIdentifier
            }),
        )
    }

    /// tsgo `getTokenAtPosition` (tokens.go:38-275).
    fn token_at_position(
        &self,
        position: u32,
        allow_position_in_leading_trivia: bool,
        include_preceding_token_at_end_position: Option<IncludePrecedingToken<'_>>,
    ) -> Found {
        // `next` is the node whose children are visited next; `prev_subtree`
        // ends at `position` (only with the callback), and the next pass
        // tests its rightmost token.
        let mut prev_subtree: Option<View> = None;
        let mut current = View::Node(self.file.root);
        // The lower bound of what can be returned; the scanner's start.
        let mut left = 0;
        // The first node visited after the one that advanced `left`: the
        // scanner reads up to its start.
        let mut node_after_left: Option<View> = None;

        let included_preceding_token = |subtree: View| -> Option<Found> {
            let child = self.find_preceding_token_ex(position, Some(subtree), false)?;
            let include = include_preceding_token_at_end_position?;
            (self.end(child) == position && include(self, child)).then_some(child)
        };

        loop {
            let mut next: Option<View> = None;
            let test_node = |node: View, prev_subtree: &mut Option<View>| -> i32 {
                let kind = self.view_kind(node);
                if kind != SyntaxKind::EndOfFileToken
                    && self.view_end(node) == position
                    && include_preceding_token_at_end_position.is_some()
                    && !self.is_reparsed(node)
                {
                    if prev_subtree
                        .is_some_and(|subtree| included_preceding_token(subtree).is_some())
                    {
                        return 0;
                    }
                    *prev_subtree = Some(node);
                }
                // A node contains `position` if it is before its end, except
                // at the end of the file (nowhere else to look): the
                // end-of-file token, and JSDoc reaching it.
                let end = self.view_end(node);
                if end < position
                    || end == position
                        && kind != SyntaxKind::EndOfFileToken
                        && (!is_js_doc_kind(kind) || end != self.end_of_file_end())
                {
                    return -1;
                }
                if self.position_of(Found::Node(node), allow_position_in_leading_trivia) > position
                {
                    return 1;
                }
                0
            };

            for child in self.children(current) {
                match child {
                    Child::Node(node) => {
                        if self.is_reparsed(node) {
                            continue;
                        }
                        if node_after_left.is_none() {
                            node_after_left = Some(node);
                        }
                        if next.is_none() {
                            match test_node(node, &mut prev_subtree) {
                                -1 => {
                                    if !is_js_doc_kind(self.view_kind(node)) {
                                        // `left` cannot move into or past JSDoc: a
                                        // token after it is scanned with all its
                                        // leading trivia.
                                        left = self.view_end(node);
                                    }
                                    node_after_left = None;
                                }
                                0 => next = Some(node),
                                _ => {}
                            }
                        }
                    }
                    Child::List(list) => {
                        let nodes = self.list_nodes(list);
                        if nodes.is_empty() {
                            continue;
                        }
                        if node_after_left.is_none() {
                            node_after_left = nodes.iter().copied().find(|&n| !self.is_reparsed(n));
                        }
                        if next.is_some() {
                            continue;
                        }
                        let (list_pos, list_end) = self.tree.range(list);
                        if list_end == position && include_preceding_token_at_end_position.is_some()
                        {
                            left = list_end;
                            node_after_left = None;
                            prev_subtree =
                                nodes.iter().rev().copied().find(|&n| !self.is_reparsed(n));
                        } else if list_end <= position {
                            left = list_end;
                            node_after_left = None;
                        } else if list_pos <= position {
                            let (mut index, mut matched) =
                                binary_search_unique(&nodes, |middle, node| {
                                    if self.is_reparsed(node) {
                                        return 0;
                                    }
                                    let cmp = test_node(node, &mut prev_subtree);
                                    if cmp < 0 {
                                        left = self.view_end(node);
                                        node_after_left = nodes[middle + 1..]
                                            .iter()
                                            .copied()
                                            .find(|&n| !self.is_reparsed(n));
                                    }
                                    cmp
                                });
                            if matched && self.is_reparsed(nodes[index]) {
                                // Filter and search again.
                                let visible = nodes
                                    .iter()
                                    .copied()
                                    .filter(|&n| !self.is_reparsed(n))
                                    .collect::<Vec<_>>();
                                (index, matched) =
                                    binary_search_unique(&visible, |middle, node| {
                                        let cmp = test_node(node, &mut prev_subtree);
                                        if cmp < 0 {
                                            left = self.view_end(node);
                                            node_after_left = visible.get(middle + 1).copied();
                                        }
                                        cmp
                                    });
                                if matched {
                                    next = Some(visible[index]);
                                }
                            } else if matched {
                                next = Some(nodes[index]);
                            }
                        }
                    }
                }
            }

            // A `prev_subtree` set by this pass ends at `position`: its
            // rightmost token is returned when the callback takes it.
            if let Some(subtree) = prev_subtree.take() {
                if let Some(child) = included_preceding_token(subtree) {
                    // The callback only takes nodes of the tree.
                    return child;
                }
            }

            // No node contains `position`: either `current` is the token, or
            // the scanner reads one the tree does not keep.
            let Some(found) = next else {
                let kind = self.view_kind(current);
                if is_token_kind(kind) || should_skip_child(kind) {
                    return Found::Node(current);
                }
                let mut scanner = TokenScanner::new(self.file, left as usize);
                // Scan only up to the start of the node after the one ending at
                // `left`, so a position between two nodes or tokens finds no
                // token before reaching the next node.
                let end =
                    node_after_left.map_or(self.view_end(current), |node| self.view_pos(node));
                while left < end {
                    let token = self.scan_navigation_token(&mut scanner, current);
                    let token_full_start = scanner.token_full_start() as u32;
                    let token_start = if allow_position_in_leading_trivia {
                        token_full_start
                    } else {
                        scanner.token_start() as u32
                    };
                    let token_end = scanner.token_end() as u32;
                    if token_end > end {
                        break;
                    }
                    if token_start <= position && position < token_end {
                        if token == SyntaxKind::Identifier || !is_token_kind(token) {
                            if is_js_doc_kind(kind) {
                                return Found::Node(current);
                            }
                            panic!("did not expect {kind:?} to have {token:?} in its trivia");
                        }
                        return Found::Token(Token {
                            kind: token,
                            pos: token_full_start,
                            end: token_end,
                            parent: current,
                        });
                    }
                    if let Some(include) = include_preceding_token_at_end_position {
                        if token_end == position {
                            let previous = Found::Token(Token {
                                kind: token,
                                pos: token_full_start,
                                end: token_end,
                                parent: current,
                            });
                            if include(self, previous) {
                                return previous;
                            }
                        }
                    }
                    left = token_end;
                    scanner.scan();
                }
                return Found::Node(current);
            };
            current = found;
            left = self.view_pos(current);
            node_after_left = None;
        }
    }

    /// tsgo `FindPrecedingTokenEx` (tokens.go:347-449): the leftmost token
    /// that ends after `position`, or, when that one is invalid or
    /// `position` is in its trivia, the rightmost valid token ending at or
    /// before it.
    fn find_preceding_token_ex(
        &self,
        position: u32,
        start_node: Option<View>,
        exclude_js_doc: bool,
    ) -> Option<Found> {
        let result = self.find_preceding(
            start_node.unwrap_or(View::Node(self.file.root)),
            position,
            exclude_js_doc,
        );
        if let Some(found) = result {
            assert!(
                !self.is_whitespace_only_jsx_text(found),
                "Expected result to be a non-whitespace token."
            );
        }
        result
    }

    fn find_preceding(&self, n: View, position: u32, exclude_js_doc: bool) -> Option<Found> {
        if self.is_non_whitespace_token(Found::Node(n))
            && self.view_kind(n) != SyntaxKind::EndOfFileToken
        {
            return Some(Found::Node(n));
        }
        // `found_child` is the leftmost child that contains `position`;
        // `prev_child` the last child visited.
        let mut found_child: Option<View> = None;
        let mut prev_child: Option<View> = None;
        for child in self.children(n) {
            if found_child.is_some() {
                break;
            }
            match child {
                Child::Node(node) => {
                    if self.is_reparsed(node) {
                        continue;
                    }
                    if position < self.view_end(node)
                        && prev_child.is_none_or(|prev| self.view_end(prev) <= position)
                    {
                        found_child = Some(node);
                    } else {
                        prev_child = Some(node);
                    }
                }
                Child::List(list) => {
                    let nodes = self.list_nodes(list);
                    if nodes.is_empty() {
                        continue;
                    }
                    let (index, matched) = binary_search_unique(&nodes, |middle, _| {
                        // A synthetic JSDoc node ends before the host starts.
                        if self.is_reparsed(nodes[middle]) {
                            return -1;
                        }
                        if position < self.view_end(nodes[middle]) {
                            if middle == 0 || position >= self.view_end(nodes[middle - 1]) {
                                return 0;
                            }
                            return 1;
                        }
                        -1
                    });
                    if matched {
                        found_child = Some(nodes[index]);
                    }
                    let valid_lookup = if matched {
                        index as isize - 1
                    } else {
                        nodes.len() as isize - 1
                    };
                    for i in (0..=valid_lookup).rev() {
                        let node = nodes[i as usize];
                        if self.is_reparsed(node) {
                            continue;
                        }
                        if prev_child.is_none() {
                            prev_child = Some(node);
                        }
                    }
                }
            }
        }

        if let Some(found_child) = found_child {
            // A token span is [start of node, end). With `position <
            // child.end`: in the child's leading trivia or preceding tokens,
            // or a child without tokens, the answer is the last token before
            // it; inside its tokens, recur into it.
            let start = self.start_of_node(Found::Node(found_child), !exclude_js_doc);
            let look_in_previous_child =
                start >= position || !self.is_valid_preceding_node(found_child);
            if !look_in_previous_child {
                return self.find_preceding(found_child, position, exclude_js_doc);
            }
            if position >= self.view_pos(found_child) {
                // The JSDoc comment before the found child.
                let js_doc = self
                    .js_docs(n)
                    .into_iter()
                    .rev()
                    .find(|&js_doc| self.view_pos(js_doc) >= self.view_pos(found_child));
                if let Some(js_doc) = js_doc {
                    if !exclude_js_doc && position < self.view_end(js_doc) {
                        return self.find_preceding(js_doc, position, exclude_js_doc);
                    }
                    return self.find_rightmost_valid_token(
                        self.view_end(js_doc),
                        n,
                        Some(position),
                        exclude_js_doc,
                    );
                }
                return self.find_rightmost_valid_token(
                    self.view_pos(found_child),
                    n,
                    None,
                    exclude_js_doc,
                );
            }
            // The answer is in the tokens between two visited children.
            return self.find_rightmost_valid_token(
                self.view_pos(found_child),
                n,
                Some(position),
                exclude_js_doc,
            );
        }

        // At the end of the file, or in the unvisited trailing tokens of
        // the node.
        let end = self.view_end(n);
        self.find_rightmost_valid_token(
            end,
            n,
            (position < end).then_some(position),
            exclude_js_doc,
        )
    }

    /// tsgo `isValidPrecedingNode`.
    fn is_valid_preceding_node(&self, node: View) -> bool {
        if self.view_kind(node) == SyntaxKind::EndOfFileToken {
            return !self.js_docs(node).is_empty();
        }
        let start = self.start_of_node(Found::Node(node), false);
        let width = self.view_end(node) as i64 - start as i64;
        !(self.is_whitespace_only_jsx_text(Found::Node(node)) || width == 0)
    }

    /// tsgo `findRightmostValidToken` (tokens.go:479-597): the rightmost
    /// valid token in `[containing_node.pos, end_pos)`, which precedes or
    /// touches `position` when given.
    fn find_rightmost_valid_token(
        &self,
        end_pos: u32,
        containing_node: View,
        position: Option<u32>,
        exclude_js_doc: bool,
    ) -> Option<Found> {
        let position = position.unwrap_or_else(|| self.view_end(containing_node));
        let mut n = containing_node;
        let mut end_pos = end_pos;
        loop {
            if self.is_non_whitespace_token(Found::Node(n)) {
                return Some(Found::Node(n));
            }
            let mut rightmost_valid_node: Option<View> = None;
            // The nodes after the last valid node.
            let mut rightmost_visited_nodes: Vec<View> = Vec::new();
            let mut has_children = false;
            let should_visit_node = |node: View| -> bool {
                !(self.is_reparsed(node)
                    || self.view_end(node) > end_pos
                    || self.start_of_node(Found::Node(node), !exclude_js_doc) >= position)
            };
            for child in self.children(n) {
                match child {
                    Child::Node(node) => {
                        if self.is_reparsed(node) {
                            continue;
                        }
                        has_children = true;
                        if !should_visit_node(node) {
                            continue;
                        }
                        rightmost_visited_nodes.push(node);
                        if self.is_valid_preceding_node(node) {
                            rightmost_valid_node = Some(node);
                            rightmost_visited_nodes.clear();
                        }
                    }
                    Child::List(list) => {
                        let nodes = self.list_nodes(list);
                        if nodes.is_empty() {
                            continue;
                        }
                        has_children = true;
                        let (index, _) = binary_search_unique(&nodes, |_, node| {
                            if self.view_end(node) > end_pos {
                                1
                            } else {
                                -1
                            }
                        });
                        let mut valid_index: isize = -1;
                        for i in (0..index).rev() {
                            if !should_visit_node(nodes[i]) {
                                continue;
                            }
                            if self.is_valid_preceding_node(nodes[i]) {
                                valid_index = i as isize;
                                rightmost_valid_node = Some(nodes[i]);
                                break;
                            }
                        }
                        for &node in &nodes[(valid_index + 1) as usize..index] {
                            if !should_visit_node(node) {
                                continue;
                            }
                            rightmost_visited_nodes.push(node);
                        }
                    }
                }
            }

            // 1. The answer is a token of `rightmost_valid_node`.
            // 2. The answer is one of the unvisited tokens after it.
            // 3. The current node has no children or tokens: it is the
            //    answer.
            let kind = self.view_kind(n);
            if !should_skip_child(kind) {
                // JSDoc nodes have no trivia tokens as children.
                let mut start_pos =
                    rightmost_valid_node.map_or(self.view_pos(n), |node| self.view_end(node));
                let mut scanner = TokenScanner::new(self.file, start_pos as usize);
                let mut tokens: Vec<Found> = Vec::new();
                let scan_until = |scanner: &mut TokenScanner<'_>,
                                  start_pos: &mut u32,
                                  limit: u32,
                                  tokens: &mut Vec<Found>| {
                    while *start_pos < limit {
                        let token = self.scan_navigation_token(scanner, n);
                        if scanner.token_start() as u32 >= limit {
                            break;
                        }
                        let token_full_start = scanner.token_full_start() as u32;
                        let token_end = scanner.token_end() as u32;
                        *start_pos = token_end;
                        tokens.push(Found::Token(Token {
                            kind: token,
                            pos: token_full_start,
                            end: token_end,
                            parent: n,
                        }));
                        scanner.scan();
                    }
                };
                for &visited in &rightmost_visited_nodes {
                    // The trailing tokens before this node.
                    let limit = self.view_pos(visited).min(position);
                    scan_until(&mut scanner, &mut start_pos, limit, &mut tokens);
                    start_pos = self.view_end(visited);
                    scanner.reset_pos(start_pos as usize);
                    scanner.scan();
                }
                // The trailing tokens after the last visited node.
                scan_until(
                    &mut scanner,
                    &mut start_pos,
                    end_pos.min(position),
                    &mut tokens,
                );
                if let Some(&token) = tokens
                    .iter()
                    .rev()
                    .find(|&&token| !self.is_whitespace_only_jsx_text(token))
                {
                    return Some(token);
                }
            }

            if !has_children {
                return (n != containing_node).then_some(Found::Node(n));
            }
            let rightmost = rightmost_valid_node?;
            end_pos = self.view_end(rightmost);
            n = rightmost;
        }
    }

    /// tsgo `scanNavigationToken`: `<<` in a JSX child is two `<` tokens.
    fn scan_navigation_token(
        &self,
        scanner: &mut TokenScanner<'_>,
        containing: View,
    ) -> SyntaxKind {
        let token = scanner.token();
        if token == SyntaxKind::LessThanLessThanToken && is_jsx_child(self.view_kind(containing)) {
            return scanner.re_scan_jsx_token();
        }
        token
    }

    /// tsgo `VisitEachChildAndJSDoc` through astnav's visitor
    /// (`getNodeVisitor`): the JSDoc comments of `view`, then its children;
    /// a JSDoc comment or tag's comment of a single part is not visited
    /// (tsc keeps it as a string).
    fn children(&self, view: View) -> Vec<Child> {
        let mut views = Vec::new();
        self.tree.children(view, &mut views);
        let js_docs = views
            .iter()
            .rev()
            .take_while(|child| matches!(child, View::JSDoc(..)))
            .count();
        views.rotate_right(js_docs);
        views
            .into_iter()
            .filter(|&child| !self.is_single_comment_list(child))
            .map(|child| match child {
                View::List(..) | View::ImplicitModifiers(_) | View::Comment(_) => {
                    Child::List(child)
                }
                _ => Child::Node(child),
            })
            .collect()
    }

    /// tsgo `IsJSDocSingleCommentNodeList`.
    fn is_single_comment_list(&self, view: View) -> bool {
        matches!(view, View::Comment(_)) && self.list_nodes(view).len() == 1
    }

    fn list_nodes(&self, list: View) -> Vec<View> {
        let mut nodes = Vec::new();
        self.tree.children(list, &mut nodes);
        nodes
    }

    /// tsgo `Node.JSDoc`.
    fn js_docs(&self, view: View) -> Vec<View> {
        let mut js_docs = Vec::new();
        if let View::Node(id) = view {
            self.tree.js_doc_views(id, &mut js_docs);
        }
        js_docs
    }

    /// tsgo `getPosition`: the node's position, or its first token's.
    fn position_of(&self, found: Found, allow_position_in_leading_trivia: bool) -> u32 {
        if allow_position_in_leading_trivia {
            return self.pos(found);
        }
        self.start_of_node(found, true)
    }

    /// tsgo `GetTokenPosOfNode` (scanner.go:2525-2539).
    fn start_of_node(&self, found: Found, include_js_doc: bool) -> u32 {
        let kind = self.kind(found);
        let pos = self.pos(found);
        // A node without width keeps its position: skipping trivia would
        // reach the next token.
        if pos == self.end(found) && kind != SyntaxKind::EndOfFileToken {
            return pos;
        }
        let text = self.file.text();
        if is_js_doc_kind(kind) || kind == SyntaxKind::JsxText {
            // JsxText has no comments, though the scanner would see some.
            let options = SkipTriviaOptions {
                stop_at_comments: true,
                ..SkipTriviaOptions::default()
            };
            return skip_trivia_ex(text, pos as usize, options) as u32;
        }
        if include_js_doc {
            if let Found::Node(view) = found {
                if let Some(&js_doc) = self.js_docs(view).first() {
                    return self.start_of_node(Found::Node(js_doc), false);
                }
            }
        }
        let options = SkipTriviaOptions {
            in_js_doc: self.has_js_doc_flag(found),
            ..SkipTriviaOptions::default()
        };
        skip_trivia_ex(text, pos as usize, options) as u32
    }

    pub(crate) fn kind(&self, found: Found) -> SyntaxKind {
        match found {
            Found::Node(view) => self.view_kind(view),
            Found::Token(token) => token.kind,
        }
    }

    fn pos(&self, found: Found) -> u32 {
        match found {
            Found::Node(view) => self.view_pos(view),
            Found::Token(token) => token.pos,
        }
    }

    fn end(&self, found: Found) -> u32 {
        match found {
            Found::Node(view) => self.view_end(view),
            Found::Token(token) => token.end,
        }
    }

    fn view_kind(&self, view: View) -> SyntaxKind {
        self.tree.kind(view).unwrap_or(SyntaxKind::SyntaxList)
    }

    fn view_pos(&self, view: View) -> u32 {
        self.tree.range(view).0
    }

    fn view_end(&self, view: View) -> u32 {
        self.tree.range(view).1
    }

    /// tsgo `NodeFlagsReparsed`: of the views, only the implicit `export`
    /// of a nested namespace.
    fn is_reparsed(&self, view: View) -> bool {
        matches!(view, View::ImplicitExport(_))
    }

    /// tsgo `NodeFlagsJSDoc`.
    fn has_js_doc_flag(&self, found: Found) -> bool {
        match found {
            Found::Node(
                View::Node(id)
                | View::HeritageTypeReference(id)
                | View::QualifiedName(id)
                | View::JSDoc(id, _),
            ) => NodeFlags::from_bits(self.tree.node(id).flags).contains(NodeFlags::JS_DOC),
            _ => false,
        }
    }

    fn end_of_file_end(&self) -> u32 {
        self.file.text().len() as u32
    }

    /// tsgo `IsNonWhitespaceToken`.
    fn is_non_whitespace_token(&self, found: Found) -> bool {
        is_token_kind(self.kind(found)) && !self.is_whitespace_only_jsx_text(found)
    }

    /// tsgo `IsWhitespaceOnlyJsxText`.
    fn is_whitespace_only_jsx_text(&self, found: Found) -> bool {
        match found {
            Found::Node(View::Node(id)) => matches!(
                &self.tree.node(id).data,
                NodeData::JsxText(text) if text.contains_only_trivia_white_spaces
            ),
            Found::Token(token) => token.kind == SyntaxKind::JsxTextAllWhiteSpaces,
            Found::Node(_) => false,
        }
    }
}

/// tsgo `core.BinarySearchUniqueFunc`: the index `cmp` answers 0 at, or
/// where the search stopped.
fn binary_search_unique(nodes: &[View], mut cmp: impl FnMut(usize, View) -> i32) -> (usize, bool) {
    if nodes.is_empty() {
        return (0, false);
    }
    let (mut low, mut high) = (0isize, nodes.len() as isize - 1);
    while low <= high {
        let middle = low + ((high - low) >> 1);
        let value = cmp(middle as usize, nodes[middle as usize]);
        if value < 0 {
            low = middle + 1;
        } else if value > 0 {
            high = middle - 1;
        } else {
            return (middle as usize, true);
        }
    }
    (low as usize, false)
}

/// tsgo `IsTokenKind`.
fn is_token_kind(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstToken.value()..=SyntaxKind::LastToken.value()).contains(&kind.value())
}

/// tsgo `IsKeywordKind`.
fn is_keyword_kind(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstKeyword.value()..=SyntaxKind::LastKeyword.value()).contains(&kind.value())
}

/// tsgo `IsJSDocKind`.
fn is_js_doc_kind(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstJSDocNode.value()..=SyntaxKind::LastJSDocNode.value()).contains(&kind.value())
}

/// tsgo `IsPropertyNameLiteral`.
fn is_property_name_literal(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Identifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
    )
}

/// tsgo `IsJsxChild`.
fn is_jsx_child(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::JsxElement
            | SyntaxKind::JsxExpression
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxText
            | SyntaxKind::JsxFragment
    )
}

/// tsgo `shouldSkipChild`: JSDoc nodes keep no trivia tokens as children.
fn should_skip_child(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::JSDoc
            | SyntaxKind::JSDocText
            | SyntaxKind::JSDocTypeLiteral
            | SyntaxKind::JSDocSignature
            | SyntaxKind::JSDocLink
            | SyntaxKind::JSDocLinkCode
            | SyntaxKind::JSDocLinkPlain
    ) || (SyntaxKind::FirstJSDocTagNode.value()..=SyntaxKind::LastJSDocTagNode.value())
        .contains(&kind.value())
}

#[cfg(test)]
#[path = "../tests/unit/astnav.rs"]
mod tests;

//! tsgo `ls/change/tracker.go`'s insertions: text or nodes inserted at a
//! position, after or before a node, after a list item, among import
//! specifiers, at the top of a file.

use tsc_binder::node_util;
use tsc_emitter::{TransformError, TransformFlags, TransformNode};
use tsc_syntax::{
    comment_ranges, skip_trivia_ex, NodeArrayId, NodeData, NodeId, SkipTriviaOptions, SourceFile,
    SyntaxKind,
};

use super::delete::{is_separator, positions_are_on_same_line};
use super::{
    line_of, line_start_of, LeadingTriviaOption, NodeOptions, Tracker, TrailingTriviaOption,
};
use crate::astnav::{Found, Navigator};
use crate::format::{self, File};
use crate::tree::View;

impl<'a> Tracker<'a> {
    /// tsgo `insertTextAt` (and `InsertText` at the position's offset).
    pub(crate) fn insert_text_at(&mut self, file: &'a SourceFile, pos: usize, text: String) {
        self.replace_range_with_text(file, (pos, pos), text);
    }

    /// tsgo `InsertNodeAt`.
    pub(crate) fn insert_node_at(
        &mut self,
        file: &'a SourceFile,
        pos: usize,
        node: TransformNode,
        options: NodeOptions,
    ) {
        self.replace_range(file, (pos, pos), node, options);
    }

    /// tsgo `InsertNodesAt`.
    pub(crate) fn insert_nodes_at(
        &mut self,
        file: &'a SourceFile,
        pos: usize,
        nodes: Vec<TransformNode>,
        options: NodeOptions,
    ) {
        self.replace_range_with_nodes(file, (pos, pos), nodes, options);
    }

    /// tsgo `InsertNodeAfter`.
    pub(crate) fn insert_node_after(
        &mut self,
        file: &'a SourceFile,
        after: NodeId,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let end = self.end_pos_for_insert_node_after(file, after, node)?;
        let options = self.get_insert_node_after_options(file, after);
        self.insert_node_at(file, end, node, options);
        Ok(())
    }

    /// tsgo `InsertNodesAfter`.
    pub(crate) fn insert_nodes_after(
        &mut self,
        file: &'a SourceFile,
        after: NodeId,
        nodes: Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        let end = self.end_pos_for_insert_node_after(file, after, nodes[0])?;
        let options = self.get_insert_node_after_options(file, after);
        self.insert_nodes_at(file, end, nodes, options);
        Ok(())
    }

    /// tsgo `InsertNodeBefore`.
    pub(crate) fn insert_node_before(
        &mut self,
        file: &'a SourceFile,
        before: NodeId,
        node: TransformNode,
        blank_line_between: bool,
        leading: LeadingTriviaOption,
    ) {
        let pos = self.get_adjusted_start_position(file, before, leading, false);
        let options =
            self.get_options_for_insert_node_before(file, before, node, blank_line_between);
        self.insert_node_at(file, pos, node, options);
    }

    /// tsgo `InsertMemberAtStart` (`insertNodeAtStartWorker`): a member at the
    /// start of a class, interface or object literal, indented as its
    /// members are, else one level past the node's line.
    pub(crate) fn insert_member_at_start(
        &mut self,
        file: &'a SourceFile,
        node: NodeId,
        new_element: TransformNode,
    ) {
        let indentation = self
            .try_compute_indentation_from_existing_members(file, node)
            .unwrap_or_else(|| self.try_compute_indentation_for_new_member(file, node));
        let Some(members) = members_or_properties(file, node) else {
            return;
        };
        let pos = file.arena.node_array(members).pos as usize;
        let options = self.get_insert_node_at_start_insert_options(file, node, indentation);
        self.insert_node_at(file, pos, new_element, options);
    }

    /// tsgo `tryComputeIndentationForNewMember`: one indentation level past
    /// the column of the node's start.
    fn try_compute_indentation_for_new_member(&self, file: &SourceFile, node: NodeId) -> i64 {
        let node_start = start_of(file, node);
        let line_start = line_start_of(file, node_start);
        let tab_size = positive_or(self.format_settings.editor_settings.tab_size, 4);
        let indent_size = positive_or(self.format_settings.editor_settings.indent_size, 4);
        find_indentation_column(file.text(), line_start, node_start, tab_size).max(0) + indent_size
    }

    /// tsgo `tryComputeIndentationFromExistingMembers`: the column every
    /// member starts at when each is on its own line, if any.
    fn try_compute_indentation_from_existing_members(
        &self,
        file: &SourceFile,
        node: NodeId,
    ) -> Option<i64> {
        let members = members_or_properties(file, node)?;
        let tab_size = positive_or(self.format_settings.editor_settings.tab_size, 4);
        let mut indentation: Option<i64> = None;
        let mut last = node;
        for &member in file.arena.node_array(members).nodes {
            if tsc_emitter::range_start_positions_are_on_same_line(
                file,
                file.arena.node(last).pos,
                file.arena.node(member).pos,
            ) {
                return None;
            }
            let member_start = start_of(file, member);
            let line_start = line_start_of(file, member_start);
            let column = find_indentation_column(file.text(), line_start, member_start, tab_size);
            if column < 0 {
                return None;
            }
            if let Some(indentation) = indentation {
                if indentation != column {
                    return None;
                }
                last = member;
                continue;
            }
            indentation = Some(column);
            last = member;
        }
        Some(indentation.unwrap_or(-1)).filter(|&indentation| indentation >= 0)
    }

    /// tsgo `getInsertNodeAtStartInsertOptions`.
    fn get_insert_node_at_start_insert_options(
        &mut self,
        file: &'a SourceFile,
        node: NodeId,
        indentation: i64,
    ) -> NodeOptions {
        let file_index = self.file_index(file);
        let has_previous_insertion = self
            .nodes_with_insertions_at_start
            .contains(&(file_index, node));
        if !has_previous_insertion {
            self.nodes_with_insertions_at_start.push((file_index, node));
        }
        let has_members = members_or_properties(file, node)
            .is_some_and(|members| !file.arena.node_array(members).nodes.is_empty());
        let kind = file.arena.node(node).kind;
        let is_object_literal = kind == SyntaxKind::ObjectLiteralExpression;
        let is_json = file.file_name.ends_with(".json");
        let insert_trailing_comma = is_object_literal && (has_members || !is_json);
        let insert_leading_comma =
            is_object_literal && is_json && !has_members && has_previous_insertion;
        let suffix = if insert_trailing_comma {
            ",".to_owned()
        } else if kind == SyntaxKind::InterfaceDeclaration && !has_members {
            ";".to_owned()
        } else {
            String::new()
        };
        let new_line = self.new_line.text();
        let prefix = if insert_leading_comma {
            format!(",{new_line}")
        } else {
            new_line.to_owned()
        };
        NodeOptions {
            indentation: Some(indentation),
            prefix,
            suffix,
            ..NodeOptions::default()
        }
    }

    /// tsgo `finishNodesWithInsertionsAtStart`: the braces of a node a
    /// member was inserted at the start of on lines of their own.
    pub(super) fn finish_nodes_with_insertions_at_start(&mut self) {
        let nodes = std::mem::take(&mut self.nodes_with_insertions_at_start);
        for (file_index, node) in nodes {
            let file = self.files[file_index].0;
            let navigator = Navigator::new(file);
            let Some(open_brace) =
                navigator.child_of_kind(View::Node(node), SyntaxKind::OpenBraceToken)
            else {
                continue;
            };
            let Some(close_brace) =
                navigator.child_of_kind(View::Node(node), SyntaxKind::CloseBraceToken)
            else {
                continue;
            };
            let open_brace_end = navigator.end(open_brace) as usize;
            let close_brace_end = navigator.end(close_brace) as usize;
            let is_empty = members_or_properties(file, node)
                .is_none_or(|members| file.arena.node_array(members).nodes.is_empty());
            let is_single_line = positions_are_on_same_line(file, open_brace_end, close_brace_end);
            if is_empty && is_single_line && open_brace_end != close_brace_end - 1 {
                self.replace_range_with_text(
                    file,
                    (open_brace_end, close_brace_end - 1),
                    String::new(),
                );
            }
            if is_single_line {
                let new_line = self.new_line.text().to_owned();
                self.insert_text_at(file, close_brace_end - 1, new_line);
            }
        }
    }

    /// tsgo `TryInsertTypeAnnotation`: `: type` after a function's `)` (an
    /// arrow function without one: after its parameter), or after a
    /// variable's `!`, a property's or parameter's postfix token, else its
    /// name; false when there is no such place.
    pub(crate) fn try_insert_type_annotation(
        &mut self,
        file: &'a SourceFile,
        node: NodeId,
        type_node: TransformNode,
    ) -> bool {
        let record = file.arena.node(node);
        let end_node = if node_util::is_function_like_kind(record.kind) {
            let navigator = Navigator::new(file);
            match navigator.child_of_kind(View::Node(node), SyntaxKind::CloseParenToken) {
                Some(close_paren) => Some(navigator.end(close_paren) as usize),
                // If no `)`, is an arrow function `x => x`, so use the end of
                // the first parameter
                None if record.kind == SyntaxKind::ArrowFunction => {
                    crate::utilities::parameters_in(file, node)
                        .first()
                        .map(|&parameter| file.arena.node(parameter).end as usize)
                }
                None => None,
            }
        } else {
            let postfix = match &record.data {
                NodeData::VariableDeclaration(data) => data.exclamation_token,
                NodeData::PropertySignature(data) => data.question_token,
                NodeData::PropertyDeclaration(data) => {
                    data.question_token.or(data.exclamation_token)
                }
                NodeData::Parameter(data) => data.question_token,
                _ => None,
            };
            postfix
                .or_else(|| node_util::get_name_of_declaration(file, node))
                .map(|end_node| file.arena.node(end_node).end as usize)
        };
        let Some(end) = end_node else {
            return false;
        };
        self.insert_node_at(
            file,
            end,
            type_node,
            NodeOptions {
                prefix: ": ".to_owned(),
                ..NodeOptions::default()
            },
        );
        true
    }

    /// tsgo `ParenthesizeArrowParameters`: `(` and `)` around a paren-less
    /// arrow function's parameters.
    pub(crate) fn parenthesize_arrow_parameters(&mut self, file: &'a SourceFile, arrow: NodeId) {
        let navigator = Navigator::new(file);
        if navigator
            .child_of_kind(View::Node(arrow), SyntaxKind::CloseParenToken)
            .is_some()
        {
            return;
        }
        let parameters = crate::utilities::parameters_in(file, arrow);
        let (Some(&first), Some(&last)) = (parameters.first(), parameters.last()) else {
            return;
        };
        let start = start_of(file, first);
        self.insert_text_at(file, start, "(".to_owned());
        self.insert_text_at(file, file.arena.node(last).end as usize, ")".to_owned());
    }

    /// tsgo `InsertModifierBefore`: a modifier token (like `type`) before a
    /// node, with a space after it.
    pub(crate) fn insert_modifier_before(
        &mut self,
        file: &'a SourceFile,
        modifier: SyntaxKind,
        before: NodeId,
    ) -> Result<(), TransformError> {
        let pos = start_of(file, before);
        let token = self.new_token(file, modifier)?;
        self.insert_node_at(
            file,
            pos,
            token,
            NodeOptions {
                suffix: " ".to_owned(),
                ..NodeOptions::default()
            },
        );
        Ok(())
    }

    /// tsgo `NewToken` for the file.
    fn new_token(
        &mut self,
        file: &'a SourceFile,
        kind: SyntaxKind,
    ) -> Result<TransformNode, TransformError> {
        let source = self.source(file);
        self.arena()
            .factory()
            .create_token(source, kind, TransformFlags::NONE)
    }

    /// The kind of a node of the tracker's arena.
    fn new_node_kind(&mut self, file: &'a SourceFile, node: TransformNode) -> SyntaxKind {
        self.syntax(file).node(node.node()).kind
    }

    /// tsgo `endPosForInsertNodeAfter`: the end of `after`, a semicolon
    /// inserted first where ASI would join the statements.
    fn end_pos_for_insert_node_after(
        &mut self,
        file: &'a SourceFile,
        after: NodeId,
        node: TransformNode,
    ) -> Result<usize, TransformError> {
        let after_end = file.arena.node(after).end as usize;
        if self.need_semicolon_between(file, after, node)
            && file.text().as_bytes()[after_end - 1] != b';'
        {
            // check if previous statement ends with semicolon
            // if not - insert semicolon to preserve the code from changing the meaning due to ASI
            let semicolon = self.new_token(file, SyntaxKind::SemicolonToken)?;
            self.replace_range(
                file,
                (after_end, after_end),
                semicolon,
                NodeOptions::default(),
            );
        }
        Ok(self.get_adjusted_end_position(file, after, TrailingTriviaOption::None))
    }

    /// tsgo `needSemicolonBetween`.
    fn need_semicolon_between(
        &mut self,
        file: &'a SourceFile,
        a: NodeId,
        b: TransformNode,
    ) -> bool {
        let a_kind = file.arena.node(a).kind;
        let b_kind = self.new_node_kind(file, b);
        let mut b_has_computed_name = || {
            let syntax = self.syntax(file);
            let name = match &syntax.node(b.node()).data {
                NodeData::PropertyDeclaration(data) => data.name,
                NodeData::PropertySignature(data) => data.name,
                NodeData::MethodDeclaration(data) => data.name,
                NodeData::MethodSignature(data) => data.name,
                NodeData::GetAccessor(data) => data.name,
                NodeData::SetAccessor(data) => data.name,
                _ => None,
            };
            name.is_some_and(|name| syntax.node(name).kind == SyntaxKind::ComputedPropertyName)
        };
        matches!(
            a_kind,
            SyntaxKind::PropertySignature | SyntaxKind::PropertyDeclaration
        ) && is_class_or_type_element(b_kind)
            && b_has_computed_name()
            || format::is_statement_but_not_declaration_kind(a_kind)
                && format::is_statement_but_not_declaration_kind(b_kind) // TODO: only if b would start with a `(` or `[`
    }

    /// tsgo `getInsertNodeAfterOptions`.
    fn get_insert_node_after_options(&self, file: &SourceFile, node: NodeId) -> NodeOptions {
        let new_line = self.new_line.text();
        let kind = file.arena.node(node).kind;
        let mut options = match kind {
            SyntaxKind::Parameter => NodeOptions::default(),
            SyntaxKind::ClassDeclaration | SyntaxKind::ModuleDeclaration => NodeOptions {
                prefix: new_line.to_owned(),
                suffix: new_line.to_owned(),
                ..NodeOptions::default()
            },
            SyntaxKind::VariableDeclaration
            | SyntaxKind::StringLiteral
            | SyntaxKind::Identifier => NodeOptions {
                prefix: ", ".to_owned(),
                ..NodeOptions::default()
            },
            SyntaxKind::PropertyAssignment => NodeOptions {
                suffix: format!(",{new_line}"),
                ..NodeOptions::default()
            },
            SyntaxKind::ExportKeyword => NodeOptions {
                prefix: " ".to_owned(),
                ..NodeOptions::default()
            },
            _ => {
                if !(is_statement(file, node) || is_class_or_type_element(kind)) {
                    // Else we haven't handled this kind of node yet -- add it
                    panic!(
                        "unimplemented node type {kind:?} in changeTracker.getInsertNodeAfterOptions"
                    );
                }
                NodeOptions {
                    suffix: new_line.to_owned(),
                    ..NodeOptions::default()
                }
            }
        };
        if file.arena.node(node).end == file.arena.node(file.root).end && is_statement(file, node) {
            options.prefix = format!("{new_line}{}", options.prefix);
        }
        options
    }

    /// tsgo `getOptionsForInsertNodeBefore`.
    fn get_options_for_insert_node_before(
        &mut self,
        file: &'a SourceFile,
        before: NodeId,
        inserted: TransformNode,
        blank_line_between: bool,
    ) -> NodeOptions {
        let new_line = self.new_line.text();
        let kind = file.arena.node(before).kind;
        let suffix = |suffix: String| NodeOptions {
            suffix,
            ..NodeOptions::default()
        };
        if is_statement(file, before) || is_class_or_type_element(kind) {
            if blank_line_between {
                return suffix(format!("{new_line}{new_line}"));
            }
            return suffix(new_line.to_owned());
        }
        match kind {
            // insert `x = 1, ` into `const x = 1, y = 2;
            SyntaxKind::VariableDeclaration => suffix(", ".to_owned()),
            SyntaxKind::Parameter => {
                if self.new_node_kind(file, inserted) == SyntaxKind::Parameter {
                    suffix(", ".to_owned())
                } else {
                    NodeOptions::default()
                }
            }
            SyntaxKind::StringLiteral
                if file.arena.node(before).parent.is_some_and(|parent| {
                    file.arena.node(parent).kind == SyntaxKind::ImportDeclaration
                }) =>
            {
                suffix(", ".to_owned())
            }
            SyntaxKind::NamedImports => suffix(", ".to_owned()),
            SyntaxKind::ImportSpecifier => suffix(format!(
                ",{}",
                if blank_line_between { new_line } else { " " }
            )),
            // We haven't handled this kind of node yet -- add it
            _ => panic!(
                "unimplemented node type {kind:?} in changeTracker.getOptionsForInsertNodeBefore"
            ),
        }
    }

    /// tsgo `InsertNodeInListAfter`: for lists whose separators are not part
    /// of their items (arguments, parameters, specifiers, ...).
    pub(crate) fn insert_node_in_list_after(
        &mut self,
        file: &'a SourceFile,
        after: NodeId,
        node: TransformNode,
        containing_list: Option<NodeArrayId>,
    ) -> Result<(), TransformError> {
        let formatting = File::new(file);
        let Some(containing_list) = containing_list.or_else(|| {
            format::get_containing_list(&formatting, Found::Node(View::Node(after)))
                .map(|list| list.id)
        }) else {
            // Debug.fail("node is not a list element")
            return Ok(());
        };
        let list = file.arena.node_array(containing_list);
        let Some(index) = list.nodes.iter().position(|&item| item == after) else {
            return Ok(());
        };
        let text = file.text();
        let navigator = Navigator::new(file);
        let end = file.arena.node(after).end as usize;
        if index != list.nodes.len() - 1 {
            // any element except the last one
            // use next sibling as an anchor
            let next_token = navigator.token_at(end as u32);
            if is_separator(file, &navigator, after, Some(next_token)) {
                // for list
                // a, b, c
                // create change for adding 'e' after 'a' as
                // - find start of next element after a (it is b)
                // - use next element start as start and end position in final change
                // - build text of change by formatting the text of node + whitespace trivia of b

                // in multiline case it will work as
                //   a,
                //   b,
                //   c,
                // result - '*' denotes leading trivia that will be inserted after new text (displayed as '#')
                //   a,
                //   insertedtext<separator>#
                // ###b,
                //   c,
                let next_node = list.nodes[index + 1];
                let start_pos = skip_trivia_ex(
                    text,
                    file.arena.node(next_node).pos as usize,
                    SkipTriviaOptions {
                        stop_at_comments: true,
                        ..Default::default()
                    },
                );
                // write separator and leading trivia of the next element as suffix
                let suffix = format!(
                    "{}{}",
                    tsc_syntax::tokens::token_to_string(navigator.kind(next_token))
                        .unwrap_or_default(),
                    &text[navigator.end(next_token) as usize..start_pos]
                );
                self.insert_nodes_at(
                    file,
                    start_pos,
                    vec![node],
                    NodeOptions {
                        suffix,
                        ..NodeOptions::default()
                    },
                );
            }
            return Ok(());
        }

        let after_start = start_of(file, after);
        let after_start_line_position = line_start_of(file, after_start);

        // insert element after the last element in the list that has more than one item
        // pick the element preceding the after element to:
        // - pick the separator
        // - determine if list is a multiline
        let mut multiline_list = false;

        // if list has only one element then we'll format is as multiline if node has comment in trailing trivia, or as singleline otherwise
        // i.e. var x = 1 // this is x
        //     | new element will be inserted at this position
        let mut separator = SyntaxKind::CommaToken; // SyntaxKind.CommaToken | SyntaxKind.SemicolonToken
        if list.nodes.len() != 1 {
            // otherwise, if list has more than one element, pick separator from the list
            let token_before_insert_position =
                navigator.preceding_token(file.arena.node(after).pos);
            separator = match token_before_insert_position {
                Some(token) if is_separator(file, &navigator, after, Some(token)) => {
                    navigator.kind(token)
                }
                _ => SyntaxKind::CommaToken,
            };
            // determine if list is multiline by checking lines of after element and element that precedes it.
            let after_minus_one_start_line_position =
                line_start_of(file, start_of(file, list.nodes[index - 1]));
            multiline_list = after_minus_one_start_line_position != after_start_line_position;
        }
        if has_comments_before_line_break(text, end)
            || !positions_are_on_same_line(file, list.pos as usize, list.end as usize)
        {
            // in this case we'll always treat containing list as multiline
            multiline_list = true;
        }
        let separator_string = tsc_syntax::tokens::token_to_string(separator).unwrap_or_default();
        if multiline_list {
            // insert separator immediately following the 'after' node to preserve comments in trailing trivia
            let separator_token = self.new_token(file, separator)?;
            self.replace_range(file, (end, end), separator_token, NodeOptions::default());
            // use the same indentation as 'after' item
            let indentation = format::find_first_non_whitespace_column(
                &formatting,
                after_start_line_position,
                after_start,
                &self.format_settings,
            );
            // insert element before the line break on the line that contains 'after' element
            let mut insert_pos = skip_trivia_ex(
                text,
                end,
                SkipTriviaOptions {
                    stop_after_line_break: true,
                    ..Default::default()
                },
            );
            // find position before "\n" or "\r\n"
            while insert_pos != end && is_line_break_byte(text.as_bytes()[insert_pos - 1]) {
                insert_pos -= 1;
            }
            self.replace_range(
                file,
                (insert_pos, insert_pos),
                node,
                NodeOptions {
                    indentation: Some(indentation),
                    prefix: self.new_line.text().to_owned(),
                    ..NodeOptions::default()
                },
            );
        } else {
            self.replace_range(
                file,
                (end, end),
                node,
                NodeOptions {
                    prefix: format!("{separator_string} "),
                    ..NodeOptions::default()
                },
            );
        }
        Ok(())
    }

    /// tsgo `InsertImportSpecifierAtIndex`: a new specifier at `index` of a
    /// `NamedImports` list.
    pub(crate) fn insert_import_specifier_at_index(
        &mut self,
        file: &'a SourceFile,
        specifier: TransformNode,
        named_imports: NodeId,
        index: usize,
    ) -> Result<(), TransformError> {
        let NodeData::NamedImports(data) = &file.arena.node(named_imports).data else {
            return Ok(());
        };
        let elements = data
            .elements
            .map(|elements| file.arena.node_array(elements).nodes)
            .unwrap_or_default();
        let previous = (index > 0).then(|| elements.get(index - 1)).flatten();
        if let Some(&previous) = previous {
            return self.insert_node_in_list_after(file, previous, specifier, None);
        }
        let declaration = file
            .arena
            .node(named_imports)
            .parent
            .and_then(|clause| file.arena.node(clause).parent)
            .expect("named imports in an import declaration");
        let blank_line_between = !positions_are_on_same_line(
            file,
            start_of(file, elements[0]),
            start_of(file, declaration),
        );
        self.insert_node_before(
            file,
            elements[0],
            specifier,
            blank_line_between,
            LeadingTriviaOption::None,
        );
        Ok(())
    }

    /// tsgo `InsertAtTopOfFile`.
    pub(crate) fn insert_at_top_of_file(
        &mut self,
        file: &'a SourceFile,
        mut insert: Vec<TransformNode>,
        blank_line_between: bool,
    ) {
        if insert.is_empty() {
            return;
        }
        let pos = get_insertion_position_at_source_file_top(file);
        let new_line = self.new_line.text();
        let mut options = NodeOptions::default();
        if pos != 0 {
            options.prefix = new_line.to_owned();
        }
        let text = file.text();
        if text.is_empty()
            || !text
                .as_bytes()
                .get(pos)
                .copied()
                .is_some_and(is_line_break_byte)
        {
            options.suffix = new_line.to_owned();
        }
        if blank_line_between {
            options.suffix.push_str(new_line);
        }
        if insert.len() == 1 {
            self.insert_node_at(file, pos, insert.remove(0), options);
        } else {
            self.insert_nodes_at(file, pos, insert, options);
        }
    }
}

/// tsgo `getInsertionPositionAtSourceFileTop`: after the prologue, the
/// shebang and the header comments.
fn get_insertion_position_at_source_file_top(file: &SourceFile) -> usize {
    let statements = crate::references::statements(file, file.root);
    let last_prologue = statements
        .iter()
        .take_while(|&&statement| is_prologue_directive(file, statement))
        .last();
    let text = file.text();
    let advance_past_line_break = |mut position: usize| {
        if let Some(&byte) = text.as_bytes().get(position) {
            if is_line_break_byte(byte) {
                position += 1;
                if byte == b'\r' && text.as_bytes().get(position) == Some(&b'\n') {
                    position += 1;
                }
            }
        }
        position
    };
    if let Some(&last_prologue) = last_prologue {
        return advance_past_line_break(file.arena.node(last_prologue).end as usize);
    }

    let mut position = 0;
    let shebang = tsc_syntax::get_shebang(text);
    if !shebang.is_empty() {
        position = advance_past_line_break(shebang.len());
    }

    let ranges = comment_ranges(text, position, false);
    if ranges.is_empty() {
        return position;
    }
    // Find the first attached comment to the first node and add before it
    let mut last_comment: Option<(usize, usize)> = None;
    let mut pinned_or_triple_slash = false;
    let mut first_node_line = None;
    for (start, end) in ranges {
        if text[start..].starts_with("/*") {
            if tsc_emitter::is_pinned_comment(text, start, end) {
                last_comment = Some((start, end));
                pinned_or_triple_slash = true;
                continue;
            }
        } else if tsc_emitter::is_recognized_triple_slash_comment(&text[start..end]) {
            last_comment = Some((start, end));
            pinned_or_triple_slash = true;
            continue;
        }

        if let Some((_, last_end)) = last_comment {
            // Always insert after pinned or triple slash comments
            if pinned_or_triple_slash {
                break;
            }
            // There was a blank line between the last comment and this comment.
            // This comment is not part of the copyright comments
            if line_of(file, start) >= line_of(file, last_end) + 2 {
                break;
            }
        }

        if let Some(&first) = statements.first() {
            let first_node_line =
                *first_node_line.get_or_insert_with(|| line_of(file, start_of(file, first)));
            if first_node_line < line_of(file, end) + 2 {
                break;
            }
        }
        last_comment = Some((start, end));
        pinned_or_triple_slash = false;
    }
    if let Some((_, end)) = last_comment {
        position = advance_past_line_break(end);
    }
    position
}

/// tsgo `ast.IsPrologueDirective`.
fn is_prologue_directive(file: &SourceFile, statement: NodeId) -> bool {
    matches!(
        &file.arena.node(statement).data,
        NodeData::ExpressionStatement(data)
            if data.expression.is_some_and(|expression| {
                file.arena.node(expression).kind == SyntaxKind::StringLiteral
            })
    )
}

/// tsgo `astnav.GetStartOfNode(node, file, false)`.
fn start_of(file: &SourceFile, node: NodeId) -> usize {
    Navigator::new(file).start_of_node(Found::Node(View::Node(node)), false) as usize
}

/// tsgo `hasCommentsBeforeLineBreak`.
fn has_comments_before_line_break(text: &str, start: usize) -> bool {
    text[start..]
        .chars()
        .find(|&ch| !tsc_syntax::is_single_line_whitespace(ch))
        .is_some_and(|ch| ch == '/')
}

/// tsgo's `stringutil.IsLineBreak(rune(text[i]))`: a byte of the text
/// read as a rune, so only `\n` and `\r`.
fn is_line_break_byte(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r')
}

/// tsgo `ast.IsClassOrTypeElement`.
fn is_class_or_type_element(kind: SyntaxKind) -> bool {
    crate::utilities::is_class_element(kind) || crate::completions::is_type_element(kind)
}

/// tsgo `ast.IsStatement`.
pub(crate) fn is_statement(file: &SourceFile, node: NodeId) -> bool {
    let kind = file.arena.node(node).kind;
    format::is_statement_but_not_declaration_kind(kind)
        || tsc_binder::bind::is_declaration_statement_kind(kind)
        || kind == SyntaxKind::Block && {
            // tsgo `isBlockStatement`: not a `try`'s, a `catch`'s or a
            // function's block.
            let parent = file
                .arena
                .node(node)
                .parent
                .map(|parent| file.arena.node(parent).kind);
            !matches!(
                parent,
                Some(SyntaxKind::TryStatement | SyntaxKind::CatchClause)
            ) && !parent.is_some_and(crate::utilities::is_function_like_kind)
        }
}

/// tsgo `getMembersOrProperties`.
fn members_or_properties(file: &SourceFile, node: NodeId) -> Option<NodeArrayId> {
    match &file.arena.node(node).data {
        NodeData::ObjectLiteralExpression(data) => data.properties,
        NodeData::ClassDeclaration(data) => data.members,
        NodeData::ClassExpression(data) => data.members,
        NodeData::InterfaceDeclaration(data) => data.members,
        NodeData::EnumDeclaration(data) => data.members,
        NodeData::TypeLiteral(data) => data.members,
        NodeData::MappedType(data) => data.members,
        _ => None,
    }
}

/// tsgo `findIndentationColumn`: the column of `member_start` when only
/// white space precedes it on its line (tabs to the next tab stop), else
/// the column of the first other character; -1 past a line break.
fn find_indentation_column(
    text: &str,
    line_start: usize,
    member_start: usize,
    tab_size: i64,
) -> i64 {
    let mut column = 0;
    for ch in text[line_start.min(text.len())..member_start.min(text.len())].chars() {
        if tsc_syntax::is_line_break(ch) {
            return -1;
        }
        if tsc_syntax::is_single_line_whitespace(ch) {
            column = if ch == '\t' {
                column + tab_size - (column % tab_size)
            } else {
                column + 1
            };
            continue;
        }
        return column;
    }
    column
}

/// A format setting, `default` unless positive.
fn positive_or(value: i64, default: i64) -> i64 {
    if value > 0 {
        value
    } else {
        default
    }
}

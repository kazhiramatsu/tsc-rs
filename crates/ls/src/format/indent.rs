//! tsgo `format/indent.go`: the indentation of a node, from its ancestors
//! and the indentation the source already has.

use tsc_syntax::{SyntaxKind, TokenScanner};

use super::file::{
    decode_rune, is_statement_but_not_declaration_kind, is_white_space_single_line, File, List,
    Node,
};
use super::util::range_is_on_one_line;
use crate::lsutil::{FormatCodeSettings, IndentStyle};

/// tsgo `GetIndentation`: the indentation expected at a position (the
/// smart indenter's).
pub(crate) fn get_indentation(
    file: &File<'_>,
    position: usize,
    options: &FormatCodeSettings,
    assume_new_line_before_close_brace: bool,
) -> i64 {
    if position > file.text().len() {
        return options.editor_settings.base_indent_size; // past EOF
    }

    // no indentation when the indent style is set to none,
    // so we can return fast
    if options.editor_settings.indent_style == IndentStyle::NONE {
        return 0;
    }

    let preceding_token = file.find_preceding_token_excluding_js_doc(position);

    let enclosing_comment_range = crate::utilities::range_of_enclosing_comment(
        &file.navigator,
        position as u32,
        preceding_token,
        file.token_at_position(position),
    );
    if let Some((comment_pos, _)) = enclosing_comment_range {
        if file.text()[comment_pos as usize..].starts_with("/*") {
            return get_comment_indent(file, position, options, comment_pos as usize);
        }
    }

    let Some(preceding_token) = preceding_token else {
        return options.editor_settings.base_indent_size;
    };

    // no indentation in string/regex/template literals
    if matches!(
        file.kind(preceding_token),
        SyntaxKind::StringLiteral
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead
            | SyntaxKind::TemplateMiddle
            | SyntaxKind::TemplateTail
    ) {
        let token_start = file.token_start(preceding_token);
        if token_start <= position && position < file.end(preceding_token) {
            return 0;
        }
    }

    let line_at_position = file.line_of(position);

    // indentation is first non-whitespace character in a previous line
    // for block indentation, we should look for a line which contains something that's not
    // whitespace.
    let current_token = file.token_at_position(position);
    // For object literals, we want indentation to work just like with blocks.
    // If the `{` starts in any position (even in the middle of a line), then
    // the following indentation should treat `{` as the start of that line (including leading whitespace).
    let is_object_literal = file.kind(current_token) == SyntaxKind::OpenBraceToken
        && file
            .parent(current_token)
            .is_some_and(|parent| file.kind(parent) == SyntaxKind::ObjectLiteralExpression);
    if options.editor_settings.indent_style == IndentStyle::BLOCK || is_object_literal {
        return get_block_indent(file, position, options);
    }

    if file.kind(preceding_token) == SyntaxKind::CommaToken
        && file
            .parent(preceding_token)
            .is_some_and(|parent| file.kind(parent) != SyntaxKind::BinaryExpression)
    {
        // previous token is comma that separates items in list - find the previous item and try to derive indentation from it
        let actual_indentation =
            get_actual_indentation_for_list_item_before_comma(file, preceding_token, options);
        if actual_indentation != -1 {
            return actual_indentation;
        }
    }

    let container_list = file
        .parent(preceding_token)
        .and_then(|parent| get_list_by_range(file, position, position, parent));
    // use list position if the preceding token is before any list items
    if let Some(container_list) = container_list {
        let (list_pos, list_end) = file.list_range(container_list.id);
        if !(list_pos <= file.pos(preceding_token) && file.end(preceding_token) <= list_end) {
            let use_the_same_base_indentation = file.parent(current_token).is_some_and(|parent| {
                matches!(
                    file.kind(parent),
                    SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
                )
            });
            let indent_size = if use_the_same_base_indentation {
                0
            } else {
                options.editor_settings.indent_size
            };
            let res = get_actual_indentation_for_list_start_line(file, container_list, options);
            if res == -1 {
                return indent_size;
            }
            return res + indent_size;
        }
    }

    get_smart_indent(
        file,
        position,
        preceding_token,
        line_at_position,
        assume_new_line_before_close_brace,
        options,
    )
}

fn get_comment_indent(
    file: &File<'_>,
    position: usize,
    options: &FormatCodeSettings,
    comment_pos: usize,
) -> i64 {
    let previous_line = file.line_of(position) as i64 - 1;
    let comment_start_line = file.line_of(comment_pos);

    if previous_line <= comment_start_line as i64 {
        return find_first_non_whitespace_column(
            file,
            file.line_starts()[comment_start_line],
            position,
            options,
        );
    }

    let start_position_of_line = file.line_starts()[previous_line as usize];
    let (character, column) = find_first_non_whitespace_character_and_column(
        file,
        start_position_of_line,
        position,
        options,
    );

    if column == 0 {
        return column;
    }

    let first_non_whitespace_character_code =
        file.text().as_bytes()[start_position_of_line + character];
    if first_non_whitespace_character_code == b'*' {
        return column - 1;
    }
    column
}

fn get_block_indent(file: &File<'_>, position: usize, options: &FormatCodeSettings) -> i64 {
    // move backwards until we find a line with a non-whitespace character,
    // then find the first non-whitespace character for that line.
    let mut current = position;
    while current > 0 {
        let (ch, size) = decode_rune(file.text(), current);
        if !tsc_syntax::is_whitespace_like(ch) {
            break;
        }
        current -= size;
    }

    let line_start = super::util::get_line_start_position_for_position(file, current);
    find_first_non_whitespace_column(file, line_start, current, options)
}

fn get_actual_indentation_for_list_item_before_comma(
    file: &File<'_>,
    comma_token: Node,
    options: &FormatCodeSettings,
) -> i64 {
    // previous token is comma that separates items in list - find the previous item and try to derive indentation from it
    let Some(containing_list) = get_containing_list(file, comma_token) else {
        return -1;
    };
    let nodes = file.list_nodes(containing_list);
    match nodes.iter().position(|&node| node == comma_token) {
        Some(comma_index) if comma_index > 0 => {
            derive_actual_indentation_from_list(file, &nodes, comma_index - 1, options)
        }
        _ => -1,
    }
}

/// tsgo `nextTokenKind`.
#[derive(Clone, Copy, Eq, PartialEq)]
enum NextTokenKind {
    Unknown,
    OpenBrace,
    CloseBrace,
}

fn next_token_is_curly_brace_on_same_line_as_cursor(
    file: &File<'_>,
    preceding_token: Node,
    current: Node,
    line_at_position: usize,
) -> NextTokenKind {
    let Some(next_token) = file.find_next_token(preceding_token, current) else {
        return NextTokenKind::Unknown;
    };

    match file.kind(next_token) {
        // open braces are always indented at the parent level
        SyntaxKind::OpenBraceToken => NextTokenKind::OpenBrace,
        // close braces are indented at the parent level if they are located on the same line with cursor
        SyntaxKind::CloseBraceToken
            if get_start_line_for_node(file, next_token) == line_at_position =>
        {
            NextTokenKind::CloseBrace
        }
        _ => NextTokenKind::Unknown,
    }
}

fn get_smart_indent(
    file: &File<'_>,
    position: usize,
    preceding_token: Node,
    line_at_position: usize,
    assume_new_line_before_close_brace: bool,
    options: &FormatCodeSettings,
) -> i64 {
    // try to find node that can contribute to indentation and includes 'position' starting from 'precedingToken'
    // if such node is found - compute initial indentation for 'position' inside this node
    let mut previous = None;
    let mut current = Some(preceding_token);

    while let Some(node) = current {
        if crate::lsutil::position_belongs_to_node(&file.navigator, node, position as u32)
            && should_indent_child_node(options, file, node, previous, true, true)
        {
            let (current_start_line, current_start_char) =
                get_start_line_and_character_for_node(file, node);
            let ntk = next_token_is_curly_brace_on_same_line_as_cursor(
                file,
                preceding_token,
                node,
                line_at_position,
            );
            let mut indentation_delta = 0;
            if ntk != NextTokenKind::Unknown {
                // handle cases when codefix is about to be inserted before the close brace
                if assume_new_line_before_close_brace && ntk == NextTokenKind::CloseBrace {
                    indentation_delta = options.editor_settings.indent_size;
                }
            } else if line_at_position != current_start_line {
                indentation_delta = options.editor_settings.indent_size;
            }
            return get_indentation_for_node_worker(
                file,
                node,
                current_start_line,
                current_start_char,
                None,
                indentation_delta,
                true,
                options,
            );
        }

        // check if current node is a list item - if yes, take indentation from it
        // do not consider parent-child line sharing yet:
        // function foo(a
        //    | preceding node 'a' does share line with its parent but indentation is expected
        let actual_indentation = get_actual_indentation_for_list_item(file, node, options, true);
        if actual_indentation != -1 {
            return actual_indentation;
        }

        previous = Some(node);
        current = file.parent(node);
    }
    // no parent was found - return the base indentation of the SourceFile
    options.editor_settings.base_indent_size
}

/// tsgo `GetIndentationForNode`.
pub(crate) fn get_indentation_for_node(
    file: &File<'_>,
    n: Node,
    ignore_actual_indentation_range: Option<(usize, usize)>,
    options: &FormatCodeSettings,
) -> i64 {
    let (start_line, start_pos) = file.line_and_offset(file.token_start(n));
    get_indentation_for_node_worker(
        file,
        n,
        start_line,
        start_pos,
        ignore_actual_indentation_range,
        0,
        false,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
fn get_indentation_for_node_worker(
    file: &File<'_>,
    mut current: Node,
    mut current_start_line: usize,
    mut current_start_character: usize,
    ignore_actual_indentation_range: Option<(usize, usize)>,
    mut indentation_delta: i64,
    is_next_child: bool,
    options: &FormatCodeSettings,
) -> i64 {
    let mut parent = file.parent(current);

    // Walk up the tree and collect indentation for parent-child node pairs. Indentation is not added if
    // * parent and child nodes start on the same line, or
    // * parent is an IfStatement and child starts on the same line as an 'else clause'.
    while let Some(p) = parent {
        let mut use_actual_indentation = true;
        if let Some((pos, end)) = ignore_actual_indentation_range {
            let start = file.token_start(current);
            use_actual_indentation = start < pos || start > end;
        }

        let (containing_list_or_parent_start_line, containing_list_or_parent_start_character) =
            get_containing_list_or_parent_start(file, p, current);
        let parent_and_child_share_line = containing_list_or_parent_start_line
            == current_start_line
            || child_starts_on_the_same_line_with_else_in_if_statement(
                file,
                p,
                current,
                current_start_line,
            );

        if use_actual_indentation {
            // check if current node is a list item - if yes, take indentation from it
            let first_list_child = get_containing_list(file, current)
                .and_then(|list| file.list_nodes(list).first().copied());
            // A list indents its children if the children begin on a later line than the list itself:
            //
            // f1(               L0 - List start
            //   {               L1 - First child start: indented, along with all other children
            //     prop: 0
            //   },
            //   {
            //     prop: 1
            //   }
            // )
            //
            // f2({             L0 - List start and first child start: children are not indented.
            //   prop: 0             Object properties are indented only one level, because the list
            // }, {                  itself contributes nothing.
            //   prop: 1        L3 - The indentation of the second object literal is best understood by
            // })                    looking at the relationship between the list and *first* list item.
            let list_indents_child = first_list_child.is_some_and(|first| {
                get_start_line_for_node(file, first) > containing_list_or_parent_start_line
            });
            let actual_indentation =
                get_actual_indentation_for_list_item(file, current, options, list_indents_child);
            if actual_indentation != -1 {
                return actual_indentation + indentation_delta;
            }

            // try to fetch actual indentation for current node from source text
            let actual_indentation = get_actual_indentation_for_node(
                file,
                current,
                p,
                current_start_line,
                current_start_character,
                parent_and_child_share_line,
                options,
            );
            if actual_indentation != -1 {
                return actual_indentation + indentation_delta;
            }
        }

        // increase indentation if parent node wants its content to be indented and parent and child nodes don't start on the same line
        if should_indent_child_node(options, file, p, Some(current), true, is_next_child)
            && !parent_and_child_share_line
        {
            indentation_delta += options.editor_settings.indent_size;
        }

        // In our AST, a call argument's `parent` is the call-expression, not the argument list.
        // We would like to increase indentation based on the relationship between an argument and its argument-list,
        // so we spoof the starting position of the (parent) call-expression to match the (non-parent) argument-list.
        // But, the spoofed start-value could then cause a problem when comparing the start position of the call-expression
        // to *its* parent (in the case of an iife, an expression statement), adding an extra level of indentation.
        //
        // Instead, when at an argument, we unspoof the starting position of the enclosing call expression
        // *after* applying indentation for the argument.
        let use_true_start = is_argument_and_start_line_overlaps_expression_being_called(
            file,
            p,
            current,
            current_start_line,
        );

        current = p;
        parent = file.parent(current);

        if use_true_start {
            (current_start_line, current_start_character) =
                file.line_and_offset(file.token_start(current));
        } else {
            current_start_line = containing_list_or_parent_start_line;
            current_start_character = containing_list_or_parent_start_character;
        }
    }

    indentation_delta + options.editor_settings.base_indent_size
}

/// tsgo `getActualIndentationForNode`: -1 if the actual indentation of the
/// node should not be used (because the node is a nested expression, say).
fn get_actual_indentation_for_node(
    file: &File<'_>,
    current: Node,
    parent: Node,
    current_line: usize,
    current_char: usize,
    parent_and_child_share_line: bool,
    options: &FormatCodeSettings,
) -> i64 {
    // actual indentation is used for statements\declarations if one of cases below is true:
    // - parent is SourceFile - by default immediate children of SourceFile are not indented except when user indents them manually
    // - parent and child are not on the same line
    let use_actual_indentation = (file.is_declaration(current)
        || is_statement_but_not_declaration_kind(file.kind(current)))
        && (file.kind(parent) == SyntaxKind::SourceFile || !parent_and_child_share_line);

    if !use_actual_indentation {
        return -1;
    }

    find_column_for_first_non_whitespace_character_in_line(
        file,
        current_line,
        current_char,
        options,
    )
}

fn is_argument_and_start_line_overlaps_expression_being_called(
    file: &File<'_>,
    parent: Node,
    child: Node,
    child_start_line: usize,
) -> bool {
    if !(file.kind(parent) == SyntaxKind::CallExpression && file.arguments(parent).contains(&child))
    {
        return false;
    }
    let Some(expression) = file.expression(parent) else {
        return false;
    };
    let expression_of_call_expression_end_line = file.line_of(file.end(expression));
    expression_of_call_expression_end_line == child_start_line
}

fn get_actual_indentation_for_list_item(
    file: &File<'_>,
    node: Node,
    options: &FormatCodeSettings,
    list_indents_child: bool,
) -> i64 {
    if file
        .parent(node)
        .is_some_and(|parent| file.kind(parent) == SyntaxKind::VariableDeclarationList)
    {
        // VariableDeclarationList has no wrapping tokens
        return -1;
    }
    let Some(containing_list) = get_containing_list(file, node) else {
        return -1;
    };
    let nodes = file.list_nodes(containing_list);
    if let Some(index) = nodes.iter().position(|&element| element == node) {
        let result = derive_actual_indentation_from_list(file, &nodes, index, options);
        if result != -1 {
            return result;
        }
    }
    let delta = if list_indents_child {
        options.editor_settings.indent_size
    } else {
        0
    };
    let res = get_actual_indentation_for_list_start_line(file, containing_list, options);
    if res == -1 {
        return delta;
    }
    res + delta
}

fn get_actual_indentation_for_list_start_line(
    file: &File<'_>,
    list: List,
    options: &FormatCodeSettings,
) -> i64 {
    let (line, char) = file.line_and_offset(file.list_range(list.id).0);
    find_column_for_first_non_whitespace_character_in_line(file, line, char, options)
}

fn derive_actual_indentation_from_list(
    file: &File<'_>,
    nodes: &[Node],
    index: usize,
    options: &FormatCodeSettings,
) -> i64 {
    let node = nodes[index];

    // walk toward the start of the list starting from current node and check if the line is the same for all items.
    // if end line for item [i - 1] differs from the start line for item [i] - find column of the first non-whitespace character on the line of item [i]

    let (mut line, mut char) = get_start_line_and_character_for_node(file, node);

    for &element in nodes[..=index].iter().rev() {
        if file.kind(element) == SyntaxKind::CommaToken {
            continue;
        }
        // skip list items that ends on the same line with the current list element
        let prev_end_line = file.line_of(file.end(element));
        if prev_end_line != line {
            return find_column_for_first_non_whitespace_character_in_line(
                file, line, char, options,
            );
        }

        (line, char) = get_start_line_and_character_for_node(file, element);
    }
    -1
}

fn find_column_for_first_non_whitespace_character_in_line(
    file: &File<'_>,
    line: usize,
    char: usize,
    options: &FormatCodeSettings,
) -> i64 {
    let line_start = file.line_starts()[line];
    find_first_non_whitespace_column(file, line_start, line_start + char, options)
}

/// tsgo `FindFirstNonWhitespaceColumn`.
pub(crate) fn find_first_non_whitespace_column(
    file: &File<'_>,
    start_pos: usize,
    end_pos: usize,
    options: &FormatCodeSettings,
) -> i64 {
    find_first_non_whitespace_character_and_column(file, start_pos, end_pos, options).1
}

/// tsgo `findFirstNonWhitespaceCharacterAndColumn`: the character is the
/// actual index of the character since the beginning of the line, the
/// column its position after expanding tabs to spaces. For "0\t2$" the
/// character of `$` is 3 and its column 6 (with a tab size of 4).
pub(crate) fn find_first_non_whitespace_character_and_column(
    file: &File<'_>,
    start_pos: usize,
    end_pos: usize,
    options: &FormatCodeSettings,
) -> (usize, i64) {
    let mut column = 0;
    let text = file.text();
    let mut pos = start_pos;
    let tab_size = options.editor_settings.tab_size;
    while pos < end_pos {
        let (ch, size) = decode_rune(text, pos);
        if !is_white_space_single_line(ch) {
            break;
        }

        if ch == '\t' {
            if tab_size > 0 {
                column += tab_size + (column % tab_size);
            }
        } else {
            column += 1;
        }

        pos += size;
    }
    (pos - start_pos, column)
}

/// tsgo `childStartsOnTheSameLineWithElseInIfStatement`.
pub(super) fn child_starts_on_the_same_line_with_else_in_if_statement(
    file: &File<'_>,
    parent: Node,
    child: Node,
    child_start_line: usize,
) -> bool {
    if file.kind(parent) == SyntaxKind::IfStatement && file.else_statement(parent) == Some(child) {
        let else_keyword = file
            .find_preceding_token(file.pos(child))
            .expect("an else keyword");
        let else_keyword_start_line = get_start_line_for_node(file, else_keyword);
        return else_keyword_start_line == child_start_line;
    }
    false
}

fn get_start_line_and_character_for_node(file: &File<'_>, n: Node) -> (usize, usize) {
    file.line_and_offset(file.token_start(n))
}

fn get_start_line_for_node(file: &File<'_>, n: Node) -> usize {
    file.line_of(file.token_start(n))
}

/// tsgo `GetContainingList`.
pub(crate) fn get_containing_list(file: &File<'_>, node: Node) -> Option<List> {
    let parent = file.parent(node)?;
    get_list_by_range(file, file.token_start(node), file.end(node), parent)
}

fn get_list_by_range(file: &File<'_>, start: usize, end: usize, node: Node) -> Option<List> {
    let get = |list| get_list(file, list, (start, end), node);
    match file.kind(node) {
        SyntaxKind::TypeReference => get(file.type_argument_list(node)),
        SyntaxKind::ObjectLiteralExpression => get(file.property_list(node)),
        SyntaxKind::ArrayLiteralExpression => get(file.element_list(node)),
        SyntaxKind::TypeLiteral => get(file.member_list(node)),
        SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression
        | SyntaxKind::ArrowFunction
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::MethodSignature
        | SyntaxKind::CallSignature
        | SyntaxKind::Constructor
        | SyntaxKind::ConstructorType
        | SyntaxKind::ConstructSignature => {
            get(file.type_parameter_list(node)).or_else(|| get(file.parameter_list(node)))
        }
        SyntaxKind::GetAccessor => get(file.parameter_list(node)),
        SyntaxKind::ClassDeclaration
        | SyntaxKind::ClassExpression
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::TypeAliasDeclaration => get(file.type_parameter_list(node)),
        SyntaxKind::NewExpression | SyntaxKind::CallExpression => {
            get(file.type_argument_list(node)).or_else(|| get(file.argument_list(node)))
        }
        SyntaxKind::VariableDeclarationList => get(file.declarations(node)),
        SyntaxKind::ObjectBindingPattern
        | SyntaxKind::ArrayBindingPattern
        | SyntaxKind::NamedImports
        | SyntaxKind::NamedExports => get(file.element_list(node)),
        _ => None, // TODO: should this be a panic? It isn't in strada.
    }
}

fn get_list(
    file: &File<'_>,
    list: Option<tsc_syntax::NodeArrayId>,
    (start, end): (usize, usize),
    node: Node,
) -> Option<List> {
    let list = list?;
    let (visual_pos, visual_end) = get_visual_list_range(file, node, file.list_range(list));
    (visual_pos <= start && end <= visual_end).then(|| file.list(list))
}

/// tsgo `getVisualListRange`: the list's range expanded to the end of the
/// token before it and the start of the token after it.
fn get_visual_list_range(
    file: &File<'_>,
    _node: Node,
    (pos, end): (usize, usize),
) -> (usize, usize) {
    let prior_end = file
        .find_preceding_token(pos)
        .map_or(pos, |prior| file.end(prior));
    // Find the token that starts at or after list.End() using the scanner
    let scanner = TokenScanner::new(file.source, end);
    let next_start = if scanner.token() == SyntaxKind::EndOfFileToken {
        end
    } else {
        scanner.token_start()
    };
    (prior_end, next_start)
}

fn get_containing_list_or_parent_start(
    file: &File<'_>,
    parent: Node,
    child: Node,
) -> (usize, usize) {
    let start_pos = match get_containing_list(file, child) {
        Some(list) => file.list_range(list.id).0,
        None => file.token_start(parent),
    };
    file.line_and_offset(start_pos)
}

fn is_control_flow_ending_statement(kind: SyntaxKind, parent_kind: SyntaxKind) -> bool {
    match kind {
        SyntaxKind::ReturnStatement
        | SyntaxKind::ThrowStatement
        | SyntaxKind::ContinueStatement
        | SyntaxKind::BreakStatement => parent_kind != SyntaxKind::Block,
        _ => false,
    }
}

/// tsgo `ShouldIndentChildNode`: whether the parent indents the child by an
/// explicit rule. With `is_next_child`, the indentation of a hypothetical
/// child *after* this one. `with_source_file`: tsgo's source file argument
/// is not nil.
pub(crate) fn should_indent_child_node(
    settings: &FormatCodeSettings,
    file: &File<'_>,
    parent: Node,
    child: Option<Node>,
    with_source_file: bool,
    is_next_child: bool,
) -> bool {
    node_will_indent_child(settings, file, parent, child, with_source_file, false)
        && !(is_next_child
            && child.is_some_and(|child| {
                is_control_flow_ending_statement(file.kind(child), file.kind(parent))
            }))
}

/// tsgo `NodeWillIndentChild`.
pub(crate) fn node_will_indent_child(
    settings: &FormatCodeSettings,
    file: &File<'_>,
    parent: Node,
    child: Option<Node>,
    with_source_file: bool,
    indent_by_default: bool,
) -> bool {
    let child_kind = child.map_or(SyntaxKind::Unknown, |child| file.kind(child));
    let parent_kind = file.kind(parent);

    match parent_kind {
        SyntaxKind::ExpressionStatement
        | SyntaxKind::ClassDeclaration
        | SyntaxKind::ClassExpression
        | SyntaxKind::InterfaceDeclaration
        | SyntaxKind::EnumDeclaration
        | SyntaxKind::TypeAliasDeclaration
        | SyntaxKind::ArrayLiteralExpression
        | SyntaxKind::Block
        | SyntaxKind::ModuleBlock
        | SyntaxKind::ObjectLiteralExpression
        | SyntaxKind::TypeLiteral
        | SyntaxKind::MappedType
        | SyntaxKind::TupleType
        | SyntaxKind::ParenthesizedExpression
        | SyntaxKind::PropertyAccessExpression
        | SyntaxKind::CallExpression
        | SyntaxKind::NewExpression
        | SyntaxKind::VariableStatement
        | SyntaxKind::ExportAssignment
        | SyntaxKind::ReturnStatement
        | SyntaxKind::ConditionalExpression
        | SyntaxKind::ArrayBindingPattern
        | SyntaxKind::ObjectBindingPattern
        | SyntaxKind::JsxOpeningElement
        | SyntaxKind::JsxOpeningFragment
        | SyntaxKind::JsxSelfClosingElement
        | SyntaxKind::JsxExpression
        | SyntaxKind::MethodSignature
        | SyntaxKind::CallSignature
        | SyntaxKind::ConstructSignature
        | SyntaxKind::Parameter
        | SyntaxKind::FunctionType
        | SyntaxKind::ConstructorType
        | SyntaxKind::ParenthesizedType
        | SyntaxKind::TaggedTemplateExpression
        | SyntaxKind::AwaitExpression
        | SyntaxKind::NamedExports
        | SyntaxKind::NamedImports
        | SyntaxKind::ExportSpecifier
        | SyntaxKind::ImportSpecifier
        | SyntaxKind::PropertyDeclaration
        | SyntaxKind::CaseClause
        | SyntaxKind::DefaultClause => true,
        SyntaxKind::CaseBlock => settings.indent_switch_case != Some(false),
        SyntaxKind::VariableDeclaration
        | SyntaxKind::PropertyAssignment
        | SyntaxKind::BinaryExpression => {
            if settings.indent_multi_line_object_literal_beginning_on_blank_line != Some(true)
                && with_source_file
                && child_kind == SyntaxKind::ObjectLiteralExpression
            {
                let child = child.expect("a child");
                return range_is_on_one_line(file, file.pos(child), file.end(child));
            }
            if parent_kind == SyntaxKind::BinaryExpression
                && with_source_file
                && child_kind == SyntaxKind::JsxElement
            {
                let child = child.expect("a child");
                let text = file.text();
                let parent_start_line =
                    file.line_of(tsc_syntax::skip_trivia(text, file.pos(parent)));
                let child_start_line = file.line_of(tsc_syntax::skip_trivia(text, file.pos(child)));
                return parent_start_line != child_start_line;
            }
            if parent_kind != SyntaxKind::BinaryExpression {
                return true;
            }
            indent_by_default
        }
        SyntaxKind::DoStatement
        | SyntaxKind::WhileStatement
        | SyntaxKind::ForInStatement
        | SyntaxKind::ForOfStatement
        | SyntaxKind::ForStatement
        | SyntaxKind::IfStatement
        | SyntaxKind::FunctionDeclaration
        | SyntaxKind::FunctionExpression
        | SyntaxKind::MethodDeclaration
        | SyntaxKind::Constructor
        | SyntaxKind::GetAccessor
        | SyntaxKind::SetAccessor => child_kind != SyntaxKind::Block,
        SyntaxKind::ArrowFunction => {
            if with_source_file && child_kind == SyntaxKind::ParenthesizedExpression {
                let child = child.expect("a child");
                return range_is_on_one_line(file, file.pos(child), file.end(child));
            }
            child_kind != SyntaxKind::Block
        }
        SyntaxKind::ExportDeclaration => child_kind != SyntaxKind::NamedExports,
        SyntaxKind::ImportDeclaration => {
            child_kind != SyntaxKind::ImportClause
                || child
                    .and_then(|child| file.named_bindings(child))
                    .is_some_and(|bindings| file.kind(bindings) != SyntaxKind::NamedImports)
        }
        SyntaxKind::JsxElement => child_kind != SyntaxKind::JsxClosingElement,
        SyntaxKind::JsxFragment => child_kind != SyntaxKind::JsxClosingFragment,
        SyntaxKind::IntersectionType | SyntaxKind::UnionType | SyntaxKind::SatisfiesExpression => {
            if matches!(
                child_kind,
                SyntaxKind::TypeLiteral | SyntaxKind::TupleType | SyntaxKind::MappedType
            ) {
                return false;
            }
            indent_by_default
        }
        SyntaxKind::TryStatement => {
            if child_kind == SyntaxKind::Block {
                return false;
            }
            indent_by_default
        }
        // No explicit rule for given nodes so the result will follow the default value argument
        _ => indent_by_default,
    }
}

/// tsgo `childIsUnindentedBranchOfConditionalExpression`: a multiline
/// conditional typically increases the indentation of its whenTrue and
/// whenFalse children, unless the subexpressions themselves span multiple
/// lines, applying their own indentation: the whenTrue branch begins on the
/// line the condition ends, and the whenFalse branch on the line the
/// whenTrue branch ends.
pub(super) fn child_is_unindented_branch_of_conditional_expression(
    file: &File<'_>,
    parent: Node,
    child: Node,
    child_start_line: usize,
) -> bool {
    if file.kind(parent) != SyntaxKind::ConditionalExpression {
        return false;
    }
    let Some((condition, when_true, when_false)) = file.conditional_parts(parent) else {
        return false;
    };
    if child != when_true && child != when_false {
        return false;
    }
    let condition_end_line = file.line_of(file.end(condition));
    if child == when_true {
        return child_start_line == condition_end_line;
    }
    // On the whenFalse side, we have to look at the whenTrue side, because if that one was
    // indented, whenFalse must also be indented:
    //
    // const y = true
    //   ? 1 : (          L1: whenTrue indented because it's on a new line
    //     0              L2: indented two stops, one because whenTrue was indented
    //   );                   and one because of the parentheses spanning multiple lines
    let true_start_line = get_start_line_for_node(file, when_true);
    let true_end_line = file.line_of(file.end(when_true));
    condition_end_line == true_start_line && true_end_line == child_start_line
}

/// tsgo `argumentStartsOnSameLineAsPreviousArgument`.
pub(super) fn argument_starts_on_same_line_as_previous_argument(
    file: &File<'_>,
    parent: Node,
    child: Node,
    child_start_line: usize,
) -> bool {
    if !matches!(
        file.kind(parent),
        SyntaxKind::CallExpression | SyntaxKind::NewExpression
    ) {
        return false;
    }
    let arguments = file.arguments(parent);
    let Some(current_index) = arguments.iter().position(|&argument| argument == child) else {
        // If it's not one of the arguments, don't look past this
        return false;
    };
    if current_index == 0 {
        return false; // Can't look at previous node if first
    }
    let previous_node = arguments[current_index - 1];
    let line_of_previous_node = file.line_of(file.end(previous_node));
    child_start_line == line_of_previous_node
}

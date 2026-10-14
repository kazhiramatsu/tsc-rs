//! tsgo `format/api.go`: the spans the formatter formats for each request.

use tsc_syntax::SyntaxKind;

use super::file::{decode_rune, is_white_space_single_line, File, Node};
use super::indent::get_indentation_for_node;
use super::span::{
    find_enclosing_node, get_own_or_inherited_delta, get_scan_start_position, ErrorRanges,
    FormatSpanWorker,
};
use super::util::{
    find_immediately_preceding_token_of_kind, find_outermost_node_within_list_level,
    get_line_start_position_for_position,
};
use crate::lsutil::FormatCodeSettings;

/// tsgo `FormatRequestKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FormatRequestKind {
    Document,
    Selection,
    OnEnter,
    OnSemicolon,
    OnOpeningCurlyBrace,
    OnClosingCurlyBrace,
}

/// tsgo `core.TextChange`: the text that replaces a range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TextChange {
    pub(crate) pos: usize,
    pub(crate) end: usize,
    pub(crate) new_text: String,
}

/// tsgo `GetNewLineOrDefaultFromContext` (the host's new line is the
/// settings' own).
fn new_line(options: &FormatCodeSettings) -> &str {
    let new_line = options.editor_settings.new_line_character.as_str();
    if new_line.is_empty() {
        "\n"
    } else {
        new_line
    }
}

/// tsgo `FormatSpan`.
pub(crate) fn format_span(
    file: &File<'_>,
    options: &FormatCodeSettings,
    span: (usize, usize),
    kind: FormatRequestKind,
) -> Vec<TextChange> {
    // find the smallest node that fully wraps the range and compute the initial indentation for the node
    let enclosing_node = find_enclosing_node(file, span);
    FormatSpanWorker::new(
        file,
        options,
        new_line(options),
        get_scan_start_position(file, enclosing_node, span),
        span,
        enclosing_node,
        get_indentation_for_node(file, enclosing_node, Some(span), options),
        get_own_or_inherited_delta(file, enclosing_node, options),
        kind,
        ErrorRanges::new(&file.parse_error_ranges(), span),
    )
    .execute()
}

fn format_node_lines(
    file: &File<'_>,
    options: &FormatCodeSettings,
    node: Option<Node>,
    request_kind: FormatRequestKind,
) -> Vec<TextChange> {
    let Some(node) = node else {
        return Vec::new();
    };
    let token_start = file.token_start(node);
    let line_start = get_line_start_position_for_position(file, token_start);
    format_span(file, options, (line_start, file.end(node)), request_kind)
}

/// tsgo `FormatDocument`.
pub(crate) fn format_document(file: &File<'_>, options: &FormatCodeSettings) -> Vec<TextChange> {
    format_span(
        file,
        options,
        (0, file.end(file.root())),
        FormatRequestKind::Document,
    )
}

/// tsgo `FormatSelection`.
pub(crate) fn format_selection(
    file: &File<'_>,
    options: &FormatCodeSettings,
    start: usize,
    end: usize,
) -> Vec<TextChange> {
    format_span(
        file,
        options,
        (get_line_start_position_for_position(file, start), end),
        FormatRequestKind::Selection,
    )
}

/// tsgo `FormatOnOpeningCurly`: the span ends at the opening curly, so that
/// the brace matched to the one just typed is left alone if it will be the
/// wrong one after further edits. For the method body below typed without
/// brace matching, the closing brace does not move:
///
/// ```text
/// class C {
///     foo()
/// }
/// ```
pub(crate) fn format_on_opening_curly(
    file: &File<'_>,
    options: &FormatCodeSettings,
    position: usize,
) -> Vec<TextChange> {
    let Some(opening_curly) =
        find_immediately_preceding_token_of_kind(file, position, SyntaxKind::OpenBraceToken)
    else {
        return Vec::new();
    };
    let curly_brace_range = file.parent(opening_curly);
    let outermost_node = find_outermost_node_within_list_level(file, curly_brace_range)
        .expect("an opening curly's parent");
    let text_range = (
        get_line_start_position_for_position(file, file.token_start(outermost_node)),
        position,
    );
    format_span(
        file,
        options,
        text_range,
        FormatRequestKind::OnOpeningCurlyBrace,
    )
}

/// tsgo `FormatOnClosingCurly`.
pub(crate) fn format_on_closing_curly(
    file: &File<'_>,
    options: &FormatCodeSettings,
    position: usize,
) -> Vec<TextChange> {
    let preceding_token =
        find_immediately_preceding_token_of_kind(file, position, SyntaxKind::CloseBraceToken);
    format_node_lines(
        file,
        options,
        find_outermost_node_within_list_level(file, preceding_token),
        FormatRequestKind::OnClosingCurlyBrace,
    )
}

/// tsgo `FormatOnSemicolon`.
pub(crate) fn format_on_semicolon(
    file: &File<'_>,
    options: &FormatCodeSettings,
    position: usize,
) -> Vec<TextChange> {
    let semicolon =
        find_immediately_preceding_token_of_kind(file, position, SyntaxKind::SemicolonToken);
    format_node_lines(
        file,
        options,
        find_outermost_node_within_list_level(file, semicolon),
        FormatRequestKind::OnSemicolon,
    )
}

/// tsgo `FormatOnEnter`.
pub(crate) fn format_on_enter(
    file: &File<'_>,
    options: &FormatCodeSettings,
    position: usize,
) -> Vec<TextChange> {
    let line = file.line_of(position);
    if line == 0 {
        return Vec::new();
    }
    let text = file.text();
    // get start position for the previous line
    let start_pos = file.line_starts()[line - 1];
    // After the enter key, the cursor is now at a new line. The new line may or may not contain non-whitespace characters.
    // If the new line has only whitespaces, we won't want to format this line, because that would remove the indentation as
    // trailing whitespaces. So the end of the formatting span should be the later one between:
    //  1. the end of the previous line
    //  2. the last non-whitespace character in the current line
    let mut end_of_format_span = file.end_line_position(line);
    while end_of_format_span > start_pos as isize {
        let (ch, size) = decode_rune(text, end_of_format_span as usize);
        if size == 0 || is_white_space_single_line(ch) {
            // on multibyte character keep backing up
            end_of_format_span -= 1;
            continue;
        }
        break;
    }

    // if the character at the end of the span is a line break, we shouldn't include it, because it indicates we don't want to
    // touch the current line at all. Also, on some OSes the line break consists of two characters (\r\n), we should test if the
    // previous character before the end of format span is line break character as well.
    let (ch, _) = decode_rune(text, end_of_format_span as usize);
    if tsc_syntax::is_line_break(ch) {
        end_of_format_span -= 1;
    }

    // end value is exclusive so add 1 to the result
    let span = (start_pos, (end_of_format_span + 1) as usize);

    format_span(file, options, span, FormatRequestKind::OnEnter)
}

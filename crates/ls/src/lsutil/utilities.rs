//! tsgo `ls/lsutil/utilities.go`.

use tsc_checker::CompilerOptions;
use tsc_emitter::NewLineKind;
use tsc_syntax::{for_each_child, NodeId, SourceFile, SyntaxKind};

use crate::astnav::Navigator;
use crate::lsutil::{
    syntax_requires_trailing_comma_or_semicolon_or_asi, syntax_requires_trailing_semicolon_or_asi,
};
use crate::tree::View;

/// tsgo `compilerOptions.NewLine.GetNewLineCharacter()`: a carriage
/// return and line feed only for `newLine: crlf`.
pub(crate) fn new_line_kind(options: &CompilerOptions) -> NewLineKind {
    match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        _ => NewLineKind::LineFeed,
    }
}

/// tsgo `ProbablyUsesSemicolons`: whether the first statements and
/// members that end with a semicolon or an ASI point mostly end with a
/// semicolon.
pub(crate) fn probably_uses_semicolons(file: &SourceFile) -> bool {
    let navigator = Navigator::new(file);
    const STATEMENTS_TO_OBSERVE: usize = 5;
    let line_of = |position: usize| {
        file.positions()
            .line_of_byte(position.min(file.text().len()) as u32)
            .unwrap_or(0)
    };

    fn visit(
        file: &SourceFile,
        navigator: &Navigator<'_>,
        node: NodeId,
        counts: &mut (usize, usize),
        line_of: &dyn Fn(usize) -> u32,
    ) -> bool {
        let kind = file.arena.node(node).kind;
        if syntax_requires_trailing_semicolon_or_asi(kind) {
            match navigator.last_token(View::Node(node)) {
                Some(token) if navigator.kind(token) == SyntaxKind::SemicolonToken => {
                    counts.0 += 1;
                }
                _ => counts.1 += 1,
            }
        } else if syntax_requires_trailing_comma_or_semicolon_or_asi(kind) {
            match navigator.last_token(View::Node(node)) {
                Some(token) if navigator.kind(token) == SyntaxKind::SemicolonToken => {
                    counts.0 += 1;
                }
                Some(token) if navigator.kind(token) != SyntaxKind::CommaToken => {
                    let last_token_line = line_of(navigator.start_of_node(token, false) as usize);
                    let next_token_line = line_of(tsc_syntax::skip_trivia(
                        file.text(),
                        navigator.end(token) as usize,
                    ));
                    // Avoid counting missing semicolon in single-line objects:
                    // `function f(p: { x: string /*no semicolon here is insignificant*/ }) {`
                    if last_token_line != next_token_line {
                        counts.1 += 1;
                    }
                }
                _ => {}
            }
        }
        if counts.0 + counts.1 >= STATEMENTS_TO_OBSERVE {
            return true;
        }
        let mut stop = false;
        for_each_child(&file.arena, file.arena.node(node), |child| {
            stop = visit(file, navigator, child, counts, line_of);
            stop
        });
        stop
    }

    let mut counts = (0, 0);
    for_each_child(&file.arena, file.arena.node(file.root), |child| {
        visit(file, &navigator, child, &mut counts, &line_of)
    });
    let (with_semicolon, without_semicolon) = counts;

    // One statement missing a semicolon isn't sufficient evidence to say the user
    // doesn't want semicolons, because they may not even be done writing that statement.
    if with_semicolon == 0 && without_semicolon <= 1 {
        return true;
    }
    // When both kinds of observation exist, treat the file as using semicolons when the
    // ratio withSemicolon/withoutSemicolon exceeds 1/nStatementsToObserve (real arithmetic),
    // implemented as an integer inequality to avoid truncation.
    if without_semicolon == 0 {
        return true;
    }
    with_semicolon * STATEMENTS_TO_OBSERVE > without_semicolon
}

/// tsgo `ModuleSpecifierToValidIdentifier`: an identifier from a module
/// specifier's base name (its invalid characters dropped, the letter after
/// each capitalized), `_`-prefixed when empty or a reserved word.
pub(crate) fn module_specifier_to_valid_identifier(
    module_specifier: &str,
    force_capitalize: bool,
) -> String {
    let without_extension = remove_any_file_extension(module_specifier);
    let trimmed = without_extension
        .strip_suffix("/index")
        .unwrap_or(&without_extension);
    let base_name = tsc_program::base_file_name(trimmed)
        .to_string_lossy()
        .into_owned();
    let mut result = String::new();
    let mut chars = base_name.chars();
    let mut last_char_was_valid = true;
    match chars.next() {
        Some(first) if tsc_syntax::is_identifier_start(first) => {
            result.push(if force_capitalize {
                tsc_syntax::go_unicode::to_upper_char(first)
            } else {
                first
            });
        }
        _ => last_char_was_valid = false,
    }
    for ch in chars {
        let is_valid = tsc_syntax::is_identifier_part(ch);
        if is_valid {
            result.push(if last_char_was_valid {
                ch
            } else {
                tsc_syntax::go_unicode::to_upper_char(ch)
            });
        }
        last_char_was_valid = is_valid;
    }
    // Need `"_"` to ensure result isn't empty.
    let reserved = tsc_syntax::keyword_kind(&result)
        .is_some_and(crate::completions::keywords::is_non_contextual_keyword);
    if !result.is_empty() && !reserved {
        return result;
    }
    format!("_{result}")
}

/// tsgo `tspath.RemoveAnyFileExtension`: a known extension, else the base
/// name's last `.`-extension, removed.
fn remove_any_file_extension(path: &str) -> String {
    let removed = tsc_binder::bind::remove_file_extension(path.into())
        .to_string_lossy()
        .into_owned();
    if removed.len() != path.len() {
        return removed;
    }
    let base_start = path.rfind('/').map_or(0, |index| index + 1);
    match path[base_start..].rfind('.') {
        Some(index) => path[..base_start + index].to_owned(),
        None => path.to_owned(),
    }
}

/// tsgo `GetQuotePreference`: the preference, else the quote of the file's
/// first imported module.
pub(crate) fn get_quote_preference(
    file: &tsc_syntax::SourceFile,
    preferences: &super::UserPreferences,
) -> super::QuotePreference {
    use super::QuotePreference;
    let preference = preferences.quote_preference;
    if preference != QuotePreference::UNKNOWN && preference != QuotePreference::AUTO {
        if preference == QuotePreference::SINGLE {
            return QuotePreference::SINGLE;
        }
        return QuotePreference::DOUBLE;
    }
    let first_module_specifier = crate::references::collect_external_module_references(file)
        .imports
        .into_iter()
        .find(|&import| file.arena.node(import).kind == tsc_syntax::SyntaxKind::StringLiteral);
    match first_module_specifier {
        Some(specifier)
            if file.text()[crate::astnav::Navigator::new(file).start_of_node(
                crate::astnav::Found::Node(crate::tree::View::Node(specifier)),
                false,
            ) as usize..]
                .starts_with('\'') =>
        {
            QuotePreference::SINGLE
        }
        _ => QuotePreference::DOUBLE,
    }
}

/// A message with its arguments in the request's locale (tsgo `Localize`).
pub(crate) fn localize(
    catalog: Option<&dyn tsc_diagnostics::MessageCatalog>,
    message: &'static tsc_diagnostics::DiagnosticMessage,
    args: &[&str],
) -> String {
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    tsc_diagnostics::format_message(message.template_in(catalog), &args)
}

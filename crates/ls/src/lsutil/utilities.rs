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

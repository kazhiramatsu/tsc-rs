//! The scanner with trivia (tsgo `SetSkipTrivia(false)`, which the
//! formatter's scanner sets): each token's full start, start, end, kind and
//! whether it is unterminated, as tsgo's scanner returns them for the text.

use tsc_syntax::{parse_source_file, ParseOptions, SyntaxKind, TokenScanner};

type Token = (usize, usize, usize, SyntaxKind, bool);

fn tokens(text: &str) -> Vec<Token> {
    let file = parse_source_file("/a.ts", text, ParseOptions::default(), None);
    let mut scanner = TokenScanner::with_trivia(&file, 0);
    let mut tokens = Vec::new();
    loop {
        let kind = scanner.scan();
        tokens.push((
            scanner.token_full_start(),
            scanner.token_start(),
            scanner.token_end(),
            kind,
            scanner.is_unterminated(),
        ));
        if kind == SyntaxKind::EndOfFileToken {
            return tokens;
        }
    }
}

/// A shebang, a line or paragraph separator and a next line (U+0085) are
/// skipped; `\r\n` is one new line; white space runs over every single-line
/// white space character; an unclosed comment is unterminated.
#[test]
fn white_space_new_lines_and_comments() {
    assert_eq!(
        tokens(
            "#!/usr/bin/env node\nlet a; // c\r\n/* d */\u{2028}b\u{85} \u{a0}c\t\u{b}\u{c}/* e"
        ),
        [
            (0, 19, 20, SyntaxKind::NewLineTrivia, false),
            (20, 20, 23, SyntaxKind::LetKeyword, false),
            (23, 23, 24, SyntaxKind::WhitespaceTrivia, false),
            (24, 24, 25, SyntaxKind::Identifier, false),
            (25, 25, 26, SyntaxKind::SemicolonToken, false),
            (26, 26, 27, SyntaxKind::WhitespaceTrivia, false),
            (27, 27, 31, SyntaxKind::SingleLineCommentTrivia, false),
            (31, 31, 33, SyntaxKind::NewLineTrivia, false),
            (33, 33, 40, SyntaxKind::MultiLineCommentTrivia, false),
            (40, 43, 44, SyntaxKind::Identifier, false),
            (44, 46, 49, SyntaxKind::WhitespaceTrivia, false),
            (49, 49, 50, SyntaxKind::Identifier, false),
            (50, 50, 53, SyntaxKind::WhitespaceTrivia, false),
            (53, 53, 57, SyntaxKind::MultiLineCommentTrivia, true),
            (57, 57, 57, SyntaxKind::EndOfFileToken, false),
        ]
    );
    assert_eq!(
        tokens("\u{85}\u{3000}x\r\r\ny\u{2029} \u{feff}z"),
        [
            (0, 2, 5, SyntaxKind::WhitespaceTrivia, false),
            (5, 5, 6, SyntaxKind::Identifier, false),
            (6, 6, 7, SyntaxKind::NewLineTrivia, false),
            (7, 7, 9, SyntaxKind::NewLineTrivia, false),
            (9, 9, 10, SyntaxKind::Identifier, false),
            (10, 13, 17, SyntaxKind::WhitespaceTrivia, false),
            (17, 17, 18, SyntaxKind::Identifier, false),
            (18, 18, 18, SyntaxKind::EndOfFileToken, false),
        ]
    );
}

/// Merge conflict markers are trivia; a `=======` marker runs to the next
/// `>>>>>>>` one.
#[test]
fn conflict_markers() {
    assert_eq!(
        tokens("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> B\n"),
        [
            (0, 0, 1, SyntaxKind::Identifier, false),
            (1, 1, 2, SyntaxKind::NewLineTrivia, false),
            (2, 2, 14, SyntaxKind::ConflictMarkerTrivia, false),
            (14, 14, 15, SyntaxKind::NewLineTrivia, false),
            (15, 15, 16, SyntaxKind::Identifier, false),
            (16, 16, 17, SyntaxKind::NewLineTrivia, false),
            (17, 17, 27, SyntaxKind::ConflictMarkerTrivia, false),
            (27, 27, 36, SyntaxKind::ConflictMarkerTrivia, false),
            (36, 36, 37, SyntaxKind::NewLineTrivia, false),
            (37, 37, 37, SyntaxKind::EndOfFileToken, false),
        ]
    );
}

/// A `#!` after the start of the file is an unknown token.
#[test]
fn shebang_after_the_start() {
    assert_eq!(
        tokens(" #!x /**/ //"),
        [
            (0, 0, 1, SyntaxKind::WhitespaceTrivia, false),
            (1, 1, 3, SyntaxKind::Unknown, false),
            (3, 3, 4, SyntaxKind::Identifier, false),
            (4, 4, 5, SyntaxKind::WhitespaceTrivia, false),
            (5, 5, 9, SyntaxKind::MultiLineCommentTrivia, false),
            (9, 9, 10, SyntaxKind::WhitespaceTrivia, false),
            (10, 10, 12, SyntaxKind::SingleLineCommentTrivia, false),
            (12, 12, 12, SyntaxKind::EndOfFileToken, false),
        ]
    );
}

/// tsgo `GetShebang`: the first line when it starts with `#!`, without its
/// line break.
#[test]
fn shebang_line() {
    assert_eq!(
        tsc_syntax::get_shebang("#!/usr/bin/env node\nx"),
        "#!/usr/bin/env node"
    );
    assert_eq!(tsc_syntax::get_shebang("#!node\r\nx"), "#!node");
    assert_eq!(tsc_syntax::get_shebang("#!node"), "#!node");
    assert_eq!(tsc_syntax::get_shebang(" #!node"), "");
    assert_eq!(tsc_syntax::get_shebang(""), "");
}

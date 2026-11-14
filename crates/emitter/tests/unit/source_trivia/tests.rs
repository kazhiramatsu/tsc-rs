use super::skip_source_trivia;

#[test]
fn ordinary_tokens_and_ascii_trivia_keep_their_language_boundaries() {
    for (text, start, expected) in [
        ("", 0, 0),
        ("const x", 0, 0),
        ("   \tconst x", 0, 4),
        ("\r\n \u{000b}\u{000c}const", 0, 5),
        ("x \t\r\n", 1, 5),
        ("  #!not-a-shebang", 0, 2),
        ("value/* comment */", 0, 0),
        ("value/* comment */", 5, 18),
        ("/* unterminated", 0, 15),
    ] {
        assert_eq!(
            skip_source_trivia(text, start),
            expected,
            "{text:?} at {start}"
        );
    }
}

#[test]
fn unicode_comments_shebangs_and_conflicts_keep_the_scanners_boundaries() {
    for text in [
        "#! /usr/bin/env node\n// lead\nconst x = 1;",
        "/* a */ \n // b\r\nconst x = 1;",
        "\u{feff}\u{2003}\u{2028}\u{2029}\tconst café = '😀';",
        "// a\u{2028}const x = 1;",
        "/* unterminated",
        "<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n",
        "||||||| base\nx\n=======\ny\n>>>>>>> theirs\n",
        "a /= b; x < y; x === y; x || y; #private;",
    ] {
        for start in text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain([text.len()])
        {
            assert_eq!(
                skip_source_trivia(text, start),
                tsc_syntax::skip_trivia(text, start),
                "{text:?} at {start}"
            );
        }
    }
}

#[test]
fn invalid_byte_cursors_still_reach_the_scanners_validation() {
    let text = "é😀";
    for start in [1, 3, text.len() + 1, usize::MAX] {
        assert!(std::panic::catch_unwind(|| skip_source_trivia(text, start)).is_err());
        assert!(std::panic::catch_unwind(|| tsc_syntax::skip_trivia(text, start)).is_err());
    }
}

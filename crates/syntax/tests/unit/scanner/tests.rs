use super::*;

#[test]
fn identifier_tables_follow_script_target_for_bmp_and_supplementary_code_points() {
    // U+08A1 is an ESNext identifier start/part but is absent from
    // the ES5 tables. U+10400 proves the same selection above the
    // BMP, where Rust still advances one scalar but diagnostics are
    // later projected to two UTF-16 code units.
    for text in ["\u{08a1}", "\u{10400}"] {
        let mut es5 = Scanner::new_with_target(text, LanguageVariant::Standard, ScriptTarget::ES5);
        assert_eq!(es5.scan(), SyntaxKind::Unknown, "{text:?}");
        assert_eq!(es5.errors().len(), 1, "{text:?}");

        let mut es2015 =
            Scanner::new_with_target(text, LanguageVariant::Standard, ScriptTarget::ES2015);
        assert_eq!(es2015.scan(), SyntaxKind::Identifier, "{text:?}");
        assert_eq!(es2015.pos(), text.len(), "{text:?}");
        assert!(es2015.errors().is_empty(), "{text:?}");

        let with_prefix = format!("a{text}");
        let mut es5_part =
            Scanner::new_with_target(&with_prefix, LanguageVariant::Standard, ScriptTarget::ES5);
        assert_eq!(es5_part.scan(), SyntaxKind::Identifier, "{text:?}");
        assert_eq!(es5_part.pos(), 1, "{text:?}");

        let mut es2015_part = Scanner::new_with_target(
            &with_prefix,
            LanguageVariant::Standard,
            ScriptTarget::ES2015,
        );
        assert_eq!(es2015_part.scan(), SyntaxKind::Identifier, "{text:?}");
        assert_eq!(es2015_part.pos(), with_prefix.len(), "{text:?}");
    }
}

#[test]
fn comment_only_input_has_no_tokens() {
    assert_eq!(
        scan_tokens("// line\n/* block */\n", LanguageVariant::Standard),
        Vec::new()
    );
}

#[test]
fn token_after_trivia_gets_preceding_line_break_flag() {
    assert_eq!(
        scan_tokens("// line\nx", LanguageVariant::Standard),
        vec![TokenRecord {
            kind: SyntaxKind::Identifier,
            start: 8,
            end: 9,
            preceding_line_break: true,
        }]
    );
}

#[test]
fn byte_token_stream_is_lazy_and_uses_parser_positions() {
    let text = "/* 😀 */\nconst 才能 = 1;";
    let mut tokens = scan_byte_tokens(text, LanguageVariant::Standard);

    let first = tokens.next().expect("const token");
    assert_eq!(first.kind, SyntaxKind::ConstKeyword);
    assert_eq!(first.start as usize, text.find("const").unwrap());
    assert_eq!(first.end as usize, text.find("const").unwrap() + 5);
    assert!(first.preceding_line_break);

    let identifier = tokens.next().expect("identifier token");
    assert_eq!(identifier.kind, SyntaxKind::Identifier);
    assert_eq!(
        &text[identifier.start as usize..identifier.end as usize],
        "才能"
    );
    assert_eq!(tokens.count(), 3);
}

#[test]
fn jsdoc_scanner_preserves_comment_text_and_tag_boundaries() {
    let text = "hello mail@host {@link X} @foo-bar";
    let mut scanner = Scanner::new(text, LanguageVariant::Standard);

    assert_eq!(
        scanner.scan_jsdoc_comment_text_token(false),
        SyntaxKind::JSDocCommentTextToken
    );
    assert_eq!(scanner.token_value(), "hello mail@host ");
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::OpenBraceToken);
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::AtToken);
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::Identifier);
    assert_eq!(scanner.token_value(), "link");
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::WhitespaceTrivia);
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::Identifier);
    assert_eq!(scanner.token_value(), "X");
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::CloseBraceToken);
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::WhitespaceTrivia);
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::AtToken);
    assert_eq!(scanner.scan_jsdoc_token(), SyntaxKind::Identifier);
    assert_eq!(scanner.token_value(), "foo-bar");
}

#[test]
fn jsdoc_type_mode_skips_one_leading_asterisk_per_line() {
    let text = "{Foo\n * | Bar}";
    let mut scanner = Scanner::new(text, LanguageVariant::Standard);
    scanner.set_skip_jsdoc_leading_asterisks(true);

    assert_eq!(scanner.scan(), SyntaxKind::OpenBraceToken);
    assert_eq!(scanner.scan(), SyntaxKind::Identifier);
    assert_eq!(scanner.token_value(), "Foo");
    assert_eq!(scanner.scan(), SyntaxKind::BarToken);
    assert_eq!(scanner.scan(), SyntaxKind::Identifier);
    assert_eq!(scanner.token_value(), "Bar");
    assert_eq!(scanner.scan(), SyntaxKind::CloseBraceToken);

    scanner.set_skip_jsdoc_leading_asterisks(false);
}

#[test]
fn shebang_at_start_is_trivia() {
    assert_eq!(
        scan_tokens("#!/usr/bin/env node\n", LanguageVariant::Standard),
        Vec::new()
    );
}

#[test]
fn dump_positions_are_utf16_offsets() {
    assert_eq!(
        scan_tokens("/* \u{1f600} */x", LanguageVariant::Standard),
        vec![TokenRecord {
            kind: SyntaxKind::Identifier,
            start: 8,
            end: 9,
            preceding_line_break: false,
        }]
    );
}

#[test]
fn unterminated_block_comment_reports_1010() {
    let mut scanner = Scanner::new("/* unterminated", LanguageVariant::Standard);

    assert_eq!(scanner.scan(), SyntaxKind::EndOfFileToken);

    // tsc pins: the error sits at the end of text, zero width.
    assert_eq!(scanner.errors().len(), 1);
    assert_eq!(scanner.errors()[0].message.code, 1010);
    assert_eq!(scanner.errors()[0].start, "/* unterminated".len());
    assert_eq!(scanner.errors()[0].length, 0);
}

#[test]
fn save_restore_rewinds_position_and_errors() {
    let mut scanner = Scanner::new("/* unterminated", LanguageVariant::Standard);
    let saved = scanner.save();

    assert_eq!(scanner.scan(), SyntaxKind::EndOfFileToken);
    assert_eq!(scanner.errors().len(), 1);

    scanner.restore(saved);

    assert_eq!(scanner.pos(), 0);
    assert_eq!(scanner.errors(), &[]);
}

#[test]
fn ordinary_token_lookahead_can_cross_a_scan_range_boundary() {
    let mut scanner = Scanner::new("**/", LanguageVariant::Standard);
    scanner.reset_range(0, 1);

    // tsc uses unchecked punctuation lookahead after checking only the
    // first code unit against scanRange's end.
    assert_eq!(scanner.scan(), SyntaxKind::AsteriskAsteriskToken);
    assert_eq!(scanner.pos(), 2);
}

#[test]
fn look_ahead_always_rewinds_after_truthy_result() {
    let mut scanner = Scanner::new("a b", LanguageVariant::Standard);

    let result = scanner.look_ahead(|scanner| {
        assert_eq!(scanner.scan(), SyntaxKind::Identifier);
        Some(scanner.pos())
    });

    assert_eq!(result, Some(1));
    assert_eq!(scanner.pos(), 0);
    assert_eq!(scanner.token, SyntaxKind::Unknown);
}

#[test]
fn try_scan_commits_truthy_and_rewinds_falsy() {
    let mut scanner = Scanner::new("a b", LanguageVariant::Standard);

    let result = scanner.try_scan(|scanner| scanner.scan());

    assert_eq!(result, SyntaxKind::Identifier);
    assert_eq!(scanner.pos(), 1);
    assert_eq!(scanner.token, SyntaxKind::Identifier);

    let result = scanner.try_scan(|scanner| {
        assert_eq!(scanner.scan(), SyntaxKind::Identifier);
        false
    });

    assert!(!result);
    assert_eq!(scanner.pos(), 1);
    assert_eq!(scanner.token, SyntaxKind::Identifier);
}

#[test]
fn speculation_restores_nested_state_and_errors() {
    let mut scanner = Scanner::new("a \"\\xG\"", LanguageVariant::Standard);

    let result = scanner.look_ahead(|scanner| {
        assert_eq!(scanner.scan(), SyntaxKind::Identifier);
        let inner = scanner.try_scan(|scanner| {
            assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);
            assert_eq!(scanner.errors().len(), 1);
            true
        });
        assert!(inner);
        assert_eq!(scanner.pos(), "a \"\\xG\"".len());
        assert_eq!(scanner.errors().len(), 1);
        true
    });

    assert!(result);
    assert_eq!(scanner.pos(), 0);
    assert_eq!(scanner.token, SyntaxKind::Unknown);
    assert!(scanner.errors().is_empty());
}

fn directives_of(text: &str) -> Vec<CommentDirective> {
    let mut scanner = Scanner::new(text, LanguageVariant::Standard);
    while scanner.scan() != SyntaxKind::EndOfFileToken {}
    scanner.take_comment_directives()
}

#[test]
fn single_line_comment_directives_are_collected() {
    let text = "// @ts-ignore\nlet x;\n/// @ts-expect-error\nlet y;\n";
    assert_eq!(
        directives_of(text),
        vec![
            CommentDirective {
                pos: 0,
                end: "// @ts-ignore".len() as u32,
                kind: CommentDirectiveKind::Ignore,
            },
            CommentDirective {
                pos: text.find("///").unwrap() as u32,
                end: text.find("///").unwrap() as u32 + "/// @ts-expect-error".len() as u32,
                kind: CommentDirectiveKind::ExpectError,
            },
        ]
    );
}

#[test]
fn trailing_single_line_comment_is_a_directive() {
    // The comment slice starts at `//` regardless of what precedes
    // it on the line.
    assert_eq!(directives_of("let a = 1; // @ts-ignore\nlet x;\n").len(), 1);
}

#[test]
fn four_slashes_are_not_a_directive() {
    // ^\/\/\/?  allows at most three slashes before the pragma.
    assert_eq!(directives_of("////@ts-ignore\n"), Vec::new());
    assert_eq!(directives_of("///@ts-ignore\n").len(), 1);
}

#[test]
fn directive_name_has_no_word_boundary() {
    // tsc's regex quirk: `@ts-ignored` matches as `@ts-ignore`.
    assert_eq!(
        directives_of("// @ts-ignored\n")[0].kind,
        CommentDirectiveKind::Ignore
    );
}

#[test]
fn multi_line_directive_matches_only_the_last_line() {
    // Interior lines never match, whatever they contain.
    assert_eq!(
        directives_of("/*\n@ts-ignore\nrest\n*/\nlet x;\n"),
        Vec::new()
    );
    assert_eq!(directives_of("/*\n@ts-ignore\n*/\nlet x;\n"), Vec::new());

    // One-liner: the last line IS the whole comment.
    let one_liner = "/* @ts-ignore */\nlet x;\n";
    assert_eq!(
        directives_of(one_liner),
        vec![CommentDirective {
            pos: 0,
            end: "/* @ts-ignore */".len() as u32,
            kind: CommentDirectiveKind::Ignore,
        }]
    );

    // Closing-line directive: pos is the LAST line's start, end is
    // one past `*/`; leading whitespace and a star shell are
    // allowed by ^(?:\/|\*)*\s* after trimStart.
    let closing = "/* leading\n * @ts-expect-error */\nlet x;\n";
    let last_line = closing.find(" * @").unwrap();
    assert_eq!(
        directives_of(closing),
        vec![CommentDirective {
            pos: last_line as u32,
            end: closing.find("*/").unwrap() as u32 + 2,
            kind: CommentDirectiveKind::ExpectError,
        }]
    );

    // A bare space between star-shell runs breaks the match.
    assert_eq!(directives_of("/*\n * * @ts-ignore */\n"), Vec::new());
}

#[test]
fn unterminated_multi_line_comment_still_appends_its_directive() {
    // tsc appends before the unterminated-comment error.
    let text = "/* @ts-ignore";
    assert_eq!(
        directives_of(text),
        vec![CommentDirective {
            pos: 0,
            end: text.len() as u32,
            kind: CommentDirectiveKind::Ignore,
        }]
    );
}

#[test]
fn template_literal_interior_is_not_a_directive() {
    assert_eq!(
        directives_of("const s = `\n// @ts-ignore\n`;\n"),
        Vec::new()
    );
}

#[test]
fn js_whitespace_diverges_from_scanner_whitespace_where_tsc_does() {
    // U+0085 / U+200B: scanner trivia, not regex \s.
    assert!(is_single_line_whitespace('\u{0085}'));
    assert!(!is_js_whitespace('\u{0085}'));
    assert!(is_single_line_whitespace('\u{200B}'));
    assert!(!is_js_whitespace('\u{200B}'));
    // U+FEFF is in both.
    assert!(is_js_whitespace('\u{FEFF}'));
    assert_eq!(directives_of("//\u{0085}@ts-ignore\n"), Vec::new());
    assert_eq!(directives_of("//\u{FEFF}@ts-ignore\n").len(), 1);
}

#[test]
fn scans_keywords_and_punctuation() {
    let tokens = scan_tokens(
        "class C { async m() { return x?.y ?? 1; } }",
        LanguageVariant::Standard,
    )
    .into_iter()
    .map(|token| token.kind)
    .collect::<Vec<_>>();

    assert_eq!(
        tokens,
        vec![
            SyntaxKind::ClassKeyword,
            SyntaxKind::Identifier,
            SyntaxKind::OpenBraceToken,
            SyntaxKind::AsyncKeyword,
            SyntaxKind::Identifier,
            SyntaxKind::OpenParenToken,
            SyntaxKind::CloseParenToken,
            SyntaxKind::OpenBraceToken,
            SyntaxKind::ReturnKeyword,
            SyntaxKind::Identifier,
            SyntaxKind::QuestionDotToken,
            SyntaxKind::Identifier,
            SyntaxKind::QuestionQuestionToken,
            SyntaxKind::NumericLiteral,
            SyntaxKind::SemicolonToken,
            SyntaxKind::CloseBraceToken,
            SyntaxKind::CloseBraceToken,
        ]
    );
}

#[test]
fn greater_than_compounds_wait_for_rescan() {
    let tokens = scan_tokens("a >= b >> c >>>= d", LanguageVariant::Standard)
        .into_iter()
        .map(|token| token.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        tokens,
        vec![
            SyntaxKind::Identifier,
            SyntaxKind::GreaterThanToken,
            SyntaxKind::EqualsToken,
            SyntaxKind::Identifier,
            SyntaxKind::GreaterThanToken,
            SyntaxKind::GreaterThanToken,
            SyntaxKind::Identifier,
            SyntaxKind::GreaterThanToken,
            SyntaxKind::GreaterThanToken,
            SyntaxKind::GreaterThanToken,
            SyntaxKind::EqualsToken,
            SyntaxKind::Identifier,
        ]
    );
}

#[test]
fn unicode_identifier_ranges_match_tsc_table() {
    let tokens = scan_tokens("var 才能ソЫ = 1;", LanguageVariant::Standard)
        .into_iter()
        .map(|token| token.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        tokens,
        vec![
            SyntaxKind::VarKeyword,
            SyntaxKind::Identifier,
            SyntaxKind::EqualsToken,
            SyntaxKind::NumericLiteral,
            SyntaxKind::SemicolonToken,
        ]
    );
}

#[test]
fn string_escape_sequences_set_value_and_flags() {
    let mut scanner = Scanner::new("\"\\n\\t\\x41\\u0042\\u{43}\"", LanguageVariant::Standard);

    assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);

    assert_eq!(scanner.token_value, "\n\tABC");
    assert!(scanner.token_flags.contains(TokenFlags::HEX_ESCAPE));
    assert!(scanner.token_flags.contains(TokenFlags::UNICODE_ESCAPE));
    assert!(scanner
        .token_flags
        .contains(TokenFlags::EXTENDED_UNICODE_ESCAPE));
    assert!(scanner.errors().is_empty());
}

#[test]
fn invalid_extended_unicode_escape_reports_1198() {
    let mut scanner = Scanner::new("\"\\u{110000}\"", LanguageVariant::Standard);

    assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);

    assert!(scanner
        .token_flags
        .contains(TokenFlags::CONTAINS_INVALID_ESCAPE));
    assert_eq!(
        scanner
            .errors()
            .iter()
            .map(|error| error.message.code)
            .collect::<Vec<_>>(),
        vec![1198]
    );
}

#[test]
fn unterminated_string_reports_1002() {
    let mut scanner = Scanner::new("\"abc", LanguageVariant::Standard);

    assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);

    assert!(scanner.token_flags.contains(TokenFlags::UNTERMINATED));
    assert_eq!(scanner.errors().len(), 1);
    assert_eq!(scanner.errors()[0].message.code, 1002);
}

#[test]
fn string_escape_oracle_pins() {
    struct Case {
        text: &'static str,
        value: &'static str,
        flags: u32,
        errors: &'static [(usize, usize, u32)],
    }

    let cases = [
        Case {
            text: "\"\\n\"",
            value: "\n",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\t\"",
            value: "\t",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\b\"",
            value: "\u{0008}",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\v\"",
            value: "\u{000b}",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\f\"",
            value: "\u{000c}",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\r\"",
            value: "\r",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\'\"",
            value: "'",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "'\\\"'",
            value: "\"",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\0\"",
            value: "\0",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\x41\"",
            value: "A",
            flags: 4096,
            errors: &[],
        },
        Case {
            text: "\"\\u0042\"",
            value: "B",
            flags: 1024,
            errors: &[],
        },
        Case {
            text: "\"\\u{43}\"",
            value: "C",
            flags: 8,
            errors: &[],
        },
        Case {
            text: "\"a\\\nb\"",
            value: "ab",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"a\\\r\nb\"",
            value: "ab",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "\"\\xG\"",
            value: "\\xG",
            flags: 2048,
            errors: &[(3, 0, 1125)],
        },
        Case {
            text: "\"\\u00G0\"",
            value: "\\u00G0",
            flags: 2048,
            errors: &[(5, 0, 1125)],
        },
        Case {
            text: "\"\\u{}\"",
            value: "\\u{}",
            flags: 2048,
            errors: &[(4, 0, 1125)],
        },
        Case {
            text: "\"\\u{110000}\"",
            value: "\\u{110000}",
            flags: 2048,
            errors: &[(4, 6, 1198)],
        },
        Case {
            text: "\"\\u{41\"",
            value: "\\u{41",
            flags: 2048,
            errors: &[(6, 0, 1199)],
        },
        Case {
            text: "\"\\8\"",
            value: "8",
            flags: 2048,
            errors: &[(1, 2, 1488)],
        },
        Case {
            text: "\"\\123\"",
            value: "S",
            flags: 2048,
            errors: &[(1, 4, 1487)],
        },
        Case {
            text: "\"abc",
            value: "abc",
            flags: 4,
            errors: &[(4, 0, 1002)],
        },
    ];

    for case in cases {
        let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::StringLiteral, "{}", case.text);
        assert_eq!(scanner.token_value, case.value, "{}", case.text);
        assert_eq!(scanner.token_flags.0, case.flags, "{}", case.text);
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| (error.start, error.length, error.message.code))
                .collect::<Vec<_>>(),
            case.errors,
            "{}",
            case.text
        );
    }
}

#[test]
fn numeric_literal_oracle_pins() {
    struct Case {
        text: &'static str,
        kind: SyntaxKind,
        end: usize,
        value: &'static str,
        flags: u32,
        errors: &'static [(usize, usize, u32)],
    }

    let cases = [
        Case {
            text: "1_2",
            kind: SyntaxKind::NumericLiteral,
            end: 3,
            value: "12",
            flags: 512,
            errors: &[],
        },
        Case {
            text: "1__2",
            kind: SyntaxKind::NumericLiteral,
            end: 4,
            value: "12",
            flags: 16896,
            errors: &[(2, 1, 6189)],
        },
        Case {
            text: "1_",
            kind: SyntaxKind::NumericLiteral,
            end: 2,
            value: "1",
            flags: 16896,
            errors: &[(1, 1, 6188)],
        },
        Case {
            text: "0_1",
            kind: SyntaxKind::NumericLiteral,
            end: 3,
            value: "1",
            flags: 16896,
            errors: &[(1, 1, 6188)],
        },
        Case {
            text: "01",
            kind: SyntaxKind::NumericLiteral,
            end: 2,
            value: "1",
            flags: 32,
            errors: &[(0, 2, 1121)],
        },
        Case {
            text: "08",
            kind: SyntaxKind::NumericLiteral,
            end: 2,
            value: "8",
            flags: 8192,
            errors: &[(0, 2, 1489)],
        },
        Case {
            text: "1e2",
            kind: SyntaxKind::NumericLiteral,
            end: 3,
            value: "100",
            flags: 16,
            errors: &[],
        },
        Case {
            text: "1e+n",
            kind: SyntaxKind::NumericLiteral,
            end: 4,
            value: "1",
            flags: 16,
            errors: &[(3, 0, 1124), (0, 4, 1352)],
        },
        Case {
            text: "1.0n",
            kind: SyntaxKind::NumericLiteral,
            end: 4,
            value: "1",
            flags: 0,
            errors: &[(0, 4, 1353)],
        },
        Case {
            text: "1n",
            kind: SyntaxKind::BigIntLiteral,
            end: 2,
            value: "1n",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "0xAFn",
            kind: SyntaxKind::BigIntLiteral,
            end: 5,
            value: "0xafn",
            flags: 64,
            errors: &[],
        },
        Case {
            text: "0x_f",
            kind: SyntaxKind::NumericLiteral,
            end: 4,
            value: "15",
            flags: 576,
            errors: &[(2, 1, 6188)],
        },
        Case {
            text: "0x",
            kind: SyntaxKind::NumericLiteral,
            end: 1,
            value: "0",
            flags: 0,
            errors: &[(1, 1, 1351)],
        },
        Case {
            text: "0b101n",
            kind: SyntaxKind::BigIntLiteral,
            end: 6,
            value: "5n",
            flags: 128,
            errors: &[],
        },
        Case {
            text: "0b_",
            kind: SyntaxKind::NumericLiteral,
            end: 3,
            value: "0",
            flags: 640,
            errors: &[(2, 1, 6188), (2, 1, 6188), (3, 0, 1177)],
        },
        Case {
            text: "0o77n",
            kind: SyntaxKind::BigIntLiteral,
            end: 5,
            value: "63n",
            flags: 256,
            errors: &[],
        },
        Case {
            text: ".5",
            kind: SyntaxKind::NumericLiteral,
            end: 2,
            value: "0.5",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "00.1",
            kind: SyntaxKind::NumericLiteral,
            end: 2,
            value: "0",
            flags: 32,
            errors: &[(0, 2, 1121)],
        },
    ];

    for case in cases {
        let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

        assert_eq!(scanner.scan(), case.kind, "{}", case.text);
        assert_eq!(scanner.pos(), case.end, "{}", case.text);
        assert_eq!(scanner.token_value, case.value, "{}", case.text);
        assert_eq!(scanner.token_flags.0, case.flags, "{}", case.text);
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| (error.start, error.length, error.message.code))
                .collect::<Vec<_>>(),
            case.errors,
            "{}",
            case.text
        );
    }
}

#[test]
fn legacy_octal_after_minus_reports_from_minus() {
    let mut scanner = Scanner::new("-01", LanguageVariant::Standard);

    assert_eq!(scanner.scan(), SyntaxKind::MinusToken);
    assert_eq!(scanner.scan(), SyntaxKind::NumericLiteral);

    assert_eq!(scanner.token_value, "1");
    assert_eq!(scanner.token_flags.0, 32);
    assert_eq!(scanner.errors().len(), 1);
    assert_eq!(scanner.errors()[0].start, 0);
    assert_eq!(scanner.errors()[0].length, 3);
    assert_eq!(scanner.errors()[0].message.code, 1121);
    assert_eq!(scanner.errors()[0].args, vec!["-0o1".to_owned()]);
}

#[test]
fn template_literal_oracle_pins() {
    struct Case {
        text: &'static str,
        kind: SyntaxKind,
        end: usize,
        value: &'static str,
        flags: u32,
        errors: &'static [(usize, usize, u32)],
    }

    let cases = [
        Case {
            text: "`a`",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 3,
            value: "a",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "`a${b}`",
            kind: SyntaxKind::TemplateHead,
            end: 4,
            value: "a",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "`a\\nb`",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 6,
            value: "a\nb",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "`a\r\nb`",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 6,
            value: "a\nb",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "`a\rb`",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 5,
            value: "a\nb",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "`\\xG`",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 5,
            value: "\\xG",
            flags: 2048,
            errors: &[],
        },
        Case {
            text: "`\\u{110000}`",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 12,
            value: "\\u{110000}",
            flags: 2048,
            errors: &[],
        },
        Case {
            text: "`abc",
            kind: SyntaxKind::NoSubstitutionTemplateLiteral,
            end: 4,
            value: "abc",
            flags: 4,
            errors: &[(4, 0, 1160)],
        },
    ];

    for case in cases {
        let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

        assert_eq!(scanner.scan(), case.kind, "{}", case.text);
        assert_eq!(scanner.pos(), case.end, "{}", case.text);
        assert_eq!(scanner.token_value, case.value, "{}", case.text);
        assert_eq!(scanner.token_flags.0, case.flags, "{}", case.text);
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| (error.start, error.length, error.message.code))
                .collect::<Vec<_>>(),
            case.errors,
            "{}",
            case.text
        );
    }
}

#[test]
fn rescan_template_token_oracle_pins() {
    struct Case {
        text: &'static str,
        is_tagged_template: bool,
        kind: SyntaxKind,
        end: usize,
        value: &'static str,
        flags: u32,
        errors: &'static [(usize, usize, u32)],
    }

    let cases = [
        Case {
            text: "}tail`",
            is_tagged_template: false,
            kind: SyntaxKind::TemplateTail,
            end: 6,
            value: "tail",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "}mid${x}`",
            is_tagged_template: false,
            kind: SyntaxKind::TemplateMiddle,
            end: 6,
            value: "mid",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "}\\xG`",
            is_tagged_template: false,
            kind: SyntaxKind::TemplateTail,
            end: 5,
            value: "\\xG",
            flags: 2048,
            errors: &[(3, 0, 1125)],
        },
        Case {
            text: "}\\u{110000}`",
            is_tagged_template: false,
            kind: SyntaxKind::TemplateTail,
            end: 12,
            value: "\\u{110000}",
            flags: 2048,
            errors: &[(4, 6, 1198)],
        },
        Case {
            text: "}\\xG`",
            is_tagged_template: true,
            kind: SyntaxKind::TemplateTail,
            end: 5,
            value: "\\xG",
            flags: 2048,
            errors: &[],
        },
    ];

    for case in cases {
        let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::CloseBraceToken, "{}", case.text);
        assert_eq!(
            scanner.re_scan_template_token(case.is_tagged_template),
            case.kind,
            "{}",
            case.text
        );
        assert_eq!(scanner.pos(), case.end, "{}", case.text);
        assert_eq!(scanner.token_value, case.value, "{}", case.text);
        assert_eq!(scanner.token_flags.0, case.flags, "{}", case.text);
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| (error.start, error.length, error.message.code))
                .collect::<Vec<_>>(),
            case.errors,
            "{}",
            case.text
        );
    }
}

#[test]
fn rescan_greater_less_and_hash_oracle_pins() {
    let cases = [
        (">=", SyntaxKind::GreaterThanEqualsToken, 2),
        (">>=", SyntaxKind::GreaterThanGreaterThanEqualsToken, 3),
        (">>>", SyntaxKind::GreaterThanGreaterThanGreaterThanToken, 3),
        (
            ">>>=",
            SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken,
            4,
        ),
    ];

    for (text, expected_kind, expected_end) in cases {
        let mut scanner = Scanner::new(text, LanguageVariant::Standard);
        assert_eq!(scanner.scan(), SyntaxKind::GreaterThanToken, "{text}");
        assert_eq!(scanner.re_scan_greater_token(), expected_kind, "{text}");
        assert_eq!(scanner.pos(), expected_end, "{text}");
    }

    let mut scanner = Scanner::new("<<", LanguageVariant::Standard);
    assert_eq!(scanner.scan(), SyntaxKind::LessThanLessThanToken);
    assert_eq!(scanner.re_scan_less_than_token(), SyntaxKind::LessThanToken);
    assert_eq!(scanner.pos(), 1);

    let mut scanner = Scanner::new("#x", LanguageVariant::Standard);
    assert_eq!(scanner.scan(), SyntaxKind::PrivateIdentifier);
    assert_eq!(scanner.re_scan_hash_token(), SyntaxKind::HashToken);
    assert_eq!(scanner.pos(), 1);
}

#[test]
fn rescan_slash_regex_extent_oracle_pins() {
    struct Case {
        text: &'static str,
        first: SyntaxKind,
        end: usize,
        value: &'static str,
        flags: u32,
        errors: &'static [(usize, usize, u32)],
    }

    let cases = [
        Case {
            text: "/abc/g",
            first: SyntaxKind::SlashToken,
            end: 6,
            value: "/abc/g",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "/a[b\\/]c/i",
            first: SyntaxKind::SlashToken,
            end: 10,
            value: "/a[b\\/]c/i",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "/=x/",
            first: SyntaxKind::SlashEqualsToken,
            end: 4,
            value: "/=x/",
            flags: 0,
            errors: &[],
        },
        Case {
            text: "/unterminated",
            first: SyntaxKind::SlashToken,
            end: 13,
            value: "/unterminated",
            flags: 4,
            errors: &[(0, 13, 1161)],
        },
        Case {
            text: "/abc\nnext",
            first: SyntaxKind::SlashToken,
            end: 4,
            value: "/abc",
            flags: 4,
            errors: &[(0, 4, 1161)],
        },
    ];

    for case in cases {
        let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

        assert_eq!(scanner.scan(), case.first, "{}", case.text);
        assert_eq!(
            scanner.re_scan_slash_token(false),
            SyntaxKind::RegularExpressionLiteral,
            "{}",
            case.text
        );
        assert_eq!(scanner.pos(), case.end, "{}", case.text);
        assert_eq!(scanner.token_value, case.value, "{}", case.text);
        assert_eq!(scanner.token_flags.0, case.flags, "{}", case.text);
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| (error.start, error.length, error.message.code))
                .collect::<Vec<_>>(),
            case.errors,
            "{}",
            case.text
        );
    }
}

#[test]
fn jsx_scanner_oracle_pins() {
    struct JsxCase {
        text: &'static str,
        kind: SyntaxKind,
        end: usize,
        value: &'static str,
        errors: &'static [(usize, usize, u32)],
    }

    let cases = [
        JsxCase {
            text: "<div",
            kind: SyntaxKind::LessThanToken,
            end: 1,
            value: "",
            errors: &[],
        },
        JsxCase {
            text: "</div",
            kind: SyntaxKind::LessThanSlashToken,
            end: 2,
            value: "",
            errors: &[],
        },
        JsxCase {
            text: "hello {x}",
            kind: SyntaxKind::JsxText,
            end: 6,
            value: "hello ",
            errors: &[],
        },
        JsxCase {
            text: "  \n  <x",
            kind: SyntaxKind::JsxTextAllWhiteSpaces,
            end: 5,
            value: "  \n  ",
            errors: &[],
        },
        JsxCase {
            text: "a>b",
            kind: SyntaxKind::JsxText,
            end: 3,
            value: "a>b",
            errors: &[(1, 1, 1382)],
        },
        JsxCase {
            text: "a}b",
            kind: SyntaxKind::JsxText,
            end: 3,
            value: "a}b",
            errors: &[(1, 1, 1381)],
        },
    ];

    for case in cases {
        let mut scanner = Scanner::new(case.text, LanguageVariant::Jsx);

        assert_eq!(scanner.scan_jsx_token(true), case.kind, "{}", case.text);
        assert_eq!(scanner.pos(), case.end, "{}", case.text);
        assert_eq!(scanner.token_value, case.value, "{}", case.text);
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| (error.start, error.length, error.message.code))
                .collect::<Vec<_>>(),
            case.errors,
            "{}",
            case.text
        );
    }
}

#[test]
fn jsx_identifier_and_attribute_value_oracle_pins() {
    let mut scanner = Scanner::new("foo-bar", LanguageVariant::Jsx);
    assert_eq!(scanner.scan(), SyntaxKind::Identifier);
    assert_eq!(scanner.scan_jsx_identifier(), SyntaxKind::Identifier);
    assert_eq!(scanner.pos(), 7);
    assert_eq!(scanner.token_value, "foo-bar");

    let mut scanner = Scanner::new("class-name", LanguageVariant::Jsx);
    assert_eq!(scanner.scan(), SyntaxKind::ClassKeyword);
    assert_eq!(scanner.scan_jsx_identifier(), SyntaxKind::Identifier);
    assert_eq!(scanner.pos(), 10);
    assert_eq!(scanner.token_value, "class-name");

    let mut scanner = Scanner::new("\"a\\nb\"", LanguageVariant::Jsx);
    assert_eq!(
        scanner.scan_jsx_attribute_value(),
        SyntaxKind::StringLiteral
    );
    assert_eq!(scanner.pos(), 6);
    assert_eq!(scanner.token_value, "a\\nb");
    assert!(scanner.errors().is_empty());

    let mut scanner = Scanner::new("\"a\nb\"", LanguageVariant::Jsx);
    assert_eq!(
        scanner.scan_jsx_attribute_value(),
        SyntaxKind::StringLiteral
    );
    assert_eq!(scanner.pos(), 5);
    assert_eq!(scanner.token_value, "a\nb");
    assert!(scanner.errors().is_empty());
}

#[test]
fn template_raw_text_decodes_to_lossless_utf16() {
    assert_eq!(template_text_utf16("\u{FFFD}", Some("\\uD800")), [0xD800]);
    assert_eq!(template_text_utf16("\u{FFFD}", Some("\\u{DC00}")), [0xDC00]);
    assert_eq!(
        template_text_utf16("😀", Some("\\u{1F600}")),
        [0xD83D, 0xDE00]
    );
    assert_eq!(
        template_text_utf16("\\uD800", Some("\\\\uD800")),
        "\\uD800".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(template_text_utf16("\u{FFFD}", Some("\u{FFFD}")), [0xFFFD]);
    assert_eq!(
        template_text_utf16("a\nb", Some("a\r\nb")),
        "a\nb".encode_utf16().collect::<Vec<_>>()
    );
}

#[test]
fn jsdoc_parsing_mode_controls_preceding_comment_flag() {
    fn has_jsdoc_flag(text: &str, mode: JSDocParsingMode, is_typescript: bool) -> bool {
        let mut scanner = Scanner::new(text, LanguageVariant::Standard);
        scanner.set_jsdoc_parsing_mode(mode, is_typescript);
        scanner.scan();
        scanner.has_preceding_jsdoc_comment()
    }

    let ordinary = "/** @param {number} x */ function f() {}";
    let see = "/** @SEE f */ function f() {}";
    let link = "/** {@LINK f} */ function f() {}";

    assert!(has_jsdoc_flag(ordinary, JSDocParsingMode::ParseAll, true));
    assert!(!has_jsdoc_flag(
        ordinary,
        JSDocParsingMode::ParseNone,
        false
    ));
    assert!(!has_jsdoc_flag(
        ordinary,
        JSDocParsingMode::ParseForTypeErrors,
        true
    ));
    assert!(has_jsdoc_flag(
        see,
        JSDocParsingMode::ParseForTypeErrors,
        true
    ));
    assert!(has_jsdoc_flag(
        link,
        JSDocParsingMode::ParseForTypeErrors,
        true
    ));
    assert!(!has_jsdoc_flag(
        see,
        JSDocParsingMode::ParseForTypeInfo,
        true
    ));
    assert!(has_jsdoc_flag(
        ordinary,
        JSDocParsingMode::ParseForTypeErrors,
        false
    ));
    assert!(has_jsdoc_flag(
        ordinary,
        JSDocParsingMode::ParseForTypeInfo,
        false
    ));
}

use super::*;

#[test]
fn invariant_args_default_to_a_sample_and_full_corpus_has_no_limit() {
    let sampled = parse_invariant_args(std::iter::empty()).unwrap();
    assert_eq!(sampled.suite, InvariantSuite::All);
    assert_eq!(sampled.limit, Some(200));
    assert!(!sampled.full_corpus);

    let full = parse_invariant_args(
        ["--suite", "all", "--full-corpus"]
            .into_iter()
            .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(full.suite, InvariantSuite::All);
    assert_eq!(full.limit, None);
    assert!(full.full_corpus);
}

#[test]
fn invariant_args_reject_partial_full_corpus_spelling() {
    for arguments in [
        vec!["--limit", "10", "--full-corpus"],
        vec!["--full-corpus", "--limit", "10"],
    ] {
        assert!(parse_invariant_args(arguments.into_iter().map(str::to_owned)).is_err());
    }
    assert!(
        parse_invariant_args(["--full"].into_iter().map(str::to_owned)).is_err(),
        "an approximate alias must not accidentally create completion evidence"
    );
}

#[test]
fn non_ascii_prefix_compares_in_utf16() {
    // Six 3-byte U+2028 chars make UTF-16 offsets lag UTF-8 byte
    // offsets by 12 in the ASCII tail, so every token in the 12
    // bytes past the midpoint has a UTF-16 end below the byte cut —
    // a mixed-coordinate filter admits those tokens on the full
    // scan only and fails spuriously (the corpus shape:
    // es2019/allowUnescapedParagraphAndLineSeparatorsInStringLiteral.ts,
    // cut 400).
    let text = format!("/*{}*/{}", "\u{2028}".repeat(6), "aa=1;".repeat(12));
    assert!(prefix_determinism_holds(
        &text,
        tsc_syntax::LanguageVariant::Standard
    ));
}

#[test]
fn ascii_prefix_still_holds() {
    let text = "const value = 1;\nconst other = value + 2;\n".to_string();
    assert!(prefix_determinism_holds(
        &text,
        tsc_syntax::LanguageVariant::Standard
    ));
}

#[test]
fn invalid_numeric_prefix_excludes_the_fragmented_boundary_token() {
    let text = "// Error\r\nvar binary = 0b21010;\r\n\
                var binary1 = 0B21010;\r\n\
                var octal = 0o81010;\r\n\
                var octal = 0O91010;";
    assert_eq!(midpoint_char_boundary(text), 49);
    assert!(prefix_determinism_holds(
        text,
        tsc_syntax::LanguageVariant::Standard
    ));
}

#[test]
fn possible_comment_opener_at_the_cut_is_a_boundary_token() {
    let text = "let x = 1;// comment!!";
    let cut = midpoint_char_boundary(text);
    assert_eq!(cut, 11);
    assert_eq!(&text[cut - 1..cut + 1], "//");
    assert!(prefix_determinism_holds(
        text,
        tsc_syntax::LanguageVariant::Standard
    ));
}

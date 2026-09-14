//! Existing escape decision-table controls now exercise the owning parser flags.
//! Raw reconstruction no longer belongs to the tagged-template transformer.

use tsc_syntax::{parse_source_file, SyntaxKind};
use tsc_types::TokenFlags;

fn template_cooked_is_invalid(raw: &str) -> bool {
    let source = parse_source_file(
        "template.ts",
        &format!("tag`{raw}`;"),
        Default::default(),
        None,
    );
    let fragment = source
        .arena
        .nodes()
        .iter()
        .find(|node| node.kind == SyntaxKind::NoSubstitutionTemplateLiteral)
        .unwrap();
    let flags = TokenFlags::from_bits(fragment.template_flags);
    let invalid = flags.intersects(TokenFlags::IS_INVALID);
    assert_eq!(
        invalid,
        flags.intersects(TokenFlags::CONTAINS_INVALID_ESCAPE),
        "Only ContainsInvalidEscape of IsInvalid applies to template fragments"
    );
    invalid
}

#[test]
fn valid_escapes_and_plain_text_are_not_invalid() {
    for raw in [
        "",
        "plain text",
        "prefix\\n",
        "\\t\\b\\v\\f\\r\\'\\\"",
        "\\u0041",
        "\\u{41}",
        "\\u{10FFFF}",
        "\\x41",
        "\\0",
        "\\0x",
        "\\q",
        "\\$",
        "\\`",
        // line continuations
        "line\\\ncontinued",
        "line\\\r\ncontinued",
        // the scanner's unexpected-end path sets no invalid flag
        "trailing\\",
    ] {
        assert!(
            !template_cooked_is_invalid(raw),
            "{raw:?} misreported invalid"
        );
    }
}

#[test]
fn octal_and_digit_escapes_are_invalid() {
    for raw in [
        "\\01", "\\1", "\\7", "\\12", "\\123", "\\47", "\\8", "\\9", "\\08",
    ] {
        assert!(template_cooked_is_invalid(raw), "{raw:?} misreported valid");
    }
}

#[test]
fn short_hex_and_unicode_runs_are_invalid() {
    for raw in [
        "\\x", "\\x1", "\\xZZ", "\\u", "\\u1", "\\u12", "\\u123", "\\uZZZZ",
    ] {
        assert!(template_cooked_is_invalid(raw), "{raw:?} misreported valid");
    }
}

#[test]
fn malformed_extended_unicode_escapes_are_invalid() {
    for raw in [
        "\\u{}",
        "\\u{ZZ}",
        "\\u{110000}",
        "\\u{41",
        "\\u{41 }",
        "\\u{",
    ] {
        assert!(template_cooked_is_invalid(raw), "{raw:?} misreported valid");
    }
}

#[test]
fn consumption_mirrors_the_scanner_across_sequences() {
    // `A` consumes its four digits: the following `\8` is a fresh
    // (invalid) escape, while a digit run inside the consumed span is not.
    assert!(template_cooked_is_invalid("\\u0041\\8"));
    assert!(!template_cooked_is_invalid("\\u0041 8"));
    // `\x41` consumes two digits; the trailing text is plain.
    assert!(!template_cooked_is_invalid("\\x4141"));
    // A valid escape after an invalid one still reports invalid.
    assert!(template_cooked_is_invalid("\\8\\n"));
    // Escaped backslash does not hide a following invalid escape...
    assert!(template_cooked_is_invalid("\\\\\\8"));
    // ...and does not invent one.
    assert!(!template_cooked_is_invalid("\\\\8"));
}

//! Decision-table contracts for the module-internal
//! `templateFlags & IsInvalid` recomputation
//! (`template_cooked_is_invalid`), row-for-row against
//! `scan_escape_sequence`'s flagging (scanner.rs:1114-1282). The
//! byte-level lowering itself is qualified by the B-5 focused oracle
//! projections and the 32-case witness gate, not here.

use super::template_cooked_is_invalid;

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

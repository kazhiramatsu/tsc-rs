use super::*;
use tsc_diagnostics::DiagnosticCategory;

fn diagnostics_for(text: &str, target: ScriptTarget) -> Vec<RegexDiagnostic> {
    validate_regular_expression_literal(text, target)
}

fn codes(text: &str, target: ScriptTarget) -> Vec<u32> {
    diagnostics_for(text, target)
        .into_iter()
        .map(|diagnostic| diagnostic.message.code)
        .collect()
}

/// The rows as (code, start, length) in UTF-16 units from the first slash.
fn rows(text: &str, target: ScriptTarget) -> Vec<(u32, u32, u32)> {
    diagnostics_for(text, target)
        .into_iter()
        .map(|diagnostic| {
            (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            )
        })
        .collect()
}

#[test]
fn validates_flags_and_target_gates() {
    let duplicate = diagnostics_for("/a/gg", ScriptTarget::ES_NEXT);
    assert_eq!(duplicate.len(), 1);
    assert_eq!(
        duplicate[0].message,
        &diagnostics::Duplicate_regular_expression_flag
    );
    assert_eq!(
        (duplicate[0].start_utf16, duplicate[0].length_utf16),
        (4, 1)
    );

    assert_eq!(
        codes("/a/z", ScriptTarget::ES_NEXT),
        vec![diagnostics::Unknown_regular_expression_flag.code]
    );
    assert_eq!(
        codes("/a/uv", ScriptTarget::ES_NEXT),
        vec![
            diagnostics::The_Unicode_u_flag_and_the_Unicode_Sets_v_flag_cannot_be_set_simultaneously
                .code
        ]
    );
    // tsgo gates only d, s and v (scanner/regexp.go:45-49).
    for literal in ["/a/u", "/a/y"] {
        let target = ScriptTarget::from_bits(ScriptTarget::ES2015.bits() - 1);
        assert!(diagnostics_for(literal, target).is_empty(), "{literal}");
    }
    for (literal, minimum, target_name) in [
        ("/a/s", ScriptTarget::ES2018, "es2018"),
        ("/a/d", ScriptTarget::ES2022, "es2022"),
        ("/a/v", ScriptTarget::ES2024, "es2024"),
    ] {
        let target = ScriptTarget::from_bits(minimum.bits() - 1);
        let actual = diagnostics_for(literal, target);
        assert_eq!(actual.len(), 1, "{literal}");
        assert_eq!(
            actual[0].message,
            &diagnostics::This_regular_expression_flag_is_only_available_when_targeting_0_or_later
        );
        assert_eq!(actual[0].args, vec![target_name]);
        assert!(diagnostics_for(literal, minimum).is_empty(), "{literal}");
    }
}

#[test]
fn validates_extended_unicode_escapes_in_utf16_units() {
    let actual = diagnostics_for("/\\u{-DDDD}/gu", ScriptTarget::ES_NEXT);
    assert_eq!(
        actual
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16
            ))
            .collect::<Vec<_>>(),
        // tsgo returns from the escape as soon as no digit follows the
        // brace, so `}` is reported where the pattern reaches it.
        vec![
            (diagnostics::Hexadecimal_digit_expected.code, 4, 0),
            (
                diagnostics::Unexpected_0_Did_you_mean_to_escape_it_with_backslash.code,
                9,
                1
            ),
        ]
    );

    let supplementary = diagnostics_for("/😀{/u", ScriptTarget::ES_NEXT);
    assert_eq!(supplementary.len(), 1);
    assert_eq!(supplementary[0].start_utf16, 3);
    assert_eq!(
        supplementary[0].message,
        &diagnostics::Unexpected_0_Did_you_mean_to_escape_it_with_backslash
    );

    let overflowing = diagnostics_for("/\\u{FFFFFFFFFFFFFFFF}/u", ScriptTarget::ES_NEXT);
    assert_eq!(
        overflowing[0].message,
        &diagnostics::An_extended_Unicode_escape_value_must_be_between_0x0_and_0x10FFFF_inclusive
    );
    assert_eq!(
        (overflowing[0].start_utf16, overflowing[0].length_utf16),
        (4, 16)
    );
}

#[test]
fn validates_groups_backreferences_and_subpattern_modifiers() {
    assert!(diagnostics_for("/(?<name>a)\\k<name>/u", ScriptTarget::ES_NEXT).is_empty());
    assert_eq!(
        codes("/(?<name>a)\\k<nme>/u", ScriptTarget::ES_NEXT),
        vec![
            diagnostics::There_is_no_capturing_group_named_0_in_this_regular_expression.code,
            diagnostics::Did_you_mean_0.code,
        ]
    );
    assert_eq!(
        codes("/\\1/u", ScriptTarget::ES_NEXT),
        vec![
            diagnostics::This_backreference_refers_to_a_group_that_does_not_exist_There_are_no_capturing_groups_in_this_regular_expression.code
        ]
    );
    assert_eq!(
        codes("/(?u:a)/u", ScriptTarget::ES_NEXT),
        vec![diagnostics::This_regular_expression_flag_cannot_be_toggled_within_a_subpattern.code]
    );
    assert_eq!(
        codes("/(?-:a)/u", ScriptTarget::ES_NEXT),
        vec![diagnostics::Subpattern_flags_must_be_present_when_there_is_a_minus_sign.code]
    );
    assert_eq!(
        diagnostics_for("/\\2(a)/u", ScriptTarget::ES_NEXT)
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            ))
            .collect::<Vec<_>>(),
        vec![(
            diagnostics::This_backreference_refers_to_a_group_that_does_not_exist_There_are_only_0_capturing_groups_in_this_regular_expression.code,
            2,
            1,
        )]
    );
    assert_eq!(
        diagnostics_for("/fo(o/", ScriptTarget::ES_NEXT)
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            ))
            .collect::<Vec<_>>(),
        vec![(diagnostics::_0_expected.code, 5, 0)]
    );
}

#[test]
fn validates_control_escape_boundaries_and_recovery() {
    assert!(
        diagnostics_for("/\\cA\\cz[\\cB\\cy]/u", ScriptTarget::ES_NEXT).is_empty(),
        "ASCII letters are valid control-escape operands"
    );

    assert_eq!(
        diagnostics_for("/\\c0\\c_\\c/u", ScriptTarget::ES_NEXT)
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                diagnostics::c_must_be_followed_by_an_ASCII_letter.code,
                1,
                2
            ),
            (
                diagnostics::c_must_be_followed_by_an_ASCII_letter.code,
                4,
                2
            ),
            (
                diagnostics::c_must_be_followed_by_an_ASCII_letter.code,
                7,
                2
            ),
        ]
    );

    for (literal, control_start, identity_start) in [("/\\c\\`/u", 1, 3), ("/[\\c\\`]/u", 2, 4)] {
        assert_eq!(
            diagnostics_for(literal, ScriptTarget::ES_NEXT)
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    diagnostics::c_must_be_followed_by_an_ASCII_letter.code,
                    control_start,
                    2,
                ),
                (
                    diagnostics::This_character_cannot_be_escaped_in_a_regular_expression.code,
                    identity_start,
                    2,
                ),
            ],
            "the invalid control escape must not consume its backslash follower: {literal}"
        );
    }

    assert!(diagnostics_for("/\\c\\`/", ScriptTarget::ES_NEXT).is_empty());
    assert!(diagnostics_for("/[\\c\\`]/", ScriptTarget::ES_NEXT).is_empty());
}

#[test]
fn rejects_all_identity_escapes_in_unicode_modes() {
    for literal in ["/\\`/u", "/\\`/v"] {
        let actual = diagnostics_for(literal, ScriptTarget::ES_NEXT);
        assert_eq!(actual.len(), 1, "{literal}");
        assert_eq!(
            actual[0].message,
            &diagnostics::This_character_cannot_be_escaped_in_a_regular_expression
        );
        assert_eq!((actual[0].start_utf16, actual[0].length_utf16), (1, 2));
    }

    assert!(diagnostics_for("/\\`/", ScriptTarget::ES_NEXT).is_empty());
}

#[test]
fn validates_classes_sets_and_unicode_properties() {
    assert_eq!(
        codes("/[z-a]/u", ScriptTarget::ES_NEXT),
        vec![diagnostics::Range_out_of_order_in_character_class.code]
    );
    assert_eq!(
        diagnostics_for("/[&&a]/v", ScriptTarget::ES_NEXT)
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            ))
            .collect::<Vec<_>>(),
        vec![(diagnostics::Expected_a_class_set_operand.code, 2, 0)]
    );
    assert_eq!(
        diagnostics_for("/[!!]/v", ScriptTarget::ES_NEXT)
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            ))
            .collect::<Vec<_>>(),
        vec![(
            diagnostics::A_character_class_must_not_contain_a_reserved_double_punctuator_Did_you_mean_to_escape_it_with_backslash.code,
            2,
            2,
        )]
    );

    let property = diagnostics_for("/\\p{General_Categor=Letter}/u", ScriptTarget::ES_NEXT);
    assert_eq!(
        property
            .iter()
            .map(|diagnostic| (
                diagnostic.message.code,
                diagnostic.message.category,
                diagnostic.start_utf16,
                diagnostic.length_utf16,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                diagnostics::Unknown_Unicode_property_name.code,
                DiagnosticCategory::Error,
                4,
                15,
            ),
            (
                diagnostics::Did_you_mean_0.code,
                DiagnosticCategory::Message,
                4,
                15,
            ),
        ]
    );
    assert_eq!(property[1].args, vec!["General_Category"]);

    assert_eq!(
        codes("/\\p{Basic_Emoji}/u", ScriptTarget::ES_NEXT),
        vec![
            diagnostics::Any_Unicode_property_that_would_possibly_match_more_than_a_single_character_is_only_available_when_the_Unicode_Sets_v_flag_is_set.code
        ]
    );
    assert!(diagnostics_for("/\\p{Script=Latin}/u", ScriptTarget::ES_NEXT).is_empty());
    let property_value = diagnostics_for("/\\p{Script=Latn_}/u", ScriptTarget::ES_NEXT);
    assert_eq!(
        property_value
            .iter()
            .map(|diagnostic| diagnostic.message.code)
            .collect::<Vec<_>>(),
        vec![
            diagnostics::Unknown_Unicode_property_value.code,
            diagnostics::Did_you_mean_0.code,
        ]
    );
    assert_eq!(property_value[1].args, vec!["Latn"]);
}

#[test]
fn named_groups_and_pattern_modifiers_follow_tsgo() {
    // Rows from tsc-19dadef8 (scanner/regexp.go:156-184, 268-284, 500-522).
    // A name defined in a nested group conflicts with a sibling group.
    assert_eq!(
        rows("/(?:(?<a>x))(?<a>z)/", ScriptTarget::ES_NEXT),
        [(1515, 15, 1)]
    );
    // A duplicate in a mutually exclusive alternative needs ES2025.
    for target in [ScriptTarget::ES2018, ScriptTarget::ES2022] {
        assert_eq!(rows("/(?<a>x)|(?<a>y)/", target), [(18063, 12, 1)]);
        assert_eq!(
            rows("/(?<a>x)(?<b>y)|(?:(?<a>z))/", target),
            [(18063, 22, 1)]
        );
    }
    assert!(rows("/(?<a>x)|(?<a>y)/", ScriptTarget::ES2025).is_empty());
    // Pattern modifiers need ES2025: the row covers the modifier text, and
    // no flag is gated on its own inside a subpattern.
    assert_eq!(
        rows("/(?i:a)(?-s:b)(?m-:c)/", ScriptTarget::ES2017),
        [(18062, 3, 1), (18062, 9, 2), (18062, 16, 2)]
    );
    assert!(rows("/(?i:a)(?-s:b)(?m-:c)/", ScriptTarget::ES2025).is_empty());
}

#[test]
fn quantifier_bounds_compare_as_decimal_strings_like_tsgo() {
    assert_eq!(
        rows(
            "/a{9223372036854775808,9223372036854775807}/",
            ScriptTarget::ES_NEXT
        ),
        [(1506, 3, 39)]
    );
    assert_eq!(
        rows(
            "/a{9007199254740993,9007199254740992}/",
            ScriptTarget::ES_NEXT
        ),
        [(1506, 3, 33)]
    );
    assert!(rows(
        "/a{9223372036854775807,9223372036854775808}/",
        ScriptTarget::ES_NEXT
    )
    .is_empty());
}

#[test]
fn class_sets_follow_tsgo() {
    // A lone `&` is an ordinary class-set character
    // (scanner/regexp.go:629-692).
    assert!(rows("/[a&b&c]/v", ScriptTarget::ES_NEXT).is_empty());
    // Every union operand of a negated class is checked.
    assert_eq!(
        rows("/[^\\p{Emoji}\\p{RGI_Emoji}]/v", ScriptTarget::ES_NEXT),
        [(1518, 12, 13)]
    );
    // A subtraction keeps its first operand's strings.
    assert!(rows("/[\\q{ab}--\\q{a}]/v", ScriptTarget::ES_NEXT).is_empty());
    assert_eq!(
        rows("/[^[\\q{ab}--\\q{a}]]/v", ScriptTarget::ES_NEXT),
        [(1518, 3, 15)]
    );
    // U+0126 and U+012F, whose low bytes are `&` and `/`, are no
    // punctuators: neither a reserved double punctuator nor a syntax
    // character, and escaping one is invalid in Unicode mode.
    assert!(rows("/[\u{126}\u{126}][\u{12F}]/v", ScriptTarget::ES_NEXT).is_empty());
    assert_eq!(rows("/\\\u{12F}/u", ScriptTarget::ES_NEXT), [(1535, 1, 2)]);
}

#[test]
fn non_bmp_characters_and_group_name_escapes_follow_tsgo() {
    // Outside Unicode mode a non-BMP character is two atoms that both start
    // at the character (scanner/regexp.go:1016-1057).
    assert_eq!(
        rows(
            "/[\u{1D608}-\u{1D621}][\u{1D621}-\u{1D608}]/",
            ScriptTarget::ES_NEXT
        ),
        [(1517, 2, 3), (1517, 9, 3)]
    );
    // An escaped non-BMP character is one identity escape.
    assert_eq!(
        rows("/\\\u{1D608}/u", ScriptTarget::ES_NEXT),
        [(1535, 1, 3)]
    );
    // An unbraced `\uHigh\uLow` pair in Unicode mode is one character, also
    // at the end of the pattern.
    assert_eq!(
        rows(
            "/[\\uD835\\uDE08-\\uD835\\uDE21][\\uD835\\uDE21-\\uD835\\uDE08]/u",
            ScriptTarget::ES_NEXT
        ),
        [(1517, 29, 25)]
    );
    assert!(rows("/\\uD835\\uDE08/u", ScriptTarget::ES_NEXT).is_empty());
    // Group names take `\u` escapes, braced or not and whatever the flags,
    // and an unbraced pair of surrogate escapes.
    assert_eq!(
        rows(
            "/(?<\\u{13A0}>)\\k<\\u13A0>\\k<\\uD800\\uDC00>(?<\\uD835\\uDC00>)\\k<\\u{1D400}>/",
            ScriptTarget::ES_NEXT
        ),
        [(1532, 27, 12)]
    );
    // A braced pair is not a name: TS1514 at the escape, then `>` and the
    // escape itself are reported at the same position, which the parser
    // drops (it keeps the first scanner error at a position).
    assert_eq!(
        rows(
            "/(?<\u{1D400}>)\\k<\\uD835\\uDC00>\\k<\\u{D835}\\u{DC00}>/",
            ScriptTarget::ES_NEXT
        ),
        [(1514, 27, 0), (1005, 27, 0), (1538, 27, 8), (1538, 35, 8)]
    );
}

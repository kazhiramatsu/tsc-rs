use super::fixup_whitespace_and_decode_entities;

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn jsx_text_normalizer_classifies_only_first_middle_and_last_lines() {
    assert_eq!(
        fixup_whitespace_and_decode_entities("   "),
        Some(utf16("   "))
    );
    assert_eq!(fixup_whitespace_and_decode_entities(" \n \t"), None);
    assert_eq!(
        fixup_whitespace_and_decode_entities(
            "  first  \r\n \u{0085}&nbsp; middle\u{200b}\n  last  ",
        ),
        Some(utf16("  first \u{00a0} middle last  "))
    );
}

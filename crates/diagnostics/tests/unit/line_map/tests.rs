use super::{compute_line_map, compute_line_starts};

#[test]
fn line_starts_match_tsc_line_breaks() {
    assert_eq!(
        compute_line_starts("a\r\nb\nc\u{2028}d\u{2029}e"),
        vec![0, 3, 5, 7, 9]
    );
}

#[test]
fn columns_are_utf16_code_units() {
    let map = compute_line_map("a😀b\nc");
    assert_eq!(map.byte_to_utf16(0), Some(0));
    assert_eq!(map.byte_to_utf16("a".len() as u32), Some(1));
    assert_eq!(map.byte_to_utf16("a😀".len() as u32), Some(3));
    assert_eq!(
        map.line_and_character_utf16(4).unwrap(),
        super::LineAndCharacter {
            line: 0,
            character: 4,
        }
    );
}

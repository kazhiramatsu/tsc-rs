use super::*;

#[test]
fn generated_diagnostic_pins_match_tsc() {
    assert_eq!(Unterminated_string_literal.code, 1002);
    assert_eq!(_0_expected.code, 1005);
    assert_eq!(ALL_BY_CODE.len(), 2130);
}

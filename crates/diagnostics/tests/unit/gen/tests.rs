use super::*;

#[test]
fn generated_diagnostic_pins_match_typescript_7_1() {
    assert_eq!(Unterminated_string_literal.code, 1002);
    assert_eq!(_0_expected.code, 1005);
    assert_eq!(ALL_BY_CODE.len(), 2222);
    assert!(ALL_BY_CODE.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert_eq!(
        A_label_is_not_allowed_here.text,
        "A label is not allowed here."
    );
}

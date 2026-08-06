use super::SyntaxKind;

#[test]
fn generated_values_match_tsc_pins() {
    assert_eq!(SyntaxKind::Identifier as u16, 80);
    assert_eq!(
        SyntaxKind::FirstAssignment.value(),
        SyntaxKind::EqualsToken as u16
    );
}

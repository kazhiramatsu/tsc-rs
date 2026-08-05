use super::*;

#[test]
fn generated_values_match_tsc_pins() {
    assert_eq!(TypeFlags::STRING_LITERAL.bits(), 1024);
    assert_eq!(FlowFlags::TRUE_CONDITION.bits(), 32);
}

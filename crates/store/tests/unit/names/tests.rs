use super::*;

#[test]
fn name_hints_keep_text_lengths_scalar_validity_and_legacy_fallback() {
    assert_eq!(std::mem::size_of::<NameRecord>(), 8);
    for value in ["", "ordinary", "名前", "a long identifier name"] {
        let id = push_str_with_contextual_hint(value, false);
        assert_eq!(text(id), value.as_bytes());
        assert_eq!(scalar_text(id), Some(value));
        assert!(!may_be_contextual_identifier(id));
    }
    let id = push_str_with_contextual_hint("private", true);
    assert!(may_be_contextual_identifier(id));
    assert_eq!(scalar_text(id), Some("private"));
    assert!(may_be_contextual_identifier(push_str("legacy unknown")));
    let surrogate = [0xed, 0xa0, 0x80];
    let id = push_with_contextual_hint(&surrogate, false);
    assert_eq!(text(id), surrogate);
    assert_eq!(scalar_text(id), None);
    assert!(may_be_contextual_identifier(id));
}

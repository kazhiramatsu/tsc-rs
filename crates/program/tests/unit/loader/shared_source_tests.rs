use super::*;

#[test]
fn shared_source_text_survives_decoding_and_snapshot_construction() {
    static TEXT: tsc_diagnostics::StaticText<4> = tsc_diagnostics::StaticText::new("😀");
    let text = TEXT.shared();
    let snapshot = decode_source_text(SourceRead::Text(text.clone()))
        .unwrap()
        .into_snapshot();
    assert!(snapshot.shared_text().ptr_eq(&text));
    assert!(!snapshot.shared_text().in_text_heap());
    assert_eq!(snapshot.positions().byte_to_utf16(4), Some(2));
    let owned = decode_source_text(SourceRead::Bytes(vec![0xff, 0xfe, 0x3d, 0xd8, 0, 0xde]))
        .unwrap()
        .into_snapshot();
    assert_eq!(owned.text(), "😀");
    assert!(owned.shared_text().in_text_heap());
}

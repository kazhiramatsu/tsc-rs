use tsc_diagnostics::PositionIndex;
use tsc_emitter::{
    create_text_writer, NewLineKind, SourceBytePosition, SourceByteRange, SourcePositionError,
    SourceRange, SourceUtf16Location, SourceUtf16Position,
};

#[test]
fn writer_tracks_utf8_text_and_utf16_coordinates_independently() {
    let mut writer = create_text_writer(NewLineKind::LineFeed);
    writer.write("A😀e\u{301}");
    assert_eq!(writer.text(), "A😀e\u{301}");
    assert_eq!(writer.text_position().value(), 5);
    assert_eq!(writer.line(), 0);
    assert_eq!(writer.column(), 5);

    writer.write("\r\n雪\u{2028}x");
    assert_eq!(writer.line(), 2);
    assert_eq!(writer.column(), 1);
    assert_eq!(writer.text_position().value(), 10);
    assert!(!writer.has_trailing_whitespace());
    writer.write("\u{85}");
    assert_eq!(writer.text_position().value(), 11);
    assert!(writer.has_trailing_whitespace());

    writer.write_line(false);
    writer.increase_indent();
    assert_eq!(writer.column(), 4);
    writer.write_keyword("const");
    assert!(writer.text().ends_with("\n    const"));
    assert_eq!(writer.column(), 9);

    writer.write_comment("/*c*/");
    assert!(writer.has_trailing_comment());
    writer.write_space(" ");
    assert!(!writer.has_trailing_comment());
    writer.clear();
    assert_eq!(writer.text(), "");
    assert_eq!(writer.location().position().value(), 0);
    assert!(writer.is_at_start_of_line());
}

#[test]
fn source_positions_reject_byte_utf16_and_synthetic_confusion() {
    let text = "a😀\r\ne\u{301}\u{2028}z";
    let positions = PositionIndex::new_static(text);
    let emoji_end_byte = "a😀".len() as u32;
    let emoji_end = SourceBytePosition::new(emoji_end_byte, &positions).expect("scalar boundary");
    assert_eq!(
        SourceUtf16Position::from_byte(emoji_end, &positions)
            .unwrap()
            .value(),
        3
    );
    let location = SourceUtf16Location::from_byte(emoji_end, &positions).unwrap();
    assert_eq!((location.line(), location.column()), (0, 3));

    assert_eq!(
        SourceBytePosition::new(2, &positions),
        Err(SourcePositionError::NotUnicodeScalarBoundary { position: 2 })
    );
    assert!(matches!(
        SourceBytePosition::new(100, &positions),
        Err(SourcePositionError::OutOfBounds { .. })
    ));
    assert!(matches!(
        SourceByteRange::new(emoji_end_byte, 1, &positions),
        Err(SourcePositionError::InvertedRange { .. })
    ));
    assert_eq!(
        SourceRange::from_raw(u32::MAX, u32::MAX, &positions).unwrap(),
        SourceRange::Synthesized
    );
    assert!(matches!(
        SourceRange::from_raw(u32::MAX, 0, &positions),
        Err(SourcePositionError::MixedSyntheticRange { .. })
    ));
}

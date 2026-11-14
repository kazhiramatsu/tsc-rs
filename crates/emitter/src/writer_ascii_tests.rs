use super::*;
use tsc_syntax::SyntaxKind;

#[test]
fn every_syntax_token_spelling_satisfies_the_fixed_text_contract() {
    for raw in 0..=u16::MAX {
        let Some(kind) = SyntaxKind::from_u16(raw) else {
            continue;
        };
        let expected = tsc_syntax::tokens::token_to_string(kind);
        let actual = AsciiText::token(kind);
        assert_eq!(actual.map(AsciiText::text), expected, "{kind:?}");
        if let Some(text) = actual {
            assert_eq!(AsciiText::new(text.text()), text, "{kind:?}");
        }
    }
}

#[test]
fn arbitrary_text_cannot_enter_the_fixed_text_writer() {
    for text in ["", "\n", "x\r", "π", "\u{2028}", "😀"] {
        assert!(std::panic::catch_unwind(|| AsciiText::new(text)).is_err());
    }
}

#[test]
fn fixed_writes_preserve_all_writer_state_across_writer_kinds() {
    let spellings = [
        ascii_text!(" "),
        ascii_text!(";"),
        ascii_text!("const"),
        ascii_text!("=>"),
        ascii_text!("{"),
        ascii_text!("$"),
        ascii_text!("\t"),
        ascii_text!("\0"),
        ascii_text!("some longer fixed text"),
    ];
    for new_line in [NewLineKind::LineFeed, NewLineKind::CarriageReturnLineFeed] {
        for width in [0, 1, 4, 5, 65] {
            let mut writers = vec![
                TextWriter::new(new_line),
                TextWriter::single_line(),
                TextWriter::display_parts(),
                TextWriter::change_tracker(new_line, width),
                TextWriter::snippet_change_tracker(new_line, width),
            ];
            let mut multiline_parts = TextWriter::new(new_line);
            multiline_parts.display_parts = Some(Vec::new());
            writers.push(multiline_parts);
            for mut initial in writers {
                initial.indent_width = width;
                for depth in [0, 1, 17] {
                    for prefix in [0, 1, 2] {
                        for pending in [false, true] {
                            let mut plain = initial.clone();
                            plain.indent = depth;
                            if prefix == 1 {
                                plain.write("π😀\r\n");
                            } else if prefix == 2 {
                                plain.write_literal_utf16(&[0xd800]);
                                plain.write_line(true);
                            }
                            plain.write_comment("/* trailing */");
                            plain.write_line(true);
                            if pending {
                                plain.defer_trailing_semicolons();
                                plain.write_trailing_semicolon(";");
                            }
                            let mut fixed = plain.clone();
                            for (index, text) in spellings.into_iter().enumerate() {
                                for class in 0..4 {
                                    match class {
                                        0 => {
                                            plain.write_keyword(text.text());
                                            fixed.write_ascii_keyword(text);
                                        }
                                        1 => {
                                            plain.write_operator(text.text());
                                            fixed.write_ascii_operator(text);
                                        }
                                        2 => {
                                            plain.write_punctuation(text.text());
                                            fixed.write_ascii_punctuation(text);
                                        }
                                        _ => {
                                            plain.write_space(text.text());
                                            fixed.write_ascii_space(text);
                                        }
                                    }
                                    assert_eq!(plain, fixed, "width={width} depth={depth} prefix={prefix} pending={pending} token={index} class={class}");
                                }
                                plain.write_line(index % 2 == 0);
                                fixed.write_line(index % 2 == 0);
                                assert_eq!(plain, fixed);
                            }
                            plain.clear();
                            fixed.clear();
                            assert_eq!(plain, fixed);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn indentation_chunks_preserve_utf16_authority_and_coordinates() {
    for width in [0, 1, 4, 5, 64, 65] {
        for depth in [0, 1, 2, 17] {
            for new_line in [NewLineKind::LineFeed, NewLineKind::CarriageReturnLineFeed] {
                let mut writer = TextWriter::change_tracker(new_line, width);
                writer.write_literal_utf16(&[0xd800]);
                writer.write_line(true);
                writer.indent = depth;
                writer.write_ascii_punctuation(ascii_text!(";"));
                let mut expected = vec![0xd800];
                expected.extend(new_line.text().encode_utf16());
                expected.extend(std::iter::repeat_n(32, width * depth as usize));
                expected.push(59);
                assert_eq!(writer.text_utf16().as_ref(), expected);
                assert_eq!(writer.text(), String::from_utf16_lossy(&expected));
                assert_eq!(writer.text_position().value() as usize, expected.len());
                assert_eq!(writer.line(), 1);
                assert_eq!(writer.column() as usize, width * depth as usize + 1);
                assert!(!writer.is_at_start_of_line());
                assert_eq!(
                    writer.positions.as_ref().unwrap().last_non_trivia_position,
                    writer.text().len()
                );
            }
        }
    }
}

#[test]
fn fixed_tokens_keep_classifications_and_semicolon_order() {
    let mut writer = TextWriter::display_parts();
    writer.defer_trailing_semicolons();
    writer.write_trailing_semicolon(";");
    writer.write_ascii_keyword(ascii_text!("const"));
    writer.write_ascii_space(ascii_text!(" "));
    writer.write_ascii_operator(ascii_text!("="));
    writer.write_ascii_punctuation(ascii_text!(";"));
    assert_eq!(writer.text(), ";const =;");
    let parts = writer.take_display_parts().unwrap();
    assert_eq!(
        parts
            .iter()
            .map(|p| (p.classification, p.text.as_str()))
            .collect::<Vec<_>>(),
        [
            ("punctuation", ";"),
            ("keyword", "const"),
            ("whitespace", " "),
            ("operator", "="),
            ("punctuation", ";")
        ]
    );
}

#[test]
fn public_token_writers_still_measure_variable_unicode_and_line_breaks() {
    for write in [
        TextWriter::write_keyword,
        TextWriter::write_operator,
        TextWriter::write_punctuation,
        TextWriter::write_space,
    ] {
        let mut writer = TextWriter::new(NewLineKind::LineFeed);
        write(&mut writer, "π\r\n😀\u{2028}");
        assert_eq!(writer.text(), "π\r\n😀\u{2028}");
        assert_eq!(writer.text_position().value(), 6);
        assert_eq!(writer.line(), 2);
        assert_eq!(writer.column(), 0);
        assert!(writer.is_at_start_of_line());
    }
}

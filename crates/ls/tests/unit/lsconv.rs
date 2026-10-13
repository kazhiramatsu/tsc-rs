//! tsgo `ls/lsconv/converters_test.go`'s conversions.

use std::sync::Arc;

use tsc_lsproto::{LanguageKind, Position, PositionEncodingKind};
use tsc_syntax::ScriptKind;

use super::*;

struct Text(&'static str);

impl Script for Text {
    fn file_name(&self) -> &str {
        "/test.ts"
    }

    fn text(&self) -> &str {
        self.0
    }
}

fn converters(text: &str, encoding: PositionEncodingKind) -> Converters {
    let line_map = Arc::new(LspLineMap::new(text));
    Converters::new(encoding, move |_| Arc::clone(&line_map))
}

/// The reference tsgo computes with Node.js: at each code point boundary,
/// the UTF-8 offset and the line and UTF-16 character, lines broken by
/// `\n`, `\r` and `\r\n` only.
fn reference(text: &str) -> Vec<(usize, u32, u32)> {
    let mut tuples = Vec::new();
    let (mut line, mut character) = (0, 0);
    let mut chars = text.char_indices().peekable();
    tuples.push((0, 0, 0));
    while let Some((offset, char)) = chars.next() {
        let next = offset + char.len_utf8();
        match char {
            '\r' if chars.peek().map(|&(_, next)| next) == Some('\n') => {
                // The `\n` of `\r\n` ends the line one character later.
                tuples.push((next, line, character + 1));
                let (_, newline) = chars.next().unwrap();
                line += 1;
                character = 0;
                tuples.push((next + newline.len_utf8(), line, character));
                continue;
            }
            '\r' | '\n' => {
                line += 1;
                character = 0;
            }
            _ => character += char.len_utf16() as u32,
        }
        tuples.push((next, line, character));
    }
    tuples
}

/// tsgo `TestConvertersAgainstJSReference`.
#[test]
fn conversions_match_utf16_semantics() {
    for text in [
        "",
        "hello\nworld",
        "hello\r\nworld\r\n!",
        "a\rb\rc",
        "abc\n",
        "ab\u{2014}cd\nef",
        "α\nβ\nγδε\nzz",
        "x\u{1F600}y\nz",
        "ab\u{1F600}\ncd\u{1F60A}",
        "\u{1F600}\u{1F601}\u{1F602}",
        "α — \u{1F600}\r\nβ\nγ\r",
        "  \tαβ\n\t\u{1F600}  end\n",
        "\u{1F468}\u{200D}\u{1F4BB}\nnext",
        "\n\n\r\n\r",
    ] {
        let converters = converters(text, PositionEncodingKind::UTF16);
        let script = Text(text);
        for (offset, line, character) in reference(text) {
            let position = Position { line, character };
            assert_eq!(
                converters.to_lsp_position(&script, offset),
                position,
                "to_lsp_position({offset}) in {text:?}"
            );
            assert_eq!(
                converters.from_lsp_position(&script, &position),
                offset,
                "from_lsp_position({line}, {character}) in {text:?}"
            );
        }
    }
}

/// UTF-8 positions are byte offsets within the line; positions beyond a
/// line or the text clamp.
#[test]
fn utf8_positions_and_clamping() {
    let text = "αb\ncd";
    let script = Text(text);
    let utf8 = converters(text, PositionEncodingKind::UTF8);
    assert_eq!(
        utf8.to_lsp_position(&script, 3),
        Position {
            line: 0,
            character: 3
        }
    );
    assert_eq!(
        utf8.from_lsp_position(
            &script,
            &Position {
                line: 0,
                character: 3
            }
        ),
        3
    );
    let utf16 = converters(text, PositionEncodingKind::UTF16);
    // A character past its line is the line's end (its break included).
    assert_eq!(
        utf16.from_lsp_position(
            &script,
            &Position {
                line: 0,
                character: 9
            }
        ),
        4
    );
    // A line past the end is the text's end.
    assert_eq!(
        utf16.from_lsp_position(
            &script,
            &Position {
                line: 5,
                character: 0
            }
        ),
        text.len()
    );
    assert_eq!(
        utf16.to_lsp_position(&script, 99),
        Position {
            line: 1,
            character: 2
        }
    );
}

#[test]
fn line_maps_break_on_newlines_only() {
    let map = LspLineMap::new("a\u{2028}b\rc\r\nd\ne");
    assert_eq!(map.line_starts, vec![0, 6, 9, 11]);
    assert!(!map.ascii_only);
    assert_eq!(map.index_of_line_start(8), 1);
    assert_eq!(map.index_of_line_start(10), 2);
    assert!(LspLineMap::new("abc").ascii_only);
}

/// tsgo `LanguageKindToScriptKind`.
#[test]
fn language_kinds() {
    for (language, kind) in [
        ("typescript", ScriptKind::Ts),
        ("typescriptreact", ScriptKind::Tsx),
        ("javascript", ScriptKind::Js),
        ("javascriptreact", ScriptKind::Jsx),
        ("json", ScriptKind::Json),
        ("vue", ScriptKind::Unknown),
    ] {
        assert_eq!(
            language_kind_to_script_kind(&LanguageKind(language.into())),
            kind
        );
    }
}

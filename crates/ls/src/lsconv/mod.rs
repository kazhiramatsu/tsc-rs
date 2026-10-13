//! tsgo `ls/lsconv`: conversions between the protocol's positions (line and
//! character, in UTF-16 code units unless the client and the server agreed
//! on UTF-8) and offsets in a text (UTF-8 bytes), and from the client's
//! language ids.

mod line_map;

use std::ops::Range;
use std::sync::Arc;

use tsc_lsproto::{LanguageKind, Position, PositionEncodingKind};
use tsc_syntax::ScriptKind;

pub use line_map::LspLineMap;

/// tsgo `Script` without a content mapper's span map: a file's name and
/// text.
pub trait Script {
    fn file_name(&self) -> &str;
    fn text(&self) -> &str;
}

/// The line map of a file, by its name.
pub type LineMapSource = dyn Fn(&str) -> Arc<LspLineMap> + Send + Sync;

/// tsgo `Converters`: the session's position encoding and the line maps of
/// its files.
pub struct Converters {
    position_encoding: PositionEncodingKind,
    line_map: Box<LineMapSource>,
}

impl Converters {
    /// tsgo `NewConverters`.
    pub fn new(
        position_encoding: PositionEncodingKind,
        line_map: impl Fn(&str) -> Arc<LspLineMap> + Send + Sync + 'static,
    ) -> Self {
        Self {
            position_encoding,
            line_map: Box::new(line_map),
        }
    }

    /// tsgo `ToLSPPosition` (`positionToLineAndCharacter`): an offset's line
    /// and character, the offset clamped to the text.
    pub fn to_lsp_position(&self, script: &dyn Script, position: usize) -> Position {
        let text = script.text();
        let position = position.min(text.len());
        let line_map = (self.line_map)(script.file_name());
        let line = line_map
            .index_of_line_start(position)
            .min(line_map.line_starts.len() - 1);
        let start = line_map.line_starts[line];
        let character =
            if line_map.ascii_only || self.position_encoding == PositionEncodingKind::UTF8 {
                position - start
            } else {
                text[start..position].chars().map(char::len_utf16).sum()
            };
        Position {
            line: line as u32,
            character: character as u32,
        }
    }

    /// tsgo `ToLSPRange`.
    pub fn to_lsp_range(&self, script: &dyn Script, range: Range<usize>) -> tsc_lsproto::Range {
        tsc_lsproto::Range {
            start: self.to_lsp_position(script, range.start),
            end: self.to_lsp_position(script, range.end),
        }
    }

    /// tsgo `FromLSPPosition` (`lineAndCharacterToPosition`): a line and
    /// character's offset; a line past the end is the text's end, a
    /// character past its line the line's end.
    pub fn from_lsp_position(&self, script: &dyn Script, position: &Position) -> usize {
        let text = script.text();
        let line_map = (self.line_map)(script.file_name());
        let line = position.line as usize;
        let Some(&start) = line_map.line_starts.get(line) else {
            return text.len();
        };
        let line_end = line_map
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(text.len());
        let character = position.character as usize;
        if line_map.ascii_only || self.position_encoding == PositionEncodingKind::UTF8 {
            return start.max((start + character).min(line_end));
        }
        let mut units = 0;
        let mut offset = start;
        for char in text[start..line_end].chars() {
            let length = char.len_utf16();
            if units + length > character {
                break;
            }
            units += length;
            offset += char.len_utf8();
        }
        offset
    }

    /// tsgo `FromLSPRange` for a file without a span map.
    pub fn from_lsp_range(&self, script: &dyn Script, range: &tsc_lsproto::Range) -> Range<usize> {
        self.from_lsp_position(script, &range.start)..self.from_lsp_position(script, &range.end)
    }
}

/// tsgo `LanguageKindToScriptKind`.
pub fn language_kind_to_script_kind(language: &LanguageKind) -> ScriptKind {
    match language.0.as_ref() {
        "typescript" => ScriptKind::Ts,
        "typescriptreact" => ScriptKind::Tsx,
        "javascript" => ScriptKind::Js,
        "javascriptreact" => ScriptKind::Jsx,
        "json" => ScriptKind::Json,
        _ => ScriptKind::Unknown,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/lsconv.rs"]
mod tests;

//! tsgo `ls/lsconv`: conversions between the protocol's positions (line and
//! character, in UTF-16 code units unless the client and the server agreed
//! on UTF-8) and offsets in a text (UTF-8 bytes), and from the client's
//! language ids.

mod diagnostics;
mod line_map;

use std::ops::Range;
use std::sync::Arc;

use tsc_diagnostics::spanmap::{Features, Fidelity, SpanMap};
use tsc_lsproto::{LanguageKind, Position, PositionEncodingKind};
use tsc_syntax::ScriptKind;

pub use diagnostics::{diagnostic_to_lsp, DiagnosticOptions, MappedScript, TextScript};
pub use line_map::LspLineMap;

/// tsgo `Script`: a file's name and text and, for a content mapper's
/// output, the span map from its text (the virtual text) back to its
/// original file's text, in UTF-8 bytes.
pub trait Script {
    fn file_name(&self) -> &str;
    fn text(&self) -> &str;
    /// tsgo `OriginalFileName`: a supplemental output's canonical output's
    /// name, the file's own otherwise.
    fn original_file_name(&self) -> &str {
        self.file_name()
    }
    /// tsgo `SpanMap`: none for an ordinary file (and a failed output).
    fn span_map(&self) -> Option<&SpanMap> {
        None
    }
    /// tsgo `OriginalText`.
    fn original_text(&self) -> &str {
        self.text()
    }
}

/// tsgo `MappedPosition`: a projection into one of the scripts a
/// conversion was given, by its index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptPosition {
    pub script: usize,
    pub position: usize,
    pub fidelity: Fidelity,
}

/// tsgo `MappedSpan`: a projection into one of the scripts a conversion was
/// given, by its index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptSpan {
    pub script: usize,
    pub span: Range<usize>,
    pub fidelity: Fidelity,
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

    /// tsgo `positionToLineAndCharacter`: an offset's line and character in
    /// a file, the offset clamped to the text.
    fn position_to_line_and_character(
        &self,
        file_name: &str,
        text: &str,
        position: usize,
    ) -> Position {
        let position = position.min(text.len());
        let line_map = (self.line_map)(file_name);
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

    /// tsgo `ToLSPPosition`: an offset's line and character; a content
    /// mapper's output's offset maps to its original file.
    pub fn to_lsp_position(&self, script: &dyn Script, position: usize) -> Position {
        self.to_lsp_position_where(script, position, None).0
    }

    /// tsgo `ToLSPPositionForFeature`: [`Self::to_lsp_position`] with no
    /// fidelity where the output's segment does not take part in `feature`.
    pub fn to_lsp_position_for_feature(
        &self,
        script: &dyn Script,
        position: usize,
        feature: Features,
    ) -> (Position, Fidelity) {
        self.to_lsp_position_where(script, position, Some(feature))
    }

    /// tsgo `virtualPositionToOriginal` and `positionToLineAndCharacter`.
    fn to_lsp_position_where(
        &self,
        script: &dyn Script,
        position: usize,
        feature: Option<Features>,
    ) -> (Position, Fidelity) {
        let Some(span_map) = script.span_map() else {
            return (
                self.position_to_line_and_character(script.file_name(), script.text(), position),
                Fidelity::Exact,
            );
        };
        let (mapped, fidelity) = match feature {
            None => span_map.virtual_to_original_position(position as i32),
            Some(feature) => {
                span_map.virtual_to_original_position_for_feature(position as i32, feature)
            }
        };
        (
            self.position_to_line_and_character(
                script.original_file_name(),
                script.original_text(),
                mapped as usize,
            ),
            fidelity,
        )
    }

    /// tsgo `ToLSPRange`: a range's lines and characters; a content mapper's
    /// output's range maps to its original file.
    pub fn to_lsp_range(&self, script: &dyn Script, range: Range<usize>) -> tsc_lsproto::Range {
        self.to_lsp_range_where(script, range, None).0
    }

    /// tsgo `ToLSPRange` with the fidelity of its mapping.
    pub fn to_lsp_range_with_fidelity(
        &self,
        script: &dyn Script,
        range: Range<usize>,
    ) -> (tsc_lsproto::Range, Fidelity) {
        self.to_lsp_range_where(script, range, None)
    }

    /// tsgo `ToLSPRangeForFeature`: [`Self::to_lsp_range`] with no fidelity
    /// unless contiguous segments taking part in `feature` cover the range.
    pub fn to_lsp_range_for_feature(
        &self,
        script: &dyn Script,
        range: Range<usize>,
        feature: Features,
    ) -> (tsc_lsproto::Range, Fidelity) {
        self.to_lsp_range_where(script, range, Some(feature))
    }

    /// tsgo `virtualRangeToOriginal` and `positionToLineAndCharacter`.
    fn to_lsp_range_where(
        &self,
        script: &dyn Script,
        range: Range<usize>,
        feature: Option<Features>,
    ) -> (tsc_lsproto::Range, Fidelity) {
        let Some(span_map) = script.span_map() else {
            let (file_name, text) = (script.file_name(), script.text());
            return (
                tsc_lsproto::Range {
                    start: self.position_to_line_and_character(file_name, text, range.start),
                    end: self.position_to_line_and_character(file_name, text, range.end),
                },
                Fidelity::Exact,
            );
        };
        let ((start, end), fidelity) = match feature {
            None => span_map.virtual_to_original_span(range.start as i32, range.end as i32),
            Some(feature) => span_map.virtual_to_original_span_for_feature(
                range.start as i32,
                range.end as i32,
                feature,
            ),
        };
        let (file_name, text) = (script.original_file_name(), script.original_text());
        (
            tsc_lsproto::Range {
                start: self.position_to_line_and_character(file_name, text, start as usize),
                end: self.position_to_line_and_character(file_name, text, end as usize),
            },
            fidelity,
        )
    }

    /// tsgo `FromLSPPosition` over scripts (`lspPositionToVirtualForScripts`):
    /// the position in each script; in a content mapper's output, each
    /// projection of the original position whose segment takes part in
    /// `feature`.
    pub fn from_lsp_position_for_scripts(
        &self,
        scripts: &[&dyn Script],
        position: &Position,
        feature: Features,
    ) -> Vec<ScriptPosition> {
        let mut projections = Vec::new();
        for (index, script) in scripts.iter().enumerate() {
            let Some(span_map) = script.span_map() else {
                projections.push(ScriptPosition {
                    script: index,
                    position: self.line_and_character_to_position(
                        script.file_name(),
                        script.text(),
                        position,
                    ),
                    fidelity: Fidelity::Exact,
                });
                continue;
            };
            let original = self.line_and_character_to_position(
                script.original_file_name(),
                script.original_text(),
                position,
            );
            for mapped in span_map.original_to_virtual_positions(original as i32, feature) {
                projections.push(ScriptPosition {
                    script: index,
                    position: mapped.position as usize,
                    fidelity: mapped.fidelity,
                });
            }
        }
        projections
    }

    /// tsgo `FromLSPRange` over scripts (`lspRangeToVirtualForScripts`): the
    /// range in each script; in a content mapper's output, each projection
    /// of the original range covered by segments that take part in
    /// `feature`.
    pub fn from_lsp_range_for_scripts(
        &self,
        scripts: &[&dyn Script],
        range: &tsc_lsproto::Range,
        feature: Features,
    ) -> Vec<ScriptSpan> {
        let mut projections = Vec::new();
        for (index, script) in scripts.iter().enumerate() {
            let Some(span_map) = script.span_map() else {
                let (file_name, text) = (script.file_name(), script.text());
                projections.push(ScriptSpan {
                    script: index,
                    span: self.line_and_character_to_position(file_name, text, &range.start)
                        ..self.line_and_character_to_position(file_name, text, &range.end),
                    fidelity: Fidelity::Exact,
                });
                continue;
            };
            let original = self.from_lsp_range_to_original(*script, range);
            for mapped in span_map.original_to_virtual_spans(
                original.start as i32,
                original.end as i32,
                feature,
            ) {
                projections.push(ScriptSpan {
                    script: index,
                    span: mapped.pos as usize..mapped.end as usize,
                    fidelity: mapped.fidelity,
                });
            }
        }
        projections
    }

    /// tsgo `FromLSPRangeIntersectingForSourceFile`: in each script, the
    /// parts of the range in segments that take part in `feature` (a range
    /// over host markup keeps the script inside it); an ordinary script's
    /// whole range.
    pub fn from_lsp_range_intersecting_for_scripts(
        &self,
        scripts: &[&dyn Script],
        range: &tsc_lsproto::Range,
        feature: Features,
    ) -> Vec<ScriptSpan> {
        let mut projections = Vec::new();
        for (index, script) in scripts.iter().enumerate() {
            let Some(span_map) = script.span_map() else {
                let (file_name, text) = (script.file_name(), script.text());
                projections.push(ScriptSpan {
                    script: index,
                    span: self.line_and_character_to_position(file_name, text, &range.start)
                        ..self.line_and_character_to_position(file_name, text, &range.end),
                    fidelity: Fidelity::Exact,
                });
                continue;
            };
            let original = self.from_lsp_range_to_original(*script, range);
            for mapped in span_map.original_to_virtual_intersecting_spans(
                original.start as i32,
                original.end as i32,
                feature,
            ) {
                projections.push(ScriptSpan {
                    script: index,
                    span: mapped.pos as usize..mapped.end as usize,
                    fidelity: mapped.fidelity,
                });
            }
        }
        projections
    }

    /// tsgo `FromLSPRangeToOriginal`: a range of a content mapper's
    /// output's document as offsets of its original text.
    pub fn from_lsp_range_to_original(
        &self,
        script: &dyn Script,
        range: &tsc_lsproto::Range,
    ) -> Range<usize> {
        let (file_name, text) = (script.original_file_name(), script.original_text());
        self.line_and_character_to_position(file_name, text, &range.start)
            ..self.line_and_character_to_position(file_name, text, &range.end)
    }

    /// tsgo `lineAndCharacterToPosition` of a script without a span map: a
    /// line and character's offset; a line past the end is the text's end,
    /// a character past its line the line's end.
    pub fn from_lsp_position(&self, script: &dyn Script, position: &Position) -> usize {
        self.line_and_character_to_position(script.file_name(), script.text(), position)
    }

    /// tsgo `lineAndCharacterToPosition`.
    fn line_and_character_to_position(
        &self,
        file_name: &str,
        text: &str,
        position: &Position,
    ) -> usize {
        let line_map = (self.line_map)(file_name);
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

    /// tsgo `FromLSPRange` for a script without a span map.
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

/// tsgo `lsproto.CompareRanges`: by start, then end.
pub(crate) fn compare_ranges(
    left: &tsc_lsproto::Range,
    right: &tsc_lsproto::Range,
) -> std::cmp::Ordering {
    let position = |position: &tsc_lsproto::Position| (position.line, position.character);
    position(&left.start)
        .cmp(&position(&right.start))
        .then_with(|| position(&left.end).cmp(&position(&right.end)))
}

use std::borrow::Cow;
use tsc_syntax::is_whitespace_like;

use crate::{GeneratedUtf16Location, GeneratedUtf16Position};

const INDENT: &str = "    ";

/// Generated JavaScript text and its UTF8 sink projection. The ordinary path
/// owns only a String. Once an unpaired unit occurs, the complete original
/// UTF16 sequence becomes authoritative; replacement characters belong only
/// to the UTF8 view.
#[derive(Clone, Debug, Default, Eq)]
pub(crate) struct GeneratedText {
    utf8: String,
    utf16: Option<Vec<u16>>,
}

impl PartialEq for GeneratedText {
    fn eq(&self, other: &Self) -> bool {
        match (&self.utf16, &other.utf16) {
            (None, None) => self.utf8 == other.utf8,
            (Some(left), Some(right)) => left == right,
            (Some(left), None) => left.iter().copied().eq(other.utf8.encode_utf16()),
            (None, Some(right)) => self.utf8.encode_utf16().eq(right.iter().copied()),
        }
    }
}

impl GeneratedText {
    pub(crate) fn as_str(&self) -> &str {
        &self.utf8
    }

    pub(crate) fn units(&self) -> Cow<'_, [u16]> {
        match &self.utf16 {
            Some(units) => Cow::Borrowed(units),
            None => Cow::Owned(self.utf8.encode_utf16().collect()),
        }
    }

    fn push_str(&mut self, text: &str) {
        self.utf8.push_str(text);
        if let Some(units) = &mut self.utf16 {
            units.extend(text.encode_utf16());
        }
    }

    fn push_utf16(&mut self, units: &[u16]) {
        if units.is_empty() {
            return;
        }
        if self.utf16.is_none() && char::decode_utf16(units.iter().copied()).any(|c| c.is_err()) {
            self.utf16 = Some(self.utf8.encode_utf16().collect());
        }
        let mut projection_start = 0;
        if let Some(previous) = self.utf16.as_ref().and_then(|text| text.last()).copied() {
            if (0xd800..=0xdbff).contains(&previous) && (0xdc00..=0xdfff).contains(&units[0]) {
                // Concatenation can complete a pair across two writes. Replace
                // only the projection of the previously unpaired final unit.
                assert_eq!(self.utf8.pop(), Some(char::REPLACEMENT_CHARACTER));
                let scalar = 0x10000
                    + ((u32::from(previous) - 0xd800) << 10)
                    + (u32::from(units[0]) - 0xdc00);
                self.utf8
                    .push(char::from_u32(scalar).expect("validated surrogate pair"));
                projection_start = 1;
            }
        }
        self.utf8.extend(
            char::decode_utf16(units[projection_start..].iter().copied())
                .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER)),
        );
        if let Some(text) = &mut self.utf16 {
            text.extend_from_slice(units);
        }
    }

    /// The System helper owner obtains this byte boundary by searching ASCII
    /// delimiters in the UTF8 view. Such a boundary also identifies a complete
    /// UTF16 prefix, even when that prefix contains replacement projections.
    pub(crate) fn insert_at_utf8_boundary(&mut self, offset: usize, inserted: &Self) {
        let prefix = &self.utf8[..offset];
        if self.utf16.is_none() && inserted.utf16.is_none() {
            self.utf8.insert_str(offset, inserted.as_str());
            return;
        }
        let unit_offset = prefix.encode_utf16().count();
        let text = self
            .utf16
            .get_or_insert_with(|| self.utf8.encode_utf16().collect());
        text.splice(unit_offset..unit_offset, inserted.units().iter().copied());
        self.utf8 = String::from_utf16_lossy(text);
    }

    fn clear(&mut self) {
        self.utf8.clear();
        self.utf16 = None;
    }
}

/// TypeScript's public `NewLineKind` values used by the H1 printer.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum NewLineKind {
    CarriageReturnLineFeed,
    #[default]
    LineFeed,
}

impl NewLineKind {
    pub const fn text(self) -> &'static str {
        match self {
            Self::CarriageReturnLineFeed => "\r\n",
            Self::LineFeed => "\n",
        }
    }
}

/// Generated JavaScript text with UTF16 coordinates and a UTF8 sink view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextWriter {
    output: GeneratedText,
    new_line: &'static str,
    single_line: bool,
    indent: u32,
    line_start: bool,
    line_count: u32,
    line_position: GeneratedUtf16Position,
    text_position: GeneratedUtf16Position,
    has_trailing_comment: bool,
    recording: Option<crate::source_map::SourceMapRecording>,
}

/// tsc-port: createTextWriter @6.0.3
/// tsc-hash: 468df403cf6a10a3b3c1349c60309814fbfaf24cca610b0f6aac30fb6952bd84
/// tsc-span: _tsc.js:16365-16461
pub fn create_text_writer(new_line: NewLineKind) -> TextWriter {
    TextWriter::new(new_line)
}

impl TextWriter {
    pub(crate) const fn indent_size() -> usize {
        INDENT.len()
    }

    fn new(new_line: NewLineKind) -> Self {
        Self {
            output: GeneratedText::default(),
            new_line: new_line.text(),
            single_line: false,
            indent: 0,
            line_start: true,
            line_count: 0,
            line_position: GeneratedUtf16Position::new(0),
            text_position: GeneratedUtf16Position::new(0),
            has_trailing_comment: false,
            recording: None,
        }
    }

    /// tsc-port: createSingleLineStringWriter @6.0.3
    /// tsc-hash: d67a4fec077149442ca7a65ee5cc194c09ac015d8ce8998b8c8977bbbc1ef5ac
    /// tsc-span: _tsc.js:12672-12703
    pub fn single_line() -> Self {
        Self {
            output: GeneratedText::default(),
            new_line: " ",
            single_line: true,
            indent: 0,
            line_start: false,
            line_count: 0,
            line_position: GeneratedUtf16Position::new(0),
            text_position: GeneratedUtf16Position::new(0),
            has_trailing_comment: false,
            recording: None,
        }
    }

    /// h2-6a-m-2 §4: the print-lifetime recording rides the transformed
    /// arm's main writer; probe/re-measure writers are fresh
    /// constructions and never carry one.
    pub(crate) fn set_source_map_recording(
        &mut self,
        recording: Option<crate::source_map::SourceMapRecording>,
    ) {
        self.recording = recording;
    }

    pub(crate) fn take_source_map_recording(
        &mut self,
    ) -> Option<crate::source_map::SourceMapRecording> {
        self.recording.take()
    }

    pub(crate) fn has_source_map_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub(crate) fn recording_mut(&mut self) -> Option<&mut crate::source_map::SourceMapRecording> {
        self.recording.as_mut()
    }

    /// Record a mapping against the CURRENT print source (the comment
    /// lane and every default-range site of the printed file).
    pub(crate) fn record_source_map_position(&mut self, source_line: u32, source_character: u32) {
        let generated_line = self.line();
        let generated_character = self.column();
        if let Some(recording) = self.recording.as_mut() {
            recording.record_current(
                source_line,
                source_character,
                generated_line,
                generated_character,
            );
        }
    }

    /// Record a mapping against an explicit (possibly foreign) source.
    pub(crate) fn record_source_map_position_for(
        &mut self,
        source: crate::TransformSourceId,
        file_name: &str,
        source_line: u32,
        source_character: u32,
    ) {
        let generated_line = self.line();
        let generated_character = self.column();
        if let Some(recording) = self.recording.as_mut() {
            recording.record_for_source(
                source,
                file_name,
                source_line,
                source_character,
                generated_line,
                generated_character,
            );
        }
    }

    fn write_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.single_line {
            self.append_single_line(text);
            return;
        }
        self.write_pending_indent();
        self.append_and_measure(text);
    }

    fn write_pending_indent(&mut self) {
        if self.line_start {
            let indentation = INDENT.repeat(self.indent as usize);
            self.append_and_measure(&indentation);
            self.line_start = false;
        }
    }

    fn write_text_utf16(&mut self, units: &[u16]) {
        if units.is_empty() {
            return;
        }
        if self.single_line {
            self.append_single_line_utf16(units);
            return;
        }
        self.write_pending_indent();
        self.append_and_measure_utf16(units);
    }

    fn append_single_line_utf16(&mut self, units: &[u16]) {
        self.output.push_utf16(units);
        let length = u32::try_from(units.len())
            .expect("emitted text length exceeds the UTF-16 position domain");
        self.text_position = self
            .text_position
            .checked_add(length)
            .expect("emitted text position overflowed");
    }

    fn append_single_line(&mut self, text: &str) {
        self.output.push_str(text);
        let text_utf16_length = u32::try_from(text.encode_utf16().count())
            .expect("emitted text length exceeds the UTF-16 position domain");
        self.text_position = self
            .text_position
            .checked_add(text_utf16_length)
            .expect("emitted text position overflowed");
    }

    fn append_and_measure(&mut self, text: &str) {
        self.output.push_str(text);
        self.measure_chunk(text.encode_utf16());
    }

    fn append_and_measure_utf16(&mut self, units: &[u16]) {
        self.output.push_utf16(units);
        self.measure_chunk(units.iter().copied());
    }

    /// computeLineStarts @6.0.3, _tsc.js:8250-8277
    /// SHA256: 147e073a0b43a88a9f85025067b10b6b4ce1f11b28549970fca1e71eee8d93e8
    /// updateLineCountAndPosFor consumes only the number of lines and final
    /// start. Both input encodings use the same per-chunk UTF16 measurement;
    /// a CR and LF in different writes each increment the line count.
    fn measure_chunk(&mut self, units: impl Iterator<Item = u16>) {
        let start = self.text_position;
        let mut units = units.peekable();
        let mut length = 0u32;
        let mut added_lines = 0u32;
        let mut last_start = 0u32;
        while let Some(unit) = units.next() {
            length = length
                .checked_add(1)
                .expect("emitted text length exceeds the UTF-16 position domain");
            if unit == 13 && units.peek() == Some(&10) {
                units.next();
                length = length
                    .checked_add(1)
                    .expect("emitted text length exceeds the UTF-16 position domain");
            }
            if matches!(unit, 10 | 13 | 0x2028 | 0x2029) {
                added_lines = added_lines
                    .checked_add(1)
                    .expect("emitted line count exceeds u32");
                last_start = length;
            }
        }
        self.text_position = start
            .checked_add(length)
            .expect("emitted text position overflowed");
        if added_lines > 0 {
            self.line_count = self
                .line_count
                .checked_add(added_lines)
                .expect("emitted line count overflowed");
            self.line_position = start
                .checked_add(last_start)
                .expect("emitted line position overflowed");
            self.line_start = self.line_position == self.text_position;
        } else {
            self.line_start = false;
        }
    }

    /// Write JavaScript units without replacing unpaired surrogates.
    pub fn write_utf16(&mut self, units: &[u16]) {
        if !units.is_empty() {
            self.has_trailing_comment = false;
        }
        self.write_text_utf16(units);
    }

    pub fn raw_write_utf16(&mut self, units: &[u16]) {
        if self.single_line {
            self.append_single_line_utf16(units);
            return;
        }
        self.append_and_measure_utf16(units);
        self.has_trailing_comment = false;
    }

    pub fn write_literal_utf16(&mut self, units: &[u16]) {
        if !units.is_empty() {
            self.write_utf16(units);
        }
    }

    pub fn write_comment_utf16(&mut self, units: &[u16]) {
        if !self.single_line && !units.is_empty() {
            self.has_trailing_comment = true;
        }
        self.write_text_utf16(units);
    }

    pub fn write_string_literal_utf16(&mut self, units: &[u16]) {
        self.write_utf16(units);
    }

    pub fn write(&mut self, text: &str) {
        if !text.is_empty() {
            self.has_trailing_comment = false;
        }
        self.write_text(text);
    }

    /// Append without applying pending indentation. Unlike `write`, an empty
    /// raw write still leaves the writer off the start-of-line state, matching
    /// the defined-string branch in TypeScript's writer.
    pub fn raw_write(&mut self, text: &str) {
        if self.single_line {
            self.append_single_line(text);
            return;
        }
        if text.is_empty() {
            self.line_start = false;
        } else {
            self.append_and_measure(text);
        }
        self.has_trailing_comment = false;
    }

    pub fn write_literal(&mut self, text: &str) {
        if !text.is_empty() {
            self.write(text);
        }
    }

    pub fn write_comment(&mut self, text: &str) {
        if !self.single_line && !text.is_empty() {
            self.has_trailing_comment = true;
        }
        self.write_text(text);
    }

    pub fn write_line(&mut self, force: bool) {
        if self.single_line {
            self.append_single_line(" ");
            return;
        }
        if !self.line_start || force {
            self.output.push_str(self.new_line);
            let new_line_length = u32::try_from(self.new_line.encode_utf16().count())
                .expect("configured newline exceeds u32");
            self.text_position = self
                .text_position
                .checked_add(new_line_length)
                .expect("emitted text position overflowed");
            self.line_count = self
                .line_count
                .checked_add(1)
                .expect("emitted line count overflowed");
            self.line_position = self.text_position;
            self.line_start = true;
            self.has_trailing_comment = false;
        }
    }

    pub fn increase_indent(&mut self) {
        if self.single_line {
            return;
        }
        self.indent = self
            .indent
            .checked_add(1)
            .expect("writer indent overflowed");
    }

    pub fn decrease_indent(&mut self) {
        if self.single_line {
            return;
        }
        self.indent = self
            .indent
            .checked_sub(1)
            .expect("writer indent underflowed");
    }

    pub const fn indent(&self) -> u32 {
        self.indent
    }

    pub const fn text_position(&self) -> GeneratedUtf16Position {
        self.text_position
    }

    pub const fn line(&self) -> u32 {
        self.line_count
    }

    pub fn column(&self) -> u32 {
        if self.single_line {
            return 0;
        }
        if self.line_start {
            self.indent
                .checked_mul(INDENT.len() as u32)
                .expect("writer column overflowed")
        } else {
            self.text_position
                .value()
                .checked_sub(self.line_position.value())
                .expect("writer line position exceeds text position")
        }
    }

    pub fn location(&self) -> GeneratedUtf16Location {
        GeneratedUtf16Location::new(self.text_position, self.line(), self.column())
    }

    /// UTF8 sink projection; an unpaired UTF16 unit projects as U+FFFD.
    pub fn text(&self) -> &str {
        self.output.as_str()
    }

    /// Actual generated JavaScript units, including unpaired surrogates.
    pub fn text_utf16(&self) -> Cow<'_, [u16]> {
        self.output.units()
    }

    pub(crate) fn generated_text(&self) -> &GeneratedText {
        &self.output
    }

    pub const fn is_at_start_of_line(&self) -> bool {
        !self.single_line && self.line_start
    }

    pub const fn has_trailing_comment(&self) -> bool {
        !self.single_line && self.has_trailing_comment
    }

    pub fn has_trailing_whitespace(&self) -> bool {
        self.output
            .as_str()
            .chars()
            .next_back()
            .is_some_and(is_whitespace_like)
    }

    pub fn clear(&mut self) {
        self.output.clear();
        self.indent = 0;
        self.line_start = !self.single_line;
        self.line_count = 0;
        self.line_position = GeneratedUtf16Position::new(0);
        self.text_position = GeneratedUtf16Position::new(0);
        self.has_trailing_comment = false;
    }

    pub fn write_keyword(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_operator(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_parameter(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_property(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_punctuation(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_space(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_string_literal(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_symbol(&mut self, text: &str) {
        self.write(text);
    }

    pub fn write_trailing_semicolon(&mut self, text: &str) {
        self.write(text);
    }
}

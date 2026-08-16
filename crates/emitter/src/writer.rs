use tsc_diagnostics::compute_line_starts;
use tsc_syntax::is_whitespace_like;

use crate::{GeneratedUtf16Location, GeneratedUtf16Position};

const INDENT: &str = "    ";

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

/// UTF-8 text storage with independently maintained JavaScript/UTF-16 writer
/// coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextWriter {
    output: String,
    new_line: &'static str,
    indent: u32,
    line_start: bool,
    line_count: u32,
    line_position: GeneratedUtf16Position,
    text_position: GeneratedUtf16Position,
    has_trailing_comment: bool,
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
            output: String::new(),
            new_line: new_line.text(),
            indent: 0,
            line_start: true,
            line_count: 0,
            line_position: GeneratedUtf16Position::new(0),
            text_position: GeneratedUtf16Position::new(0),
            has_trailing_comment: false,
        }
    }

    fn write_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.line_start {
            let indentation = INDENT.repeat(self.indent as usize);
            self.append_and_measure(&indentation);
            self.line_start = false;
        }
        self.append_and_measure(text);
    }

    fn append_and_measure(&mut self, text: &str) {
        let start = self.text_position;
        self.output.push_str(text);
        let text_utf16_length = u32::try_from(text.encode_utf16().count())
            .expect("emitted text length exceeds the UTF-16 position domain");
        self.text_position = start
            .checked_add(text_utf16_length)
            .expect("emitted text position overflowed");

        let line_starts = compute_line_starts(text);
        if line_starts.len() > 1 {
            let added_lines =
                u32::try_from(line_starts.len() - 1).expect("emitted line count exceeds u32");
            self.line_count = self
                .line_count
                .checked_add(added_lines)
                .expect("emitted line count overflowed");
            let last_start = *line_starts
                .last()
                .expect("compute_line_starts always returns one entry");
            self.line_position = start
                .checked_add(last_start)
                .expect("emitted line position overflowed");
            self.line_start = self.line_position == self.text_position;
        } else {
            self.line_start = false;
        }
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
        if !text.is_empty() {
            self.has_trailing_comment = true;
        }
        self.write_text(text);
    }

    pub fn write_line(&mut self, force: bool) {
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
        self.indent = self
            .indent
            .checked_add(1)
            .expect("writer indent overflowed");
    }

    pub fn decrease_indent(&mut self) {
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

    pub fn text(&self) -> &str {
        &self.output
    }

    pub const fn is_at_start_of_line(&self) -> bool {
        self.line_start
    }

    pub const fn has_trailing_comment(&self) -> bool {
        self.has_trailing_comment
    }

    pub fn has_trailing_whitespace(&self) -> bool {
        self.output
            .chars()
            .next_back()
            .is_some_and(is_whitespace_like)
    }

    pub fn clear(&mut self) {
        self.output.clear();
        self.indent = 0;
        self.line_start = true;
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

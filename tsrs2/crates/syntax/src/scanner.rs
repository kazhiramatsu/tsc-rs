use crate::SyntaxKind;
use tsrs2_diags::{gen, DiagnosticMessage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageVariant {
    Standard,
    Jsx,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenRecord {
    pub kind: SyntaxKind,
    pub start: u32,
    pub end: u32,
    pub preceding_line_break: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Utf16OffsetMap {
    offsets: Vec<(usize, u32)>,
}

impl Utf16OffsetMap {
    fn new(text: &str) -> Self {
        let mut offsets = Vec::with_capacity(text.chars().count() + 1);
        let mut utf16_offset = 0;

        for (byte_offset, ch) in text.char_indices() {
            offsets.push((byte_offset, utf16_offset));
            utf16_offset += ch.len_utf16() as u32;
        }
        offsets.push((text.len(), utf16_offset));

        Self { offsets }
    }

    fn byte_to_utf16(&self, byte_offset: usize) -> u32 {
        self.offsets
            .binary_search_by_key(&byte_offset, |(candidate, _)| *candidate)
            .map(|index| self.offsets[index].1)
            .expect("scanner positions are valid UTF-8 boundaries")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScanError {
    message: &'static DiagnosticMessage,
    start: usize,
    length: usize,
    args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TokenFlags(u32);

impl TokenFlags {
    const PRECEDING_LINE_BREAK: Self = Self(1);

    const fn empty() -> Self {
        Self(0)
    }

    const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
struct ScannerState {
    pos: usize,
    full_start_pos: usize,
    token_start: usize,
    token: SyntaxKind,
    token_value: String,
    token_flags: TokenFlags,
    error_len: usize,
}

struct Scanner<'text> {
    text: &'text str,
    end: usize,
    pos: usize,
    full_start_pos: usize,
    token_start: usize,
    token: SyntaxKind,
    token_value: String,
    token_flags: TokenFlags,
    _language_variant: LanguageVariant,
    errors: Vec<ScanError>,
}

impl<'text> Scanner<'text> {
    fn new(text: &'text str, language_variant: LanguageVariant) -> Self {
        Self {
            text,
            end: text.len(),
            pos: 0,
            full_start_pos: 0,
            token_start: 0,
            token: SyntaxKind::Unknown,
            token_value: String::new(),
            token_flags: TokenFlags::empty(),
            _language_variant: language_variant,
            errors: Vec::new(),
        }
    }

    fn scan(&mut self) -> SyntaxKind {
        self.full_start_pos = self.pos;
        self.token_flags = TokenFlags::empty();
        self.token_value.clear();

        loop {
            self.token_start = self.pos;
            let Some(ch) = self.current_char() else {
                self.token = SyntaxKind::EndOfFileToken;
                return self.token;
            };

            if self.pos == 0 && self.starts_with("#!") {
                self.scan_shebang_trivia();
                continue;
            }

            if is_line_break(ch) {
                self.token_flags.insert(TokenFlags::PRECEDING_LINE_BREAK);
                self.advance_char();
                continue;
            }

            if is_single_line_whitespace(ch) {
                self.advance_char();
                continue;
            }

            if self.starts_with("//") {
                self.skip_single_line_comment();
                continue;
            }

            if self.starts_with("/*") {
                self.skip_multi_line_comment();
                continue;
            }

            self.advance_char();
            self.token = SyntaxKind::Unknown;
            return self.token;
        }
    }

    #[allow(dead_code)]
    fn save(&self) -> ScannerState {
        ScannerState {
            pos: self.pos,
            full_start_pos: self.full_start_pos,
            token_start: self.token_start,
            token: self.token,
            token_value: self.token_value.clone(),
            token_flags: self.token_flags,
            error_len: self.errors.len(),
        }
    }

    #[allow(dead_code)]
    fn restore(&mut self, state: ScannerState) {
        self.pos = state.pos;
        self.full_start_pos = state.full_start_pos;
        self.token_start = state.token_start;
        self.token = state.token;
        self.token_value = state.token_value;
        self.token_flags = state.token_flags;
        self.errors.truncate(state.error_len);
    }

    fn pos(&self) -> usize {
        self.pos
    }

    fn token_start(&self) -> usize {
        self.token_start
    }

    fn has_preceding_line_break(&self) -> bool {
        self.token_flags.contains(TokenFlags::PRECEDING_LINE_BREAK)
    }

    #[allow(dead_code)]
    fn errors(&self) -> &[ScanError] {
        &self.errors
    }

    fn scan_shebang_trivia(&mut self) {
        while let Some(ch) = self.current_char() {
            if is_line_break(ch) {
                break;
            }
            self.advance_char();
        }
    }

    fn skip_single_line_comment(&mut self) {
        self.pos += 2;
        while let Some(ch) = self.current_char() {
            if is_line_break(ch) {
                break;
            }
            self.advance_char();
        }
    }

    fn skip_multi_line_comment(&mut self) {
        let start = self.token_start;
        self.pos += 2;

        while self.pos < self.end {
            if self.starts_with("*/") {
                self.pos += 2;
                return;
            }

            let ch = self
                .current_char()
                .expect("position before end has a UTF-8 scalar");
            self.advance_char();
            if is_line_break(ch) {
                self.token_flags.insert(TokenFlags::PRECEDING_LINE_BREAK);
            }
        }

        self.error_at(start, self.end.saturating_sub(start), &gen::expected);
    }

    fn error_at(&mut self, start: usize, length: usize, message: &'static DiagnosticMessage) {
        self.errors.push(ScanError {
            message,
            start,
            length,
            args: Vec::new(),
        });
    }

    fn current_char(&self) -> Option<char> {
        self.text.get(self.pos..)?.chars().next()
    }

    fn advance_char(&mut self) {
        let ch = self
            .current_char()
            .expect("advance_char requires a current character");
        self.pos += ch.len_utf8();
    }

    fn starts_with(&self, needle: &str) -> bool {
        self.text[self.pos..].starts_with(needle)
    }
}

pub fn scan_tokens(text: &str, variant: LanguageVariant) -> Vec<TokenRecord> {
    let mut scanner = Scanner::new(text, variant);
    let offset_map = Utf16OffsetMap::new(text);
    let mut tokens = Vec::new();

    loop {
        let kind = scanner.scan();
        if kind == SyntaxKind::EndOfFileToken {
            break;
        }

        tokens.push(TokenRecord {
            kind,
            start: offset_map.byte_to_utf16(scanner.token_start()),
            end: offset_map.byte_to_utf16(scanner.pos()),
            preceding_line_break: scanner.has_preceding_line_break(),
        });
    }

    tokens
}

fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_single_line_whitespace(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200B}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_only_input_has_no_tokens() {
        assert_eq!(
            scan_tokens("// line\n/* block */\n", LanguageVariant::Standard),
            Vec::new()
        );
    }

    #[test]
    fn token_after_trivia_gets_preceding_line_break_flag() {
        assert_eq!(
            scan_tokens("// line\nx", LanguageVariant::Standard),
            vec![TokenRecord {
                kind: SyntaxKind::Unknown,
                start: 8,
                end: 9,
                preceding_line_break: true,
            }]
        );
    }

    #[test]
    fn shebang_at_start_is_trivia() {
        assert_eq!(
            scan_tokens("#!/usr/bin/env node\n", LanguageVariant::Standard),
            Vec::new()
        );
    }

    #[test]
    fn dump_positions_are_utf16_offsets() {
        assert_eq!(
            scan_tokens("/* \u{1f600} */x", LanguageVariant::Standard),
            vec![TokenRecord {
                kind: SyntaxKind::Unknown,
                start: 8,
                end: 9,
                preceding_line_break: false,
            }]
        );
    }

    #[test]
    fn unterminated_block_comment_reports_1010() {
        let mut scanner = Scanner::new("/* unterminated", LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::EndOfFileToken);

        assert_eq!(scanner.errors().len(), 1);
        assert_eq!(scanner.errors()[0].message.code, 1010);
        assert_eq!(scanner.errors()[0].start, 0);
        assert_eq!(scanner.errors()[0].length, "/* unterminated".len());
    }

    #[test]
    fn save_restore_rewinds_position_and_errors() {
        let mut scanner = Scanner::new("/* unterminated", LanguageVariant::Standard);
        let saved = scanner.save();

        assert_eq!(scanner.scan(), SyntaxKind::EndOfFileToken);
        assert_eq!(scanner.errors().len(), 1);

        scanner.restore(saved);

        assert_eq!(scanner.pos(), 0);
        assert_eq!(scanner.errors(), &[]);
    }
}

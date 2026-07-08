use crate::{chars, keywords, SyntaxKind};
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
    const UNTERMINATED: Self = Self(4);
    const EXTENDED_UNICODE_ESCAPE: Self = Self(8);
    const UNICODE_ESCAPE: Self = Self(1024);
    const CONTAINS_INVALID_ESCAPE: Self = Self(2048);
    const HEX_ESCAPE: Self = Self(4096);

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
    language_variant: LanguageVariant,
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
            language_variant,
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

            match ch {
                '!' => {
                    if self.starts_with("!==") {
                        return self.finish_token(SyntaxKind::ExclamationEqualsEqualsToken, 3);
                    }
                    if self.starts_with("!=") {
                        return self.finish_token(SyntaxKind::ExclamationEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::ExclamationToken, 1);
                }
                '"' | '\'' => return self.scan_string_literal(),
                '`' => return self.scan_template_token(),
                '%' => {
                    if self.starts_with("%=") {
                        return self.finish_token(SyntaxKind::PercentEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::PercentToken, 1);
                }
                '&' => {
                    if self.starts_with("&&=") {
                        return self.finish_token(SyntaxKind::AmpersandAmpersandEqualsToken, 3);
                    }
                    if self.starts_with("&&") {
                        return self.finish_token(SyntaxKind::AmpersandAmpersandToken, 2);
                    }
                    if self.starts_with("&=") {
                        return self.finish_token(SyntaxKind::AmpersandEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::AmpersandToken, 1);
                }
                '(' => return self.finish_token(SyntaxKind::OpenParenToken, 1),
                ')' => return self.finish_token(SyntaxKind::CloseParenToken, 1),
                '*' => {
                    if self.starts_with("**=") {
                        return self.finish_token(SyntaxKind::AsteriskAsteriskEqualsToken, 3);
                    }
                    if self.starts_with("**") {
                        return self.finish_token(SyntaxKind::AsteriskAsteriskToken, 2);
                    }
                    if self.starts_with("*=") {
                        return self.finish_token(SyntaxKind::AsteriskEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::AsteriskToken, 1);
                }
                '+' => {
                    if self.starts_with("++") {
                        return self.finish_token(SyntaxKind::PlusPlusToken, 2);
                    }
                    if self.starts_with("+=") {
                        return self.finish_token(SyntaxKind::PlusEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::PlusToken, 1);
                }
                ',' => return self.finish_token(SyntaxKind::CommaToken, 1),
                '-' => {
                    if self.starts_with("--") {
                        return self.finish_token(SyntaxKind::MinusMinusToken, 2);
                    }
                    if self.starts_with("-=") {
                        return self.finish_token(SyntaxKind::MinusEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::MinusToken, 1);
                }
                '.' => {
                    if self.byte_at(self.pos + 1).is_some_and(is_ascii_digit) {
                        return self.scan_number_literal();
                    }
                    if self.starts_with("...") {
                        return self.finish_token(SyntaxKind::DotDotDotToken, 3);
                    }
                    return self.finish_token(SyntaxKind::DotToken, 1);
                }
                '/' => {
                    if self.starts_with("/=") {
                        return self.finish_token(SyntaxKind::SlashEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::SlashToken, 1);
                }
                '0'..='9' => return self.scan_number_literal(),
                ':' => return self.finish_token(SyntaxKind::ColonToken, 1),
                ';' => return self.finish_token(SyntaxKind::SemicolonToken, 1),
                '<' => {
                    if self.starts_with("<<=") {
                        return self.finish_token(SyntaxKind::LessThanLessThanEqualsToken, 3);
                    }
                    if self.starts_with("<<") {
                        return self.finish_token(SyntaxKind::LessThanLessThanToken, 2);
                    }
                    if self.starts_with("<=") {
                        return self.finish_token(SyntaxKind::LessThanEqualsToken, 2);
                    }
                    if self.language_variant == LanguageVariant::Jsx
                        && self.starts_with("</")
                        && !self.starts_with("</*")
                    {
                        return self.finish_token(SyntaxKind::LessThanSlashToken, 2);
                    }
                    return self.finish_token(SyntaxKind::LessThanToken, 1);
                }
                '=' => {
                    if self.starts_with("===") {
                        return self.finish_token(SyntaxKind::EqualsEqualsEqualsToken, 3);
                    }
                    if self.starts_with("==") {
                        return self.finish_token(SyntaxKind::EqualsEqualsToken, 2);
                    }
                    if self.starts_with("=>") {
                        return self.finish_token(SyntaxKind::EqualsGreaterThanToken, 2);
                    }
                    return self.finish_token(SyntaxKind::EqualsToken, 1);
                }
                '>' => return self.finish_token(SyntaxKind::GreaterThanToken, 1),
                '?' => {
                    if self.starts_with("?.")
                        && !self.byte_at(self.pos + 2).is_some_and(is_ascii_digit)
                    {
                        return self.finish_token(SyntaxKind::QuestionDotToken, 2);
                    }
                    if self.starts_with("??=") {
                        return self.finish_token(SyntaxKind::QuestionQuestionEqualsToken, 3);
                    }
                    if self.starts_with("??") {
                        return self.finish_token(SyntaxKind::QuestionQuestionToken, 2);
                    }
                    return self.finish_token(SyntaxKind::QuestionToken, 1);
                }
                '[' => return self.finish_token(SyntaxKind::OpenBracketToken, 1),
                ']' => return self.finish_token(SyntaxKind::CloseBracketToken, 1),
                '^' => {
                    if self.starts_with("^=") {
                        return self.finish_token(SyntaxKind::CaretEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::CaretToken, 1);
                }
                '{' => return self.finish_token(SyntaxKind::OpenBraceToken, 1),
                '|' => {
                    if self.starts_with("||=") {
                        return self.finish_token(SyntaxKind::BarBarEqualsToken, 3);
                    }
                    if self.starts_with("||") {
                        return self.finish_token(SyntaxKind::BarBarToken, 2);
                    }
                    if self.starts_with("|=") {
                        return self.finish_token(SyntaxKind::BarEqualsToken, 2);
                    }
                    return self.finish_token(SyntaxKind::BarToken, 1);
                }
                '}' => return self.finish_token(SyntaxKind::CloseBraceToken, 1),
                '~' => return self.finish_token(SyntaxKind::TildeToken, 1),
                '@' => return self.finish_token(SyntaxKind::AtToken, 1),
                '\\' => {
                    if let Some(kind) = self.scan_identifier_escape_start() {
                        return kind;
                    }
                }
                '#' => return self.scan_private_identifier(),
                '\u{fffd}' => {
                    self.pos = self.end;
                    self.token = SyntaxKind::NonTextFileMarkerTrivia;
                    return self.token;
                }
                _ => {
                    if chars::is_identifier_start(ch) {
                        return self.scan_identifier();
                    }
                }
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

    fn scan_identifier(&mut self) -> SyntaxKind {
        self.token_value.clear();
        let first = self
            .current_char()
            .expect("scan_identifier requires a current character");
        self.token_value.push(first);
        self.advance_char();
        self.scan_identifier_parts();
        self.finish_identifier_token()
    }

    fn scan_identifier_escape_start(&mut self) -> Option<SyntaxKind> {
        let start = self.pos;
        let ch = self.scan_unicode_escape()?;
        if !chars::is_identifier_start(ch) {
            self.pos = start;
            return None;
        }
        self.token_value.clear();
        self.token_value.push(ch);
        self.scan_identifier_parts();
        Some(self.finish_identifier_token())
    }

    fn scan_identifier_parts(&mut self) {
        while let Some(ch) = self.current_char() {
            if chars::is_identifier_part(ch) {
                self.token_value.push(ch);
                self.advance_char();
            } else if ch == '\\' {
                let start = self.pos;
                if let Some(ch) = self.scan_unicode_escape() {
                    if chars::is_identifier_part(ch) {
                        self.token_value.push(ch);
                        continue;
                    }
                }
                self.pos = start;
                break;
            } else {
                break;
            }
        }
    }

    fn finish_identifier_token(&mut self) -> SyntaxKind {
        self.token = keywords::keyword_kind(&self.token_value).unwrap_or(SyntaxKind::Identifier);
        self.token
    }

    fn scan_private_identifier(&mut self) -> SyntaxKind {
        self.pos += 1;
        self.token_value.clear();
        self.token_value.push('#');

        if let Some(ch) = self.current_char() {
            if chars::is_identifier_start(ch) {
                self.token_value.push(ch);
                self.advance_char();
                self.scan_identifier_parts();
            } else if ch == '\\' {
                let start = self.pos;
                if let Some(ch) = self.scan_unicode_escape() {
                    if chars::is_identifier_start(ch) {
                        self.token_value.push(ch);
                        self.scan_identifier_parts();
                    } else {
                        self.pos = start;
                    }
                }
            }
        }

        self.token = SyntaxKind::PrivateIdentifier;
        self.token
    }

    fn scan_unicode_escape(&mut self) -> Option<char> {
        if !self.starts_with("\\u") {
            return None;
        }

        let start = self.pos;
        self.pos += 2;

        let value = if self.starts_with("{") {
            self.pos += 1;
            let digits_start = self.pos;
            while self.current_char().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                self.advance_char();
            }
            if self.pos == digits_start || !self.starts_with("}") {
                self.pos = start;
                return None;
            }
            let value = u32::from_str_radix(&self.text[digits_start..self.pos], 16).ok()?;
            self.pos += 1;
            value
        } else {
            if self.pos + 4 > self.end {
                self.pos = start;
                return None;
            }
            let end = self.pos + 4;
            let digits = &self.text[self.pos..end];
            if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                self.pos = start;
                return None;
            }
            self.pos = end;
            u32::from_str_radix(digits, 16).ok()?
        };

        char::from_u32(value).or_else(|| {
            self.pos = start;
            None
        })
    }

    fn scan_string_literal(&mut self) -> SyntaxKind {
        let quote = self.current_char().expect("string literal starts at quote");
        self.advance_char();
        self.token_value.clear();
        let mut segment_start = self.pos;

        loop {
            let Some(ch) = self.current_char() else {
                self.token_value
                    .push_str(&self.text[segment_start..self.pos]);
                self.token_flags.insert(TokenFlags::UNTERMINATED);
                self.error_at(self.pos, 0, &gen::Unterminated_string_literal);
                break;
            };

            if ch == quote {
                self.token_value
                    .push_str(&self.text[segment_start..self.pos]);
                self.advance_char();
                break;
            }
            if ch == '\\' {
                self.token_value
                    .push_str(&self.text[segment_start..self.pos]);
                let escaped = self.scan_escape_sequence();
                self.token_value.push_str(&escaped);
                segment_start = self.pos;
                continue;
            }
            if matches!(ch, '\n' | '\r') {
                self.token_value
                    .push_str(&self.text[segment_start..self.pos]);
                self.token_flags.insert(TokenFlags::UNTERMINATED);
                self.error_at(self.pos, 0, &gen::Unterminated_string_literal);
                break;
            }
            self.advance_char();
        }

        self.token = SyntaxKind::StringLiteral;
        self.token
    }

    fn scan_escape_sequence(&mut self) -> String {
        let start = self.pos;
        self.pos += 1;
        if self.pos >= self.end {
            self.error_at(self.pos, 0, &gen::Unexpected_end_of_text);
            return String::new();
        }

        let ch = self
            .current_char()
            .expect("escape sequence has a character after backslash");
        self.advance_char();

        match ch {
            '0' => {
                if !self.current_char().is_some_and(|ch| ch.is_ascii_digit()) {
                    return "\0".to_owned();
                }
                self.scan_octal_escape(start, ch)
            }
            '1'..='7' => self.scan_octal_escape(start, ch),
            '8' | '9' => {
                self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
                self.error_at(
                    start,
                    self.pos - start,
                    &gen::Escape_sequence_0_is_not_allowed,
                );
                ch.to_string()
            }
            'b' => "\u{0008}".to_owned(),
            't' => "\t".to_owned(),
            'n' => "\n".to_owned(),
            'v' => "\u{000b}".to_owned(),
            'f' => "\u{000c}".to_owned(),
            'r' => "\r".to_owned(),
            '\'' => "'".to_owned(),
            '"' => "\"".to_owned(),
            'u' => {
                if self.starts_with("{") {
                    self.pos = start;
                    return self.scan_extended_unicode_escape(true);
                }
                let digits_start = self.pos;
                for _ in 0..4 {
                    if !self.current_char().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                        self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
                        self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected);
                        return self.text[start..self.pos].to_owned();
                    }
                    self.advance_char();
                }
                self.token_flags.insert(TokenFlags::UNICODE_ESCAPE);
                let value =
                    u32::from_str_radix(&self.text[digits_start..self.pos], 16).unwrap_or(0xfffd);
                utf16_encode_as_string(value)
            }
            'x' => {
                let digits_start = self.pos;
                for _ in 0..2 {
                    if !self.current_char().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                        self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
                        self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected);
                        return self.text[start..self.pos].to_owned();
                    }
                    self.advance_char();
                }
                self.token_flags.insert(TokenFlags::HEX_ESCAPE);
                let value =
                    u32::from_str_radix(&self.text[digits_start..self.pos], 16).unwrap_or(0xfffd);
                utf16_encode_as_string(value)
            }
            '\r' => {
                if self.current_char() == Some('\n') {
                    self.advance_char();
                }
                String::new()
            }
            '\n' | '\u{2028}' | '\u{2029}' => String::new(),
            _ => ch.to_string(),
        }
    }

    fn scan_octal_escape(&mut self, start: usize, first: char) -> String {
        if self.current_char().is_some_and(is_octal_digit) {
            self.advance_char();
        }
        if matches!(first, '0'..='3') && self.current_char().is_some_and(is_octal_digit) {
            self.advance_char();
        }

        self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
        self.error_at(
            start,
            self.pos - start,
            &gen::Octal_escape_sequences_are_not_allowed_Use_the_syntax_0,
        );
        let value = u32::from_str_radix(&self.text[start + 1..self.pos], 8).unwrap_or(0xfffd);
        utf16_encode_as_string(value)
    }

    fn scan_extended_unicode_escape(&mut self, report: bool) -> String {
        let start = self.pos;
        self.pos += 3;
        let escaped_start = self.pos;

        while self.current_char().is_some_and(|ch| ch.is_ascii_hexdigit()) {
            self.advance_char();
        }
        let value_text = &self.text[escaped_start..self.pos];
        let value = if value_text.is_empty() {
            None
        } else {
            u32::from_str_radix(value_text, 16).ok()
        };

        let mut invalid = false;
        if value.is_none() {
            if report {
                self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected);
            }
            invalid = true;
        } else if value.is_some_and(|value| value > 0x10ffff) {
            if report {
                self.error_at(
                    escaped_start,
                    self.pos - escaped_start,
                    &gen::An_extended_Unicode_escape_value_must_be_between_0x0_and_0x10FFFF_inclusive,
                );
            }
            invalid = true;
        }

        if self.pos >= self.end {
            if report {
                self.error_at(self.pos, 0, &gen::Unexpected_end_of_text);
            }
            invalid = true;
        } else if self.starts_with("}") {
            self.pos += 1;
        } else {
            if report {
                self.error_at(self.pos, 0, &gen::Unterminated_Unicode_escape_sequence);
            }
            invalid = true;
        }

        if invalid {
            self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
            return self.text[start..self.pos].to_owned();
        }

        self.token_flags.insert(TokenFlags::EXTENDED_UNICODE_ESCAPE);
        utf16_encode_as_string(value.expect("valid extended escape has a value"))
    }

    fn scan_template_token(&mut self) -> SyntaxKind {
        self.pos += 1;
        while self.pos < self.end {
            if self.starts_with("`") {
                self.pos += 1;
                self.token = SyntaxKind::NoSubstitutionTemplateLiteral;
                return self.token;
            }
            if self.starts_with("${") {
                self.pos += 2;
                self.token = SyntaxKind::TemplateHead;
                return self.token;
            }
            if self.starts_with("\\") {
                self.pos += 1;
                if self.current_char().is_some() {
                    self.advance_char();
                }
            } else {
                self.advance_char();
            }
        }
        self.token = SyntaxKind::NoSubstitutionTemplateLiteral;
        self.token
    }

    fn scan_number_literal(&mut self) -> SyntaxKind {
        if self.starts_with("0x") || self.starts_with("0X") {
            self.pos += 2;
            self.skip_while_ascii(|byte| byte.is_ascii_hexdigit() || byte == b'_');
            return self.finish_number_with_bigint_suffix();
        }
        if self.starts_with("0b") || self.starts_with("0B") {
            self.pos += 2;
            self.skip_while_ascii(|byte| matches!(byte, b'0' | b'1' | b'_'));
            return self.finish_number_with_bigint_suffix();
        }
        if self.starts_with("0o") || self.starts_with("0O") {
            self.pos += 2;
            self.skip_while_ascii(|byte| matches!(byte, b'0'..=b'7' | b'_'));
            return self.finish_number_with_bigint_suffix();
        }

        self.skip_while_ascii(|byte| byte.is_ascii_digit() || byte == b'_');
        if self.starts_with(".") {
            self.pos += 1;
            self.skip_while_ascii(|byte| byte.is_ascii_digit() || byte == b'_');
        }
        if matches!(self.byte_at(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.byte_at(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.skip_while_ascii(|byte| byte.is_ascii_digit() || byte == b'_');
        }

        self.finish_number_with_bigint_suffix()
    }

    fn finish_number_with_bigint_suffix(&mut self) -> SyntaxKind {
        if self.starts_with("n") {
            self.pos += 1;
            self.token = SyntaxKind::BigIntLiteral;
        } else {
            self.token = SyntaxKind::NumericLiteral;
        }
        self.token
    }

    fn finish_token(&mut self, kind: SyntaxKind, width: usize) -> SyntaxKind {
        self.pos += width;
        self.token = kind;
        kind
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

    fn byte_at(&self, pos: usize) -> Option<u8> {
        self.text.as_bytes().get(pos).copied()
    }

    fn skip_while_ascii(&mut self, predicate: impl Fn(u8) -> bool) {
        while self.byte_at(self.pos).is_some_and(&predicate) {
            self.pos += 1;
        }
    }
}

fn is_ascii_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

fn is_octal_digit(ch: char) -> bool {
    matches!(ch, '0'..='7')
}

fn utf16_encode_as_string(value: u32) -> String {
    char::from_u32(value)
        .unwrap_or(char::REPLACEMENT_CHARACTER)
        .to_string()
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
                kind: SyntaxKind::Identifier,
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
                kind: SyntaxKind::Identifier,
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

    #[test]
    fn scans_keywords_and_punctuation() {
        let tokens = scan_tokens(
            "class C { async m() { return x?.y ?? 1; } }",
            LanguageVariant::Standard,
        )
        .into_iter()
        .map(|token| token.kind)
        .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![
                SyntaxKind::ClassKeyword,
                SyntaxKind::Identifier,
                SyntaxKind::OpenBraceToken,
                SyntaxKind::AsyncKeyword,
                SyntaxKind::Identifier,
                SyntaxKind::OpenParenToken,
                SyntaxKind::CloseParenToken,
                SyntaxKind::OpenBraceToken,
                SyntaxKind::ReturnKeyword,
                SyntaxKind::Identifier,
                SyntaxKind::QuestionDotToken,
                SyntaxKind::Identifier,
                SyntaxKind::QuestionQuestionToken,
                SyntaxKind::NumericLiteral,
                SyntaxKind::SemicolonToken,
                SyntaxKind::CloseBraceToken,
                SyntaxKind::CloseBraceToken,
            ]
        );
    }

    #[test]
    fn greater_than_compounds_wait_for_rescan() {
        let tokens = scan_tokens("a >= b >> c >>>= d", LanguageVariant::Standard)
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![
                SyntaxKind::Identifier,
                SyntaxKind::GreaterThanToken,
                SyntaxKind::EqualsToken,
                SyntaxKind::Identifier,
                SyntaxKind::GreaterThanToken,
                SyntaxKind::GreaterThanToken,
                SyntaxKind::Identifier,
                SyntaxKind::GreaterThanToken,
                SyntaxKind::GreaterThanToken,
                SyntaxKind::GreaterThanToken,
                SyntaxKind::EqualsToken,
                SyntaxKind::Identifier,
            ]
        );
    }

    #[test]
    fn unicode_identifier_ranges_match_tsc_table() {
        let tokens = scan_tokens("var 才能ソЫ = 1;", LanguageVariant::Standard)
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            vec![
                SyntaxKind::VarKeyword,
                SyntaxKind::Identifier,
                SyntaxKind::EqualsToken,
                SyntaxKind::NumericLiteral,
                SyntaxKind::SemicolonToken,
            ]
        );
    }

    #[test]
    fn string_escape_sequences_set_value_and_flags() {
        let mut scanner = Scanner::new("\"\\n\\t\\x41\\u0042\\u{43}\"", LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);

        assert_eq!(scanner.token_value, "\n\tABC");
        assert!(scanner.token_flags.contains(TokenFlags::HEX_ESCAPE));
        assert!(scanner.token_flags.contains(TokenFlags::UNICODE_ESCAPE));
        assert!(scanner
            .token_flags
            .contains(TokenFlags::EXTENDED_UNICODE_ESCAPE));
        assert!(scanner.errors().is_empty());
    }

    #[test]
    fn invalid_extended_unicode_escape_reports_1198() {
        let mut scanner = Scanner::new("\"\\u{110000}\"", LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);

        assert!(scanner
            .token_flags
            .contains(TokenFlags::CONTAINS_INVALID_ESCAPE));
        assert_eq!(
            scanner
                .errors()
                .iter()
                .map(|error| error.message.code)
                .collect::<Vec<_>>(),
            vec![1198]
        );
    }

    #[test]
    fn unterminated_string_reports_1002() {
        let mut scanner = Scanner::new("\"abc", LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);

        assert!(scanner.token_flags.contains(TokenFlags::UNTERMINATED));
        assert_eq!(scanner.errors().len(), 1);
        assert_eq!(scanner.errors()[0].message.code, 1002);
    }

    #[test]
    fn string_escape_oracle_pins() {
        struct Case {
            text: &'static str,
            value: &'static str,
            flags: u32,
            errors: &'static [(usize, usize, u32)],
        }

        let cases = [
            Case {
                text: "\"\\n\"",
                value: "\n",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\t\"",
                value: "\t",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\b\"",
                value: "\u{0008}",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\v\"",
                value: "\u{000b}",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\f\"",
                value: "\u{000c}",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\r\"",
                value: "\r",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\'\"",
                value: "'",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "'\\\"'",
                value: "\"",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\0\"",
                value: "\0",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\x41\"",
                value: "A",
                flags: 4096,
                errors: &[],
            },
            Case {
                text: "\"\\u0042\"",
                value: "B",
                flags: 1024,
                errors: &[],
            },
            Case {
                text: "\"\\u{43}\"",
                value: "C",
                flags: 8,
                errors: &[],
            },
            Case {
                text: "\"a\\\nb\"",
                value: "ab",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"a\\\r\nb\"",
                value: "ab",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "\"\\xG\"",
                value: "\\xG",
                flags: 2048,
                errors: &[(3, 0, 1125)],
            },
            Case {
                text: "\"\\u00G0\"",
                value: "\\u00G0",
                flags: 2048,
                errors: &[(5, 0, 1125)],
            },
            Case {
                text: "\"\\u{}\"",
                value: "\\u{}",
                flags: 2048,
                errors: &[(4, 0, 1125)],
            },
            Case {
                text: "\"\\u{110000}\"",
                value: "\\u{110000}",
                flags: 2048,
                errors: &[(4, 6, 1198)],
            },
            Case {
                text: "\"\\u{41\"",
                value: "\\u{41",
                flags: 2048,
                errors: &[(6, 0, 1199)],
            },
            Case {
                text: "\"\\8\"",
                value: "8",
                flags: 2048,
                errors: &[(1, 2, 1488)],
            },
            Case {
                text: "\"\\123\"",
                value: "S",
                flags: 2048,
                errors: &[(1, 4, 1487)],
            },
            Case {
                text: "\"abc",
                value: "abc",
                flags: 4,
                errors: &[(4, 0, 1002)],
            },
        ];

        for case in cases {
            let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

            assert_eq!(scanner.scan(), SyntaxKind::StringLiteral, "{}", case.text);
            assert_eq!(scanner.token_value, case.value, "{}", case.text);
            assert_eq!(scanner.token_flags.0, case.flags, "{}", case.text);
            assert_eq!(
                scanner
                    .errors()
                    .iter()
                    .map(|error| (error.start, error.length, error.message.code))
                    .collect::<Vec<_>>(),
                case.errors,
                "{}",
                case.text
            );
        }
    }
}

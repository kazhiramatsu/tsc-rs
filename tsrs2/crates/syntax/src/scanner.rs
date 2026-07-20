use crate::{chars, keywords, SyntaxKind};
use tsrs2_diags::{gen, DiagnosticMessage};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LanguageVariant {
    #[default]
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

/// tsc CommentDirectiveType (8199): the two comment directives the
/// scanner recognizes, `@ts-expect-error` and `@ts-ignore`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentDirectiveKind {
    ExpectError,
    Ignore,
}

/// tsc CommentDirective: `{ range: { pos, end }, type }` collected by
/// the scanner while skipping comment trivia. Offsets are BYTE
/// positions (node-pos space): `pos` is the matched line's start — the
/// comment's own start for a single-line comment, the LAST line's
/// start for a multi-line comment — and `end` is one past the
/// comment's end (`*/` inclusive; end of text when unterminated).
/// Consumers key on the LINE of `end` (createCommentDirectivesMap
/// 12963), so byte-vs-UTF-16 never matters for suppression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommentDirective {
    pub pos: u32,
    pub end: u32,
    pub kind: CommentDirectiveKind,
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
pub(crate) struct ScanError {
    pub(crate) message: &'static DiagnosticMessage,
    pub(crate) start: usize,
    pub(crate) length: usize,
    pub(crate) args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TokenFlags(u32);

impl TokenFlags {
    const PRECEDING_LINE_BREAK: Self = Self(1);
    const UNTERMINATED: Self = Self(4);
    const EXTENDED_UNICODE_ESCAPE: Self = Self(8);
    const SCIENTIFIC: Self = Self(16);
    const OCTAL: Self = Self(32);
    const HEX_SPECIFIER: Self = Self(64);
    const BINARY_SPECIFIER: Self = Self(128);
    const OCTAL_SPECIFIER: Self = Self(256);
    const CONTAINS_SEPARATOR: Self = Self(512);
    const UNICODE_ESCAPE: Self = Self(1024);
    const CONTAINS_INVALID_ESCAPE: Self = Self(2048);
    const HEX_ESCAPE: Self = Self(4096);
    const CONTAINS_LEADING_ZERO: Self = Self(8192);
    const CONTAINS_INVALID_SEPARATOR: Self = Self(16384);

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
pub(crate) struct ScannerState {
    pos: usize,
    full_start_pos: usize,
    token_start: usize,
    token: SyntaxKind,
    token_value: String,
    token_flags: TokenFlags,
    error_len: usize,
}

pub(crate) struct Scanner<'text> {
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
    /// Deliberately NOT captured by save()/restore(): tsc's
    /// speculationHelper rewinds pos/token but leaves appended
    /// commentDirectives in place, so a rewound-then-rescanned comment
    /// appends a duplicate. Harmless: consumers key directives by end
    /// LINE (createCommentDirectivesMap), which deduplicates.
    comment_directives: Vec<CommentDirective>,
}

trait Truthy {
    fn is_truthy(&self) -> bool;
}

impl Truthy for bool {
    fn is_truthy(&self) -> bool {
        *self
    }
}

impl<T> Truthy for Option<T> {
    fn is_truthy(&self) -> bool {
        self.is_some()
    }
}

impl Truthy for SyntaxKind {
    fn is_truthy(&self) -> bool {
        *self != SyntaxKind::Unknown
    }
}

impl<'text> Scanner<'text> {
    pub(crate) fn new(text: &'text str, language_variant: LanguageVariant) -> Self {
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
            comment_directives: Vec::new(),
        }
    }

    pub(crate) fn text(&self) -> &'text str {
        self.text
    }

    pub(crate) fn scan(&mut self) -> SyntaxKind {
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
                    // tsc: error(Invalid_character) with no explicit span —
                    // at the backslash, length 0.
                    self.error_at(self.pos, 0, &gen::Invalid_character);
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
                    self.error_at(self.pos, ch.len_utf8(), &gen::Invalid_character);
                }
            }

            self.advance_char();
            self.token = SyntaxKind::Unknown;
            return self.token;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn save(&self) -> ScannerState {
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
    pub(crate) fn restore(&mut self, state: ScannerState) {
        self.pos = state.pos;
        self.full_start_pos = state.full_start_pos;
        self.token_start = state.token_start;
        self.token = state.token;
        self.token_value = state.token_value;
        self.token_flags = state.token_flags;
        self.errors.truncate(state.error_len);
    }

    #[allow(dead_code)]
    fn speculation_helper<R: Truthy>(
        &mut self,
        callback: impl FnOnce(&mut Self) -> R,
        is_lookahead: bool,
    ) -> R {
        let saved = self.save();
        let result = callback(self);
        if is_lookahead || !result.is_truthy() {
            self.restore(saved);
        }
        result
    }

    #[allow(dead_code)]
    fn look_ahead<R: Truthy>(&mut self, callback: impl FnOnce(&mut Self) -> R) -> R {
        self.speculation_helper(callback, true)
    }

    #[allow(dead_code)]
    fn try_scan<R: Truthy>(&mut self, callback: impl FnOnce(&mut Self) -> R) -> R {
        self.speculation_helper(callback, false)
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn full_start_pos(&self) -> usize {
        self.full_start_pos
    }

    pub(crate) fn token(&self) -> SyntaxKind {
        self.token
    }

    pub(crate) fn token_start(&self) -> usize {
        self.token_start
    }

    pub(crate) fn token_value(&self) -> &str {
        &self.token_value
    }

    pub(crate) fn take_errors(&mut self) -> Vec<ScanError> {
        std::mem::take(&mut self.errors)
    }

    /// tsc scanner.getCommentDirectives(), moved (parseSourceFileWorker
    /// hands the collected list to the SourceFile once per parse).
    pub(crate) fn take_comment_directives(&mut self) -> Vec<CommentDirective> {
        std::mem::take(&mut self.comment_directives)
    }

    pub(crate) fn has_preceding_line_break(&self) -> bool {
        self.token_flags.contains(TokenFlags::PRECEDING_LINE_BREAK)
    }

    /// tsc hasUnicodeEscape.
    pub(crate) fn has_unicode_escape(&self) -> bool {
        self.token_flags.contains(TokenFlags::UNICODE_ESCAPE)
    }

    /// tsc TokenFlags.IsInvalid: Octal | ContainsInvalidEscape |
    /// ContainsLeadingZero | ContainsInvalidSeparator.
    pub(crate) fn token_flags_are_invalid(&self) -> bool {
        self.token_flags.0
            & (TokenFlags::OCTAL.0
                | TokenFlags::CONTAINS_INVALID_ESCAPE.0
                | TokenFlags::CONTAINS_LEADING_ZERO.0
                | TokenFlags::CONTAINS_INVALID_SEPARATOR.0)
            != 0
    }

    /// tsc hasExtendedUnicodeEscape.
    pub(crate) fn has_extended_unicode_escape(&self) -> bool {
        self.token_flags
            .contains(TokenFlags::EXTENDED_UNICODE_ESCAPE)
    }

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

    /// tsc-port: scan @6.0.3 (single-line comment arm)
    /// tsc-hash: 5e22ed053f31e13697554019d0a8c2969d93c82bfb580dab7ac886a6c37c1fc8
    /// tsc-span: _tsc.js:9523-9542
    ///
    /// The comment text from `//` through the line end feeds
    /// appendIfCommentDirective with the single-line directive shape.
    fn skip_single_line_comment(&mut self) {
        self.pos += 2;
        while let Some(ch) = self.current_char() {
            if is_line_break(ch) {
                break;
            }
            self.advance_char();
        }
        self.append_if_comment_directive(self.token_start, CommentDirectiveRegEx::SingleLine);
    }

    /// tsc-port: scan @6.0.3 (multi-line comment arm)
    /// tsc-hash: d6a25c66cf14877d656700d6c7c24f3349a1c7e08a791afba3eb19400e72ae64
    /// tsc-span: _tsc.js:9543-9576
    ///
    /// Only the LAST line of the comment (lastLineStart through the
    /// closing `*/` inclusive) feeds appendIfCommentDirective, with the
    /// multi-line directive shape — a `@ts-ignore` on an interior line
    /// is NOT a directive. The append runs whether or not the comment
    /// closed. (The JSDoc token flag of the tsc arm has no consumer
    /// here yet.)
    fn skip_multi_line_comment(&mut self) {
        let mut last_line_start = self.token_start;
        self.pos += 2;

        let mut comment_closed = false;
        while self.pos < self.end {
            if self.starts_with("*/") {
                self.pos += 2;
                comment_closed = true;
                break;
            }

            let ch = self
                .current_char()
                .expect("position before end has a UTF-8 scalar");
            self.advance_char();
            if is_line_break(ch) {
                last_line_start = self.pos;
                self.token_flags.insert(TokenFlags::PRECEDING_LINE_BREAK);
            }
        }

        self.append_if_comment_directive(last_line_start, CommentDirectiveRegEx::MultiLine);

        if !comment_closed {
            // tsc: the unterminated-comment error sits at the scan
            // position (end of text), zero width.
            self.error_at(self.pos, 0, &gen::expected);
        }
    }

    /// tsc-port: appendIfCommentDirective @6.0.3
    /// tsc-hash: a6481c583d6596a12ddc3133df3cb45c7a5ba0d02f052fe2b99c130ee2a33587
    /// tsc-span: _tsc.js:10845-10857
    ///
    /// The matched slice runs from `line_start` to the CURRENT scan
    /// position (tsc slices at the call sites and closes over `pos`):
    /// the whole comment for the single-line shape, the last line
    /// incl. `*/` for the multi-line shape. `line_start` doubles as
    /// range.pos.
    fn append_if_comment_directive(&mut self, line_start: usize, regex: CommentDirectiveRegEx) {
        let comment = &self.text[line_start..self.pos];
        let Some(kind) = get_directive_from_comment(js_trim_start(comment), regex) else {
            return;
        };
        self.comment_directives.push(CommentDirective {
            pos: line_start as u32,
            end: self.pos as u32,
            kind,
        });
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

    /// tsc scan() hash case: `#` heads a private identifier only when an
    /// identifier (or identifier escape) follows; otherwise it is an
    /// Invalid_character Unknown token. `#!` past position 0 is 18026.
    fn scan_private_identifier(&mut self) -> SyntaxKind {
        let hash_pos = self.pos;
        if hash_pos != 0 && self.byte_at(hash_pos + 1) == Some(b'!') {
            self.error_at(hash_pos, 2, &gen::can_only_be_used_at_the_start_of_a_file);
            self.pos += 1;
            self.token = SyntaxKind::Unknown;
            return self.token;
        }

        self.pos += 1;
        self.token_value.clear();
        self.token_value.push('#');

        match self.current_char() {
            Some(ch) if chars::is_identifier_start(ch) => {
                self.token_value.push(ch);
                self.advance_char();
                self.scan_identifier_parts();
            }
            Some('\\') => {
                let escape_start = self.pos;
                match self.scan_unicode_escape() {
                    Some(ch) if chars::is_identifier_start(ch) => {
                        self.token_value.push(ch);
                        self.scan_identifier_parts();
                    }
                    _ => {
                        // tsc: a bare `#` still becomes a PrivateIdentifier
                        // token; only the error is reported.
                        self.pos = escape_start;
                        self.error_at(hash_pos, 1, &gen::Invalid_character);
                    }
                }
            }
            _ => {
                self.error_at(hash_pos, 1, &gen::Invalid_character);
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

        let (value, escape_flag) = if self.starts_with("{") {
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
            (value, TokenFlags::EXTENDED_UNICODE_ESCAPE)
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
            (
                u32::from_str_radix(digits, 16).ok()?,
                TokenFlags::UNICODE_ESCAPE,
            )
        };

        match char::from_u32(value) {
            Some(ch) => {
                // tsc scanIdentifierParts: a consumed identifier escape marks
                // the token so hasUnicodeEscape/hasExtendedUnicodeEscape work.
                self.token_flags.insert(escape_flag);
                Some(ch)
            }
            None => {
                self.pos = start;
                None
            }
        }
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
                let escaped = self.scan_escape_sequence(true);
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

    fn scan_escape_sequence(&mut self, report_errors: bool) -> String {
        let start = self.pos;
        self.pos += 1;
        if self.pos >= self.end {
            if report_errors {
                self.error_at(self.pos, 0, &gen::Unexpected_end_of_text);
            }
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
                self.scan_octal_escape(start, ch, report_errors)
            }
            '1'..='7' => self.scan_octal_escape(start, ch, report_errors),
            '8' | '9' => {
                self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
                if report_errors {
                    self.error_at(
                        start,
                        self.pos - start,
                        &gen::Escape_sequence_0_is_not_allowed,
                    );
                    ch.to_string()
                } else {
                    self.text[start..self.pos].to_owned()
                }
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
                    return self.scan_extended_unicode_escape(report_errors);
                }
                let digits_start = self.pos;
                for _ in 0..4 {
                    if !self.current_char().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                        self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
                        if report_errors {
                            self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected);
                        }
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
                        if report_errors {
                            self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected);
                        }
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

    fn scan_octal_escape(&mut self, start: usize, first: char, report_errors: bool) -> String {
        if self.current_char().is_some_and(is_octal_digit) {
            self.advance_char();
        }
        if matches!(first, '0'..='3') && self.current_char().is_some_and(is_octal_digit) {
            self.advance_char();
        }

        self.token_flags.insert(TokenFlags::CONTAINS_INVALID_ESCAPE);
        if report_errors {
            self.error_at(
                start,
                self.pos - start,
                &gen::Octal_escape_sequences_are_not_allowed_Use_the_syntax_0,
            );
            let value = u32::from_str_radix(&self.text[start + 1..self.pos], 8).unwrap_or(0xfffd);
            utf16_encode_as_string(value)
        } else {
            self.text[start..self.pos].to_owned()
        }
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
        self.scan_template_and_set_token_value(false)
    }

    fn scan_template_and_set_token_value(
        &mut self,
        should_emit_invalid_escape_error: bool,
    ) -> SyntaxKind {
        let started_with_backtick = self.byte_at(self.pos) == Some(b'`');
        self.pos += 1;
        let mut segment_start = self.pos;
        let mut contents = String::new();

        let token = loop {
            if self.pos >= self.end {
                contents.push_str(&self.text[segment_start..self.pos]);
                self.token_flags.insert(TokenFlags::UNTERMINATED);
                self.error_at(self.pos, 0, &gen::Unterminated_template_literal);
                break if started_with_backtick {
                    SyntaxKind::NoSubstitutionTemplateLiteral
                } else {
                    SyntaxKind::TemplateTail
                };
            }

            let ch = self
                .current_char()
                .expect("position before end has a UTF-8 scalar");
            if ch == '`' {
                contents.push_str(&self.text[segment_start..self.pos]);
                self.pos += 1;
                break if started_with_backtick {
                    SyntaxKind::NoSubstitutionTemplateLiteral
                } else {
                    SyntaxKind::TemplateTail
                };
            }
            if self.starts_with("${") {
                contents.push_str(&self.text[segment_start..self.pos]);
                self.pos += 2;
                break if started_with_backtick {
                    SyntaxKind::TemplateHead
                } else {
                    SyntaxKind::TemplateMiddle
                };
            }
            if ch == '\\' {
                contents.push_str(&self.text[segment_start..self.pos]);
                let escaped = self.scan_escape_sequence(should_emit_invalid_escape_error);
                contents.push_str(&escaped);
                segment_start = self.pos;
                continue;
            }
            if ch == '\r' {
                contents.push_str(&self.text[segment_start..self.pos]);
                self.pos += 1;
                if self.byte_at(self.pos) == Some(b'\n') {
                    self.pos += 1;
                }
                contents.push('\n');
                segment_start = self.pos;
                continue;
            }

            self.advance_char();
        };

        self.token_value = contents;
        self.token = token;
        self.token
    }

    #[allow(dead_code)]
    pub(crate) fn re_scan_template_token(&mut self, is_tagged_template: bool) -> SyntaxKind {
        self.pos = self.token_start;
        self.token = self.scan_template_and_set_token_value(!is_tagged_template);
        self.token
    }

    #[allow(dead_code)]
    pub(crate) fn re_scan_greater_token(&mut self) -> SyntaxKind {
        if self.token == SyntaxKind::GreaterThanToken {
            if self.byte_at(self.pos) == Some(b'>') {
                if self.byte_at(self.pos + 1) == Some(b'>') {
                    if self.byte_at(self.pos + 2) == Some(b'=') {
                        self.pos += 3;
                        self.token = SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken;
                        return self.token;
                    }
                    self.pos += 2;
                    self.token = SyntaxKind::GreaterThanGreaterThanGreaterThanToken;
                    return self.token;
                }
                if self.byte_at(self.pos + 1) == Some(b'=') {
                    self.pos += 2;
                    self.token = SyntaxKind::GreaterThanGreaterThanEqualsToken;
                    return self.token;
                }
                self.pos += 1;
                self.token = SyntaxKind::GreaterThanGreaterThanToken;
                return self.token;
            }
            if self.byte_at(self.pos) == Some(b'=') {
                self.pos += 1;
                self.token = SyntaxKind::GreaterThanEqualsToken;
                return self.token;
            }
        }
        self.token
    }

    #[allow(dead_code)]
    pub(crate) fn re_scan_slash_token(&mut self, _report_errors: bool) -> SyntaxKind {
        if !matches!(
            self.token,
            SyntaxKind::SlashToken | SyntaxKind::SlashEqualsToken
        ) {
            return self.token;
        }

        let start_of_regexp_body = self.token_start + 1;
        self.pos = start_of_regexp_body;
        let mut in_escape = false;
        let mut in_character_class = false;

        loop {
            let Some(ch) = self.current_char() else {
                self.token_flags.insert(TokenFlags::UNTERMINATED);
                break;
            };
            if is_line_break(ch) {
                self.token_flags.insert(TokenFlags::UNTERMINATED);
                break;
            }
            if in_escape {
                in_escape = false;
            } else if ch == '/' && !in_character_class {
                break;
            } else if ch == '[' {
                in_character_class = true;
            } else if ch == '\\' {
                in_escape = true;
            } else if ch == ']' {
                in_character_class = false;
            }
            self.advance_char();
        }

        let end_of_regexp_body = self.pos;
        if self.token_flags.contains(TokenFlags::UNTERMINATED) {
            self.pos = self.regex_unterminated_error_end(start_of_regexp_body, end_of_regexp_body);
            self.error_at(
                self.token_start,
                self.pos - self.token_start,
                &gen::Unterminated_regular_expression_literal,
            );
        } else {
            self.pos += 1;
            while let Some(ch) = self.current_char() {
                if !chars::is_identifier_part(ch) {
                    break;
                }
                self.advance_char();
            }
        }

        self.token_value = self.text[self.token_start..self.pos].to_owned();
        self.token = SyntaxKind::RegularExpressionLiteral;
        self.token
    }

    fn regex_unterminated_error_end(&self, start: usize, end: usize) -> usize {
        let mut pos = start;
        let mut in_escape = false;
        let mut character_class_depth = 0_u32;
        let mut in_decimal_quantifier = false;
        let mut group_depth = 0_u32;

        while pos < end {
            let ch = self.text[pos..]
                .chars()
                .next()
                .expect("regex recovery stays on UTF-8 boundaries");
            if in_escape {
                in_escape = false;
            } else if ch == '\\' {
                in_escape = true;
            } else if ch == '[' {
                character_class_depth += 1;
            } else if ch == ']' && character_class_depth > 0 {
                character_class_depth -= 1;
            } else if character_class_depth == 0 {
                if ch == '{' {
                    in_decimal_quantifier = true;
                } else if ch == '}' && in_decimal_quantifier {
                    in_decimal_quantifier = false;
                } else if !in_decimal_quantifier {
                    if ch == '(' {
                        group_depth += 1;
                    } else if ch == ')' && group_depth > 0 {
                        group_depth -= 1;
                    } else if matches!(ch, ')' | ']' | '}') {
                        break;
                    }
                }
            }
            pos += ch.len_utf8();
        }

        while pos > self.token_start {
            let Some((previous_pos, ch)) = self.text[..pos].char_indices().next_back() else {
                break;
            };
            if is_whitespace_like(ch) || ch == ';' {
                pos = previous_pos;
            } else {
                break;
            }
        }
        pos
    }

    #[allow(dead_code)]
    /// tsc resetTokenState: reposition for a fresh scan (reparse paths).
    pub(crate) fn reset_token_state(&mut self, pos: usize) {
        self.pos = pos;
        self.full_start_pos = pos;
        self.token_start = pos;
        self.token = SyntaxKind::Unknown;
        self.token_value.clear();
        self.token_flags = TokenFlags::empty();
    }

    pub(crate) fn re_scan_less_than_token(&mut self) -> SyntaxKind {
        if self.token == SyntaxKind::LessThanLessThanToken {
            self.pos = self.token_start + 1;
            self.token = SyntaxKind::LessThanToken;
        }
        self.token
    }

    /// tsc reScanAsteriskEqualsToken: `*=` becomes `*` (consumed by the
    /// caller as a JSDoc all-type) with the scanner repositioned on the `=`.
    pub(crate) fn re_scan_asterisk_equals_token(&mut self) -> SyntaxKind {
        debug_assert_eq!(
            self.token,
            SyntaxKind::AsteriskEqualsToken,
            "'re_scan_asterisk_equals_token' should only be called on a '*='"
        );
        self.pos = self.token_start + 1;
        self.token = SyntaxKind::EqualsToken;
        self.token
    }

    /// tsc reScanQuestionToken: `??` split so the first `?` can head a
    /// JSDoc unknown/nullable type.
    pub(crate) fn re_scan_question_token(&mut self) -> SyntaxKind {
        debug_assert_eq!(
            self.token,
            SyntaxKind::QuestionQuestionToken,
            "'re_scan_question_token' should only be called on a '??'"
        );
        self.pos = self.token_start + 1;
        self.token = SyntaxKind::QuestionToken;
        self.token
    }

    #[allow(dead_code)]
    fn re_scan_hash_token(&mut self) -> SyntaxKind {
        if self.token == SyntaxKind::PrivateIdentifier {
            self.pos = self.token_start + 1;
            self.token = SyntaxKind::HashToken;
        }
        self.token
    }

    pub(crate) fn scan_jsx_token(&mut self, allow_multiline_jsx_text: bool) -> SyntaxKind {
        self.full_start_pos = self.pos;
        self.token_start = self.pos;

        let Some(ch) = self.current_char() else {
            self.token = SyntaxKind::EndOfFileToken;
            return self.token;
        };

        if ch == '<' {
            if self.byte_at(self.pos + 1) == Some(b'/') {
                self.pos += 2;
                self.token = SyntaxKind::LessThanSlashToken;
                return self.token;
            }
            self.pos += 1;
            self.token = SyntaxKind::LessThanToken;
            return self.token;
        }

        if ch == '{' {
            self.pos += 1;
            self.token = SyntaxKind::OpenBraceToken;
            return self.token;
        }

        let mut first_non_whitespace = 0_isize;
        while let Some(ch) = self.current_char() {
            if ch == '{' || ch == '<' {
                break;
            }
            if ch == '>' {
                self.error_at(self.pos, 1, &gen::Unexpected_token_Did_you_mean_or_gt);
            }
            if ch == '}' {
                self.error_at(self.pos, 1, &gen::Unexpected_token_Did_you_mean_or_rbrace);
            }
            if is_line_break(ch) && first_non_whitespace == 0 {
                first_non_whitespace = -1;
            } else if !allow_multiline_jsx_text && is_line_break(ch) && first_non_whitespace > 0 {
                break;
            } else if !is_whitespace_like(ch) {
                first_non_whitespace = self.pos as isize;
            }
            self.advance_char();
        }

        self.token_value = self.text[self.full_start_pos..self.pos].to_owned();
        self.token = if first_non_whitespace == -1 {
            SyntaxKind::JsxTextAllWhiteSpaces
        } else {
            SyntaxKind::JsxText
        };
        self.token
    }

    pub(crate) fn re_scan_jsx_token(&mut self, allow_multiline_jsx_text: bool) -> SyntaxKind {
        self.pos = self.full_start_pos;
        self.token_start = self.full_start_pos;
        self.scan_jsx_token(allow_multiline_jsx_text)
    }

    pub(crate) fn scan_jsx_identifier(&mut self) -> SyntaxKind {
        if !token_is_identifier_or_keyword(self.token) {
            return self.token;
        }

        while self.pos < self.end {
            if self.byte_at(self.pos) == Some(b'-') {
                self.token_value.push('-');
                self.pos += 1;
                continue;
            }

            let old_pos = self.pos;
            self.scan_identifier_parts();
            if self.pos == old_pos {
                break;
            }
        }

        self.finish_identifier_token()
    }

    pub(crate) fn scan_jsx_attribute_value(&mut self) -> SyntaxKind {
        self.full_start_pos = self.pos;
        match self.current_char() {
            Some('"' | '\'') => self.scan_jsx_attribute_string(),
            _ => self.scan(),
        }
    }

    fn scan_jsx_attribute_string(&mut self) -> SyntaxKind {
        let quote = self
            .current_char()
            .expect("JSX attribute string starts at a quote");
        self.advance_char();
        self.token_value.clear();
        let segment_start = self.pos;

        while let Some(ch) = self.current_char() {
            if ch == quote {
                self.token_value
                    .push_str(&self.text[segment_start..self.pos]);
                self.advance_char();
                self.token = SyntaxKind::StringLiteral;
                return self.token;
            }
            self.advance_char();
        }

        self.token_value
            .push_str(&self.text[segment_start..self.pos]);
        self.token_flags.insert(TokenFlags::UNTERMINATED);
        self.error_at(self.pos, 0, &gen::Unterminated_string_literal);
        self.token = SyntaxKind::StringLiteral;
        self.token
    }

    #[allow(dead_code)]
    fn re_scan_jsx_attribute_value(&mut self) -> SyntaxKind {
        self.pos = self.full_start_pos;
        self.token_start = self.full_start_pos;
        self.scan_jsx_attribute_value()
    }

    fn scan_number_literal(&mut self) -> SyntaxKind {
        let start = self.pos;

        if self.byte_at(self.pos) == Some(b'0')
            && self.pos + 2 < self.end
            && matches!(self.byte_at(self.pos + 1), Some(b'x' | b'X'))
        {
            self.pos += 2;
            self.token_value = self.scan_hex_digits(1, true, true);
            if self.token_value.is_empty() {
                self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected);
                self.token_value = "0".to_owned();
            }
            self.token_value = format!("0x{}", self.token_value);
            self.token_flags.insert(TokenFlags::HEX_SPECIFIER);
            return self.check_big_int_suffix();
        }
        if self.byte_at(self.pos) == Some(b'0')
            && self.pos + 2 < self.end
            && matches!(self.byte_at(self.pos + 1), Some(b'b' | b'B'))
        {
            self.pos += 2;
            self.token_value = self.scan_binary_or_octal_digits(2);
            if self.token_value.is_empty() {
                self.error_at(self.pos, 0, &gen::Binary_digit_expected);
                self.token_value = "0".to_owned();
            }
            self.token_value = format!("0b{}", self.token_value);
            self.token_flags.insert(TokenFlags::BINARY_SPECIFIER);
            return self.check_big_int_suffix();
        }
        if self.byte_at(self.pos) == Some(b'0')
            && self.pos + 2 < self.end
            && matches!(self.byte_at(self.pos + 1), Some(b'o' | b'O'))
        {
            self.pos += 2;
            self.token_value = self.scan_binary_or_octal_digits(8);
            if self.token_value.is_empty() {
                self.error_at(self.pos, 0, &gen::Octal_digit_expected);
                self.token_value = "0".to_owned();
            }
            self.token_value = format!("0o{}", self.token_value);
            self.token_flags.insert(TokenFlags::OCTAL_SPECIFIER);
            return self.check_big_int_suffix();
        }

        self.scan_number(start)
    }

    fn scan_number(&mut self, start: usize) -> SyntaxKind {
        let main_fragment = if self.byte_at(self.pos) == Some(b'0') {
            self.pos += 1;
            if self.byte_at(self.pos) == Some(b'_') {
                self.token_flags.insert(TokenFlags::CONTAINS_SEPARATOR);
                self.token_flags
                    .insert(TokenFlags::CONTAINS_INVALID_SEPARATOR);
                self.error_at(self.pos, 1, &gen::Numeric_separators_are_not_allowed_here);
                self.pos -= 1;
                self.scan_number_fragment()
            } else {
                let (digits, is_octal) = self.scan_digits();
                if !is_octal {
                    self.token_flags.insert(TokenFlags::CONTAINS_LEADING_ZERO);
                    js_number_to_string(&digits)
                } else if digits.is_empty() {
                    "0".to_owned()
                } else {
                    let value = trim_leading_zeroes(&digits);
                    self.token_value = radix_digits_to_decimal_string(value, 8);
                    self.token_flags.insert(TokenFlags::OCTAL);
                    let with_minus = self.token == SyntaxKind::MinusToken;
                    let error_start = if with_minus {
                        start.saturating_sub(1)
                    } else {
                        start
                    };
                    let arg = format!("{}0o{}", if with_minus { "-" } else { "" }, value);
                    self.error_at_with_args(
                        error_start,
                        self.pos - error_start,
                        &gen::Octal_literals_are_not_allowed_Use_the_syntax_0,
                        vec![arg],
                    );
                    self.token = SyntaxKind::NumericLiteral;
                    return self.token;
                }
            }
        } else {
            self.scan_number_fragment()
        };

        let mut decimal_fragment = None;
        if self.byte_at(self.pos) == Some(b'.') {
            self.pos += 1;
            decimal_fragment = Some(self.scan_number_fragment());
        }

        let mut end = self.pos;
        let mut scientific_fragment = None;
        if matches!(self.byte_at(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            self.token_flags.insert(TokenFlags::SCIENTIFIC);
            if matches!(self.byte_at(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let pre_numeric_part = self.pos;
            let final_fragment = self.scan_number_fragment();
            if final_fragment.is_empty() {
                self.error_at(self.pos, 0, &gen::Digit_expected);
            } else {
                scientific_fragment = Some(format!(
                    "{}{}",
                    &self.text[end..pre_numeric_part],
                    final_fragment
                ));
                end = self.pos;
            }
        }

        let mut result = if self.token_flags.contains(TokenFlags::CONTAINS_SEPARATOR) {
            let mut result = main_fragment;
            if let Some(fragment) = &decimal_fragment {
                result.push('.');
                result.push_str(fragment);
            }
            if let Some(fragment) = &scientific_fragment {
                result.push_str(fragment);
            }
            result
        } else {
            self.text[start..end].to_owned()
        };

        if self.token_flags.contains(TokenFlags::CONTAINS_LEADING_ZERO) {
            self.error_at(
                start,
                end.saturating_sub(start),
                &gen::Decimals_with_leading_zeros_are_not_allowed,
            );
            self.token_value = js_number_to_string(&result);
            self.token = SyntaxKind::NumericLiteral;
            return self.token;
        }

        if decimal_fragment.is_some() || self.token_flags.contains(TokenFlags::SCIENTIFIC) {
            let is_scientific =
                decimal_fragment.is_none() && self.token_flags.contains(TokenFlags::SCIENTIFIC);
            self.check_for_identifier_start_after_numeric_literal(start, is_scientific);
            self.token_value = js_number_to_string(&result);
            self.token = SyntaxKind::NumericLiteral;
            return self.token;
        }

        self.token_value = std::mem::take(&mut result);
        let token = self.check_big_int_suffix();
        self.check_for_identifier_start_after_numeric_literal(start, false);
        token
    }

    fn scan_number_fragment(&mut self) -> String {
        let mut start = self.pos;
        let mut allow_separator = false;
        let mut is_previous_token_separator = false;
        let mut result = String::new();

        loop {
            match self.byte_at(self.pos) {
                Some(b'_') => {
                    self.token_flags.insert(TokenFlags::CONTAINS_SEPARATOR);
                    if allow_separator {
                        allow_separator = false;
                        is_previous_token_separator = true;
                        result.push_str(&self.text[start..self.pos]);
                    } else {
                        self.token_flags
                            .insert(TokenFlags::CONTAINS_INVALID_SEPARATOR);
                        let message = if is_previous_token_separator {
                            &gen::Multiple_consecutive_numeric_separators_are_not_permitted
                        } else {
                            &gen::Numeric_separators_are_not_allowed_here
                        };
                        self.error_at(self.pos, 1, message);
                    }
                    self.pos += 1;
                    start = self.pos;
                }
                Some(byte) if byte.is_ascii_digit() => {
                    allow_separator = true;
                    is_previous_token_separator = false;
                    self.pos += 1;
                }
                _ => break,
            }
        }

        if self.pos > 0 && self.byte_at(self.pos - 1) == Some(b'_') {
            self.token_flags
                .insert(TokenFlags::CONTAINS_INVALID_SEPARATOR);
            self.error_at(
                self.pos - 1,
                1,
                &gen::Numeric_separators_are_not_allowed_here,
            );
        }
        result.push_str(&self.text[start..self.pos]);
        result
    }

    fn scan_digits(&mut self) -> (String, bool) {
        let start = self.pos;
        let mut is_octal = true;
        while let Some(byte) = self.byte_at(self.pos) {
            if !byte.is_ascii_digit() {
                break;
            }
            if !matches!(byte, b'0'..=b'7') {
                is_octal = false;
            }
            self.pos += 1;
        }
        (self.text[start..self.pos].to_owned(), is_octal)
    }

    fn scan_hex_digits(
        &mut self,
        min_count: usize,
        scan_as_many_as_possible: bool,
        can_have_separators: bool,
    ) -> String {
        let mut value = String::new();
        let mut allow_separator = false;
        let mut is_previous_token_separator = false;

        while value.len() < min_count || scan_as_many_as_possible {
            let Some(byte) = self.byte_at(self.pos) else {
                break;
            };
            if can_have_separators && byte == b'_' {
                self.token_flags.insert(TokenFlags::CONTAINS_SEPARATOR);
                if allow_separator {
                    allow_separator = false;
                    is_previous_token_separator = true;
                } else {
                    let message = if is_previous_token_separator {
                        &gen::Multiple_consecutive_numeric_separators_are_not_permitted
                    } else {
                        &gen::Numeric_separators_are_not_allowed_here
                    };
                    self.error_at(self.pos, 1, message);
                }
                self.pos += 1;
                continue;
            }

            allow_separator = can_have_separators;
            let lower = byte.to_ascii_lowercase();
            if !lower.is_ascii_hexdigit() {
                break;
            }
            value.push(lower as char);
            self.pos += 1;
            is_previous_token_separator = false;
        }

        if value.len() < min_count {
            value.clear();
        }
        if self.pos > 0 && self.byte_at(self.pos - 1) == Some(b'_') {
            self.error_at(
                self.pos - 1,
                1,
                &gen::Numeric_separators_are_not_allowed_here,
            );
        }
        value
    }

    fn scan_binary_or_octal_digits(&mut self, base: u8) -> String {
        let mut value = String::new();
        let mut separator_allowed = false;
        let mut is_previous_token_separator = false;

        loop {
            let Some(byte) = self.byte_at(self.pos) else {
                break;
            };
            if byte == b'_' {
                self.token_flags.insert(TokenFlags::CONTAINS_SEPARATOR);
                if separator_allowed {
                    separator_allowed = false;
                    is_previous_token_separator = true;
                } else {
                    let message = if is_previous_token_separator {
                        &gen::Multiple_consecutive_numeric_separators_are_not_permitted
                    } else {
                        &gen::Numeric_separators_are_not_allowed_here
                    };
                    self.error_at(self.pos, 1, message);
                }
                self.pos += 1;
                continue;
            }

            separator_allowed = true;
            if !byte.is_ascii_digit() || byte - b'0' >= base {
                break;
            }
            value.push(byte as char);
            self.pos += 1;
            is_previous_token_separator = false;
        }

        if self.pos > 0 && self.byte_at(self.pos - 1) == Some(b'_') {
            self.error_at(
                self.pos - 1,
                1,
                &gen::Numeric_separators_are_not_allowed_here,
            );
        }
        value
    }

    fn check_big_int_suffix(&mut self) -> SyntaxKind {
        if self.byte_at(self.pos) == Some(b'n') {
            self.token_value.push('n');
            if self.token_flags.contains(TokenFlags::BINARY_SPECIFIER) {
                let digits = &self.token_value[2..self.token_value.len() - 1];
                self.token_value = format!("{}n", radix_digits_to_decimal_string(digits, 2));
            } else if self.token_flags.contains(TokenFlags::OCTAL_SPECIFIER) {
                let digits = &self.token_value[2..self.token_value.len() - 1];
                self.token_value = format!("{}n", radix_digits_to_decimal_string(digits, 8));
            }
            self.pos += 1;
            self.token = SyntaxKind::BigIntLiteral;
        } else {
            if self.token_flags.contains(TokenFlags::BINARY_SPECIFIER) {
                self.token_value = radix_digits_to_decimal_string(&self.token_value[2..], 2);
            } else if self.token_flags.contains(TokenFlags::OCTAL_SPECIFIER) {
                self.token_value = radix_digits_to_decimal_string(&self.token_value[2..], 8);
            } else if self.token_flags.contains(TokenFlags::HEX_SPECIFIER) {
                self.token_value = radix_digits_to_decimal_string(&self.token_value[2..], 16);
            } else {
                self.token_value = js_number_to_string(&self.token_value);
            }
            self.token = SyntaxKind::NumericLiteral;
        }
        self.token
    }

    fn check_for_identifier_start_after_numeric_literal(
        &mut self,
        numeric_start: usize,
        is_scientific: bool,
    ) {
        let Some(ch) = self.current_char() else {
            return;
        };
        if !chars::is_identifier_start(ch) {
            return;
        }

        let identifier_start = self.pos;
        let length = self.scan_identifier_part_length();
        if length == 1 && self.text[identifier_start..].starts_with('n') {
            let message = if is_scientific {
                &gen::A_bigint_literal_cannot_use_exponential_notation
            } else {
                &gen::A_bigint_literal_must_be_an_integer
            };
            self.error_at(numeric_start, identifier_start - numeric_start + 1, message);
        } else {
            self.error_at(
                identifier_start,
                length,
                &gen::An_identifier_or_keyword_cannot_immediately_follow_a_numeric_literal,
            );
            self.pos = identifier_start;
        }
    }

    fn scan_identifier_part_length(&mut self) -> usize {
        // Measurement only: keep escape flags from leaking onto the token.
        let saved_token_flags = self.token_flags;
        let length = self.scan_identifier_part_length_worker();
        self.token_flags = saved_token_flags;
        length
    }

    fn scan_identifier_part_length_worker(&mut self) -> usize {
        let start = self.pos;
        while let Some(ch) = self.current_char() {
            if chars::is_identifier_part(ch) {
                self.advance_char();
            } else if ch == '\\' {
                let escape_start = self.pos;
                if let Some(ch) = self.scan_unicode_escape() {
                    if chars::is_identifier_part(ch) {
                        continue;
                    }
                }
                self.pos = escape_start;
                break;
            } else {
                break;
            }
        }
        self.pos - start
    }

    fn finish_token(&mut self, kind: SyntaxKind, width: usize) -> SyntaxKind {
        self.pos += width;
        self.token = kind;
        kind
    }

    fn error_at(&mut self, start: usize, length: usize, message: &'static DiagnosticMessage) {
        self.error_at_with_args(start, length, message, Vec::new());
    }

    fn error_at_with_args(
        &mut self,
        start: usize,
        length: usize,
        message: &'static DiagnosticMessage,
        args: Vec<String>,
    ) {
        self.errors.push(ScanError {
            message,
            start,
            length,
            args,
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

fn trim_leading_zeroes(text: &str) -> &str {
    let trimmed = text.trim_start_matches('0');
    if trimmed.is_empty() {
        "0"
    } else {
        trimmed
    }
}

fn radix_digits_to_decimal_string(digits: &str, radix: u32) -> String {
    let mut decimal_digits = vec![0_u8];

    for byte in digits.bytes() {
        let Some(digit) = ascii_digit_value(byte) else {
            continue;
        };
        if digit >= radix {
            continue;
        }

        let mut carry = digit;
        for place in &mut decimal_digits {
            let value = u32::from(*place) * radix + carry;
            *place = (value % 10) as u8;
            carry = value / 10;
        }
        while carry > 0 {
            decimal_digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }

    while decimal_digits.len() > 1 && decimal_digits.last() == Some(&0) {
        decimal_digits.pop();
    }

    decimal_digits
        .iter()
        .rev()
        .map(|digit| char::from(b'0' + *digit))
        .collect()
}

fn ascii_digit_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'f' => Some(u32::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u32::from(byte - b'A' + 10)),
        _ => None,
    }
}

fn js_number_to_string(text: &str) -> String {
    let normalized;
    let text = if text.starts_with('.') {
        normalized = format!("0{text}");
        normalized.as_str()
    } else {
        text
    };

    match text.parse::<f64>() {
        Ok(0.0) => "0".to_owned(),
        Ok(value) if value.is_finite() => value.to_string(),
        Ok(value) if value.is_sign_positive() => "Infinity".to_owned(),
        Ok(_) => "-Infinity".to_owned(),
        Err(_) => "NaN".to_owned(),
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

/// The scan half of tsc isValidBigIntString (18973-18989), which
/// probes `s + "n"` with a fresh scanner. The tsc probe scans with
/// skipTrivia:false so any trivia surfaces as a non-BigIntLiteral
/// result; this scanner skips trivia inside `scan()`, so the
/// start-adjacency checks below reject the same inputs. `None` means
/// the text is not exactly one optional minus + one BigIntLiteral
/// token covering the whole input, or the scan errored. The
/// round-trip comparison stays with the checker (parsePseudoBigInt
/// lives there).
pub struct BigIntStringScan {
    pub negative: bool,
    /// Scanner token value including the trailing `n`:
    /// radix-normalized for binary/octal (this scanner does at scan
    /// time what tsc defers to parsePseudoBigInt — checkBigIntSuffix
    /// converts exactly those two specifiers), raw for hex/decimal,
    /// separators stripped.
    pub token_value: String,
    /// tsc TokenFlags.ContainsSeparator — a validity gate upstream.
    pub contains_separator: bool,
}

pub fn scan_big_int_string(s: &str) -> Option<BigIntStringScan> {
    let text = format!("{s}n");
    let mut scanner = Scanner::new(&text, LanguageVariant::Standard);
    let mut token = scanner.scan();
    let mut expected_start = 0usize;
    let negative = token == SyntaxKind::MinusToken;
    if negative {
        if scanner.token_start() != 0 {
            return None;
        }
        expected_start = scanner.pos();
        token = scanner.scan();
    }
    if token != SyntaxKind::BigIntLiteral
        || scanner.token_start() != expected_start
        || scanner.pos() != text.len()
        || !scanner.errors().is_empty()
    {
        return None;
    }
    Some(BigIntStringScan {
        negative,
        token_value: scanner.token_value().to_owned(),
        contains_separator: scanner.token_flags.contains(TokenFlags::CONTAINS_SEPARATOR),
    })
}

/// tsc skipTrivia over the trivia forms this scanner produces (shebang,
/// whitespace, line breaks, single/multi-line comments).
pub fn skip_trivia(text: &str, start: usize) -> usize {
    let mut pos = start;
    loop {
        if pos == 0 && text.starts_with("#!") {
            while let Some(ch) = text[pos..].chars().next() {
                if is_line_break(ch) {
                    break;
                }
                pos += ch.len_utf8();
            }
            continue;
        }
        let Some(ch) = text[pos..].chars().next() else {
            return pos;
        };
        if is_whitespace_like(ch) {
            pos += ch.len_utf8();
            continue;
        }
        if text[pos..].starts_with("//") {
            pos += 2;
            while let Some(ch) = text[pos..].chars().next() {
                if is_line_break(ch) {
                    break;
                }
                pos += ch.len_utf8();
            }
            continue;
        }
        if text[pos..].starts_with("/*") {
            pos += 2;
            loop {
                if text[pos..].starts_with("*/") {
                    pos += 2;
                    break;
                }
                let Some(ch) = text[pos..].chars().next() else {
                    return pos;
                };
                pos += ch.len_utf8();
            }
            continue;
        }
        return pos;
    }
}

/// tsc-port: isLineBreak @6.0.3
/// tsc-hash: 395982e80e116c8398678784061594649abf26264ad052748b2fa4af2a106ef9
/// tsc-span: _tsc.js:8337-8339
pub fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_single_line_whitespace(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200B}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// tsc-port: isWhiteSpaceLike @6.0.3
/// tsc-hash: 124a9964c3f49bd1a6000a5b891b57fabe0f56a270a199ff89dc83ab089b0dad
/// tsc-span: _tsc.js:8331-8333
pub fn is_whitespace_like(ch: char) -> bool {
    is_line_break(ch) || is_single_line_whitespace(ch)
}

/// ECMAScript WhiteSpace ∪ LineTerminator — the set behind BOTH the
/// regex `\s` class and String.prototype.trim/trimStart, which the
/// comment-directive machinery uses (commentDirectiveRegEx*,
/// getDirectiveFromComment's trimStart, and the checker walk's
/// line.trim()). NOT the same set as tsc's isWhiteSpaceSingleLine:
/// U+0085 NEXT LINE and U+200B ZERO WIDTH SPACE are scanner
/// whitespace but NOT in `\s`, while U+FEFF is in both. Rust's
/// char::is_whitespace differs on both ends (has U+0085, lacks
/// U+FEFF), hence the explicit table.
pub fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// String.prototype.trimStart over the JS whitespace set.
pub fn js_trim_start(text: &str) -> &str {
    text.trim_start_matches(is_js_whitespace)
}

/// The two directive shapes (8202-8203):
/// `commentDirectiveRegExSingleLine = /^\/\/\/?\s*@(ts-expect-error|ts-ignore)/`
/// `commentDirectiveRegExMultiLine = /^(?:\/|\*)*\s*@(ts-expect-error|ts-ignore)/`
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommentDirectiveRegEx {
    SingleLine,
    MultiLine,
}

/// tsc-port: getDirectiveFromComment @6.0.3
/// tsc-hash: 3f6a61f955474e2c6be73a5d5f5ec207f39b2517119ad3ab73c75ca7968249c0
/// tsc-span: _tsc.js:10858-10870
///
/// Expects the trimStart'd comment slice (the caller mirrors
/// appendIfCommentDirective's `text.trimStart()`). The regexes have no
/// word boundary after the directive name — `@ts-ignored` matches as
/// `@ts-ignore`, faithfully.
fn get_directive_from_comment(
    text: &str,
    regex: CommentDirectiveRegEx,
) -> Option<CommentDirectiveKind> {
    let rest = match regex {
        // ^\/\/\/?  — two slashes, then at most one more.
        CommentDirectiveRegEx::SingleLine => {
            let rest = text.strip_prefix("//")?;
            rest.strip_prefix('/').unwrap_or(rest)
        }
        // ^(?:\/|\*)* — any contiguous run of slashes and asterisks.
        CommentDirectiveRegEx::MultiLine => text.trim_start_matches(['/', '*']),
    };
    let rest = js_trim_start(rest).strip_prefix('@')?;
    if rest.starts_with("ts-expect-error") {
        return Some(CommentDirectiveKind::ExpectError);
    }
    if rest.starts_with("ts-ignore") {
        return Some(CommentDirectiveKind::Ignore);
    }
    None
}

fn token_is_identifier_or_keyword(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::Identifier
        || (kind as u16) >= (SyntaxKind::FirstKeyword as u16)
            && (kind as u16) <= (SyntaxKind::LastKeyword as u16)
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

        // tsc pins: the error sits at the end of text, zero width.
        assert_eq!(scanner.errors().len(), 1);
        assert_eq!(scanner.errors()[0].message.code, 1010);
        assert_eq!(scanner.errors()[0].start, "/* unterminated".len());
        assert_eq!(scanner.errors()[0].length, 0);
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
    fn look_ahead_always_rewinds_after_truthy_result() {
        let mut scanner = Scanner::new("a b", LanguageVariant::Standard);

        let result = scanner.look_ahead(|scanner| {
            assert_eq!(scanner.scan(), SyntaxKind::Identifier);
            Some(scanner.pos())
        });

        assert_eq!(result, Some(1));
        assert_eq!(scanner.pos(), 0);
        assert_eq!(scanner.token, SyntaxKind::Unknown);
    }

    #[test]
    fn try_scan_commits_truthy_and_rewinds_falsy() {
        let mut scanner = Scanner::new("a b", LanguageVariant::Standard);

        let result = scanner.try_scan(|scanner| scanner.scan());

        assert_eq!(result, SyntaxKind::Identifier);
        assert_eq!(scanner.pos(), 1);
        assert_eq!(scanner.token, SyntaxKind::Identifier);

        let result = scanner.try_scan(|scanner| {
            assert_eq!(scanner.scan(), SyntaxKind::Identifier);
            false
        });

        assert!(!result);
        assert_eq!(scanner.pos(), 1);
        assert_eq!(scanner.token, SyntaxKind::Identifier);
    }

    #[test]
    fn speculation_restores_nested_state_and_errors() {
        let mut scanner = Scanner::new("a \"\\xG\"", LanguageVariant::Standard);

        let result = scanner.look_ahead(|scanner| {
            assert_eq!(scanner.scan(), SyntaxKind::Identifier);
            let inner = scanner.try_scan(|scanner| {
                assert_eq!(scanner.scan(), SyntaxKind::StringLiteral);
                assert_eq!(scanner.errors().len(), 1);
                true
            });
            assert!(inner);
            assert_eq!(scanner.pos(), "a \"\\xG\"".len());
            assert_eq!(scanner.errors().len(), 1);
            true
        });

        assert!(result);
        assert_eq!(scanner.pos(), 0);
        assert_eq!(scanner.token, SyntaxKind::Unknown);
        assert!(scanner.errors().is_empty());
    }

    fn directives_of(text: &str) -> Vec<CommentDirective> {
        let mut scanner = Scanner::new(text, LanguageVariant::Standard);
        while scanner.scan() != SyntaxKind::EndOfFileToken {}
        scanner.take_comment_directives()
    }

    #[test]
    fn single_line_comment_directives_are_collected() {
        let text = "// @ts-ignore\nlet x;\n/// @ts-expect-error\nlet y;\n";
        assert_eq!(
            directives_of(text),
            vec![
                CommentDirective {
                    pos: 0,
                    end: "// @ts-ignore".len() as u32,
                    kind: CommentDirectiveKind::Ignore,
                },
                CommentDirective {
                    pos: text.find("///").unwrap() as u32,
                    end: text.find("///").unwrap() as u32 + "/// @ts-expect-error".len() as u32,
                    kind: CommentDirectiveKind::ExpectError,
                },
            ]
        );
    }

    #[test]
    fn trailing_single_line_comment_is_a_directive() {
        // The comment slice starts at `//` regardless of what precedes
        // it on the line.
        assert_eq!(directives_of("let a = 1; // @ts-ignore\nlet x;\n").len(), 1);
    }

    #[test]
    fn four_slashes_are_not_a_directive() {
        // ^\/\/\/?  allows at most three slashes before the pragma.
        assert_eq!(directives_of("////@ts-ignore\n"), Vec::new());
        assert_eq!(directives_of("///@ts-ignore\n").len(), 1);
    }

    #[test]
    fn directive_name_has_no_word_boundary() {
        // tsc's regex quirk: `@ts-ignored` matches as `@ts-ignore`.
        assert_eq!(
            directives_of("// @ts-ignored\n")[0].kind,
            CommentDirectiveKind::Ignore
        );
    }

    #[test]
    fn multi_line_directive_matches_only_the_last_line() {
        // Interior lines never match, whatever they contain.
        assert_eq!(
            directives_of("/*\n@ts-ignore\nrest\n*/\nlet x;\n"),
            Vec::new()
        );
        assert_eq!(directives_of("/*\n@ts-ignore\n*/\nlet x;\n"), Vec::new());

        // One-liner: the last line IS the whole comment.
        let one_liner = "/* @ts-ignore */\nlet x;\n";
        assert_eq!(
            directives_of(one_liner),
            vec![CommentDirective {
                pos: 0,
                end: "/* @ts-ignore */".len() as u32,
                kind: CommentDirectiveKind::Ignore,
            }]
        );

        // Closing-line directive: pos is the LAST line's start, end is
        // one past `*/`; leading whitespace and a star shell are
        // allowed by ^(?:\/|\*)*\s* after trimStart.
        let closing = "/* leading\n * @ts-expect-error */\nlet x;\n";
        let last_line = closing.find(" * @").unwrap();
        assert_eq!(
            directives_of(closing),
            vec![CommentDirective {
                pos: last_line as u32,
                end: closing.find("*/").unwrap() as u32 + 2,
                kind: CommentDirectiveKind::ExpectError,
            }]
        );

        // A bare space between star-shell runs breaks the match.
        assert_eq!(directives_of("/*\n * * @ts-ignore */\n"), Vec::new());
    }

    #[test]
    fn unterminated_multi_line_comment_still_appends_its_directive() {
        // tsc appends before the unterminated-comment error.
        let text = "/* @ts-ignore";
        assert_eq!(
            directives_of(text),
            vec![CommentDirective {
                pos: 0,
                end: text.len() as u32,
                kind: CommentDirectiveKind::Ignore,
            }]
        );
    }

    #[test]
    fn template_literal_interior_is_not_a_directive() {
        assert_eq!(
            directives_of("const s = `\n// @ts-ignore\n`;\n"),
            Vec::new()
        );
    }

    #[test]
    fn js_whitespace_diverges_from_scanner_whitespace_where_tsc_does() {
        // U+0085 / U+200B: scanner trivia, not regex \s.
        assert!(is_single_line_whitespace('\u{0085}'));
        assert!(!is_js_whitespace('\u{0085}'));
        assert!(is_single_line_whitespace('\u{200B}'));
        assert!(!is_js_whitespace('\u{200B}'));
        // U+FEFF is in both.
        assert!(is_js_whitespace('\u{FEFF}'));
        assert_eq!(directives_of("//\u{0085}@ts-ignore\n"), Vec::new());
        assert_eq!(directives_of("//\u{FEFF}@ts-ignore\n").len(), 1);
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

    #[test]
    fn numeric_literal_oracle_pins() {
        struct Case {
            text: &'static str,
            kind: SyntaxKind,
            end: usize,
            value: &'static str,
            flags: u32,
            errors: &'static [(usize, usize, u32)],
        }

        let cases = [
            Case {
                text: "1_2",
                kind: SyntaxKind::NumericLiteral,
                end: 3,
                value: "12",
                flags: 512,
                errors: &[],
            },
            Case {
                text: "1__2",
                kind: SyntaxKind::NumericLiteral,
                end: 4,
                value: "12",
                flags: 16896,
                errors: &[(2, 1, 6189)],
            },
            Case {
                text: "1_",
                kind: SyntaxKind::NumericLiteral,
                end: 2,
                value: "1",
                flags: 16896,
                errors: &[(1, 1, 6188)],
            },
            Case {
                text: "0_1",
                kind: SyntaxKind::NumericLiteral,
                end: 3,
                value: "1",
                flags: 16896,
                errors: &[(1, 1, 6188)],
            },
            Case {
                text: "01",
                kind: SyntaxKind::NumericLiteral,
                end: 2,
                value: "1",
                flags: 32,
                errors: &[(0, 2, 1121)],
            },
            Case {
                text: "08",
                kind: SyntaxKind::NumericLiteral,
                end: 2,
                value: "8",
                flags: 8192,
                errors: &[(0, 2, 1489)],
            },
            Case {
                text: "1e2",
                kind: SyntaxKind::NumericLiteral,
                end: 3,
                value: "100",
                flags: 16,
                errors: &[],
            },
            Case {
                text: "1e+n",
                kind: SyntaxKind::NumericLiteral,
                end: 4,
                value: "1",
                flags: 16,
                errors: &[(3, 0, 1124), (0, 4, 1352)],
            },
            Case {
                text: "1.0n",
                kind: SyntaxKind::NumericLiteral,
                end: 4,
                value: "1",
                flags: 0,
                errors: &[(0, 4, 1353)],
            },
            Case {
                text: "1n",
                kind: SyntaxKind::BigIntLiteral,
                end: 2,
                value: "1n",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "0xAFn",
                kind: SyntaxKind::BigIntLiteral,
                end: 5,
                value: "0xafn",
                flags: 64,
                errors: &[],
            },
            Case {
                text: "0x_f",
                kind: SyntaxKind::NumericLiteral,
                end: 4,
                value: "15",
                flags: 576,
                errors: &[(2, 1, 6188)],
            },
            Case {
                text: "0x",
                kind: SyntaxKind::NumericLiteral,
                end: 1,
                value: "0",
                flags: 0,
                errors: &[(1, 1, 1351)],
            },
            Case {
                text: "0b101n",
                kind: SyntaxKind::BigIntLiteral,
                end: 6,
                value: "5n",
                flags: 128,
                errors: &[],
            },
            Case {
                text: "0b_",
                kind: SyntaxKind::NumericLiteral,
                end: 3,
                value: "0",
                flags: 640,
                errors: &[(2, 1, 6188), (2, 1, 6188), (3, 0, 1177)],
            },
            Case {
                text: "0o77n",
                kind: SyntaxKind::BigIntLiteral,
                end: 5,
                value: "63n",
                flags: 256,
                errors: &[],
            },
            Case {
                text: ".5",
                kind: SyntaxKind::NumericLiteral,
                end: 2,
                value: "0.5",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "00.1",
                kind: SyntaxKind::NumericLiteral,
                end: 2,
                value: "0",
                flags: 32,
                errors: &[(0, 2, 1121)],
            },
        ];

        for case in cases {
            let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

            assert_eq!(scanner.scan(), case.kind, "{}", case.text);
            assert_eq!(scanner.pos(), case.end, "{}", case.text);
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

    #[test]
    fn legacy_octal_after_minus_reports_from_minus() {
        let mut scanner = Scanner::new("-01", LanguageVariant::Standard);

        assert_eq!(scanner.scan(), SyntaxKind::MinusToken);
        assert_eq!(scanner.scan(), SyntaxKind::NumericLiteral);

        assert_eq!(scanner.token_value, "1");
        assert_eq!(scanner.token_flags.0, 32);
        assert_eq!(scanner.errors().len(), 1);
        assert_eq!(scanner.errors()[0].start, 0);
        assert_eq!(scanner.errors()[0].length, 3);
        assert_eq!(scanner.errors()[0].message.code, 1121);
        assert_eq!(scanner.errors()[0].args, vec!["-0o1".to_owned()]);
    }

    #[test]
    fn template_literal_oracle_pins() {
        struct Case {
            text: &'static str,
            kind: SyntaxKind,
            end: usize,
            value: &'static str,
            flags: u32,
            errors: &'static [(usize, usize, u32)],
        }

        let cases = [
            Case {
                text: "`a`",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 3,
                value: "a",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "`a${b}`",
                kind: SyntaxKind::TemplateHead,
                end: 4,
                value: "a",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "`a\\nb`",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 6,
                value: "a\nb",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "`a\r\nb`",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 6,
                value: "a\nb",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "`a\rb`",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 5,
                value: "a\nb",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "`\\xG`",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 5,
                value: "\\xG",
                flags: 2048,
                errors: &[],
            },
            Case {
                text: "`\\u{110000}`",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 12,
                value: "\\u{110000}",
                flags: 2048,
                errors: &[],
            },
            Case {
                text: "`abc",
                kind: SyntaxKind::NoSubstitutionTemplateLiteral,
                end: 4,
                value: "abc",
                flags: 4,
                errors: &[(4, 0, 1160)],
            },
        ];

        for case in cases {
            let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

            assert_eq!(scanner.scan(), case.kind, "{}", case.text);
            assert_eq!(scanner.pos(), case.end, "{}", case.text);
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

    #[test]
    fn rescan_template_token_oracle_pins() {
        struct Case {
            text: &'static str,
            is_tagged_template: bool,
            kind: SyntaxKind,
            end: usize,
            value: &'static str,
            flags: u32,
            errors: &'static [(usize, usize, u32)],
        }

        let cases = [
            Case {
                text: "}tail`",
                is_tagged_template: false,
                kind: SyntaxKind::TemplateTail,
                end: 6,
                value: "tail",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "}mid${x}`",
                is_tagged_template: false,
                kind: SyntaxKind::TemplateMiddle,
                end: 6,
                value: "mid",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "}\\xG`",
                is_tagged_template: false,
                kind: SyntaxKind::TemplateTail,
                end: 5,
                value: "\\xG",
                flags: 2048,
                errors: &[(3, 0, 1125)],
            },
            Case {
                text: "}\\u{110000}`",
                is_tagged_template: false,
                kind: SyntaxKind::TemplateTail,
                end: 12,
                value: "\\u{110000}",
                flags: 2048,
                errors: &[(4, 6, 1198)],
            },
            Case {
                text: "}\\xG`",
                is_tagged_template: true,
                kind: SyntaxKind::TemplateTail,
                end: 5,
                value: "\\xG",
                flags: 2048,
                errors: &[],
            },
        ];

        for case in cases {
            let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

            assert_eq!(scanner.scan(), SyntaxKind::CloseBraceToken, "{}", case.text);
            assert_eq!(
                scanner.re_scan_template_token(case.is_tagged_template),
                case.kind,
                "{}",
                case.text
            );
            assert_eq!(scanner.pos(), case.end, "{}", case.text);
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

    #[test]
    fn rescan_greater_less_and_hash_oracle_pins() {
        let cases = [
            (">=", SyntaxKind::GreaterThanEqualsToken, 2),
            (">>=", SyntaxKind::GreaterThanGreaterThanEqualsToken, 3),
            (">>>", SyntaxKind::GreaterThanGreaterThanGreaterThanToken, 3),
            (
                ">>>=",
                SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken,
                4,
            ),
        ];

        for (text, expected_kind, expected_end) in cases {
            let mut scanner = Scanner::new(text, LanguageVariant::Standard);
            assert_eq!(scanner.scan(), SyntaxKind::GreaterThanToken, "{text}");
            assert_eq!(scanner.re_scan_greater_token(), expected_kind, "{text}");
            assert_eq!(scanner.pos(), expected_end, "{text}");
        }

        let mut scanner = Scanner::new("<<", LanguageVariant::Standard);
        assert_eq!(scanner.scan(), SyntaxKind::LessThanLessThanToken);
        assert_eq!(scanner.re_scan_less_than_token(), SyntaxKind::LessThanToken);
        assert_eq!(scanner.pos(), 1);

        let mut scanner = Scanner::new("#x", LanguageVariant::Standard);
        assert_eq!(scanner.scan(), SyntaxKind::PrivateIdentifier);
        assert_eq!(scanner.re_scan_hash_token(), SyntaxKind::HashToken);
        assert_eq!(scanner.pos(), 1);
    }

    #[test]
    fn rescan_slash_regex_extent_oracle_pins() {
        struct Case {
            text: &'static str,
            first: SyntaxKind,
            end: usize,
            value: &'static str,
            flags: u32,
            errors: &'static [(usize, usize, u32)],
        }

        let cases = [
            Case {
                text: "/abc/g",
                first: SyntaxKind::SlashToken,
                end: 6,
                value: "/abc/g",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "/a[b\\/]c/i",
                first: SyntaxKind::SlashToken,
                end: 10,
                value: "/a[b\\/]c/i",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "/=x/",
                first: SyntaxKind::SlashEqualsToken,
                end: 4,
                value: "/=x/",
                flags: 0,
                errors: &[],
            },
            Case {
                text: "/unterminated",
                first: SyntaxKind::SlashToken,
                end: 13,
                value: "/unterminated",
                flags: 4,
                errors: &[(0, 13, 1161)],
            },
            Case {
                text: "/abc\nnext",
                first: SyntaxKind::SlashToken,
                end: 4,
                value: "/abc",
                flags: 4,
                errors: &[(0, 4, 1161)],
            },
        ];

        for case in cases {
            let mut scanner = Scanner::new(case.text, LanguageVariant::Standard);

            assert_eq!(scanner.scan(), case.first, "{}", case.text);
            assert_eq!(
                scanner.re_scan_slash_token(false),
                SyntaxKind::RegularExpressionLiteral,
                "{}",
                case.text
            );
            assert_eq!(scanner.pos(), case.end, "{}", case.text);
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

    #[test]
    fn jsx_scanner_oracle_pins() {
        struct JsxCase {
            text: &'static str,
            kind: SyntaxKind,
            end: usize,
            value: &'static str,
            errors: &'static [(usize, usize, u32)],
        }

        let cases = [
            JsxCase {
                text: "<div",
                kind: SyntaxKind::LessThanToken,
                end: 1,
                value: "",
                errors: &[],
            },
            JsxCase {
                text: "</div",
                kind: SyntaxKind::LessThanSlashToken,
                end: 2,
                value: "",
                errors: &[],
            },
            JsxCase {
                text: "hello {x}",
                kind: SyntaxKind::JsxText,
                end: 6,
                value: "hello ",
                errors: &[],
            },
            JsxCase {
                text: "  \n  <x",
                kind: SyntaxKind::JsxTextAllWhiteSpaces,
                end: 5,
                value: "  \n  ",
                errors: &[],
            },
            JsxCase {
                text: "a>b",
                kind: SyntaxKind::JsxText,
                end: 3,
                value: "a>b",
                errors: &[(1, 1, 1382)],
            },
            JsxCase {
                text: "a}b",
                kind: SyntaxKind::JsxText,
                end: 3,
                value: "a}b",
                errors: &[(1, 1, 1381)],
            },
        ];

        for case in cases {
            let mut scanner = Scanner::new(case.text, LanguageVariant::Jsx);

            assert_eq!(scanner.scan_jsx_token(true), case.kind, "{}", case.text);
            assert_eq!(scanner.pos(), case.end, "{}", case.text);
            assert_eq!(scanner.token_value, case.value, "{}", case.text);
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

    #[test]
    fn jsx_identifier_and_attribute_value_oracle_pins() {
        let mut scanner = Scanner::new("foo-bar", LanguageVariant::Jsx);
        assert_eq!(scanner.scan(), SyntaxKind::Identifier);
        assert_eq!(scanner.scan_jsx_identifier(), SyntaxKind::Identifier);
        assert_eq!(scanner.pos(), 7);
        assert_eq!(scanner.token_value, "foo-bar");

        let mut scanner = Scanner::new("class-name", LanguageVariant::Jsx);
        assert_eq!(scanner.scan(), SyntaxKind::ClassKeyword);
        assert_eq!(scanner.scan_jsx_identifier(), SyntaxKind::Identifier);
        assert_eq!(scanner.pos(), 10);
        assert_eq!(scanner.token_value, "class-name");

        let mut scanner = Scanner::new("\"a\\nb\"", LanguageVariant::Jsx);
        assert_eq!(
            scanner.scan_jsx_attribute_value(),
            SyntaxKind::StringLiteral
        );
        assert_eq!(scanner.pos(), 6);
        assert_eq!(scanner.token_value, "a\\nb");
        assert!(scanner.errors().is_empty());

        let mut scanner = Scanner::new("\"a\nb\"", LanguageVariant::Jsx);
        assert_eq!(
            scanner.scan_jsx_attribute_value(),
            SyntaxKind::StringLiteral
        );
        assert_eq!(scanner.pos(), 5);
        assert_eq!(scanner.token_value, "a\nb");
        assert!(scanner.errors().is_empty());
    }
}

//! Lexer producing tsc-compatible tokens with byte spans.

use crate::diagnostics::{gen, Diagnostic, MessageChain};
use wtf8::{CodePoint, Wtf8Buf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Eof,
    Unknown,
    Ident,
    NumLit,
    BigIntLit,
    StrLit,
    NoSubTemplate,
    PrivateIdent,
    JsxText,
    TemplateHead,
    TemplateMiddle,
    TemplateTail,
    RegexLit,
    // punctuation
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    Dot,
    DotDotDot,
    Semicolon,
    Comma,
    Lt,
    Gt,
    LtEq,
    GtEq,
    EqEq,
    BangEq,
    EqEqEq,
    BangEqEq,
    Arrow,
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Percent,
    PlusPlus,
    MinusMinus,
    LtLt,
    GtGt,
    GtGtGt,
    Amp,
    Bar,
    Caret,
    Bang,
    Tilde,
    AmpAmp,
    BarBar,
    Question,
    QuestionQuestion,
    QuestionDot,
    Colon,
    At,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    StarStarEq,
    SlashEq,
    PercentEq,
    LtLtEq,
    GtGtEq,
    GtGtGtEq,
    AmpEq,
    BarEq,
    CaretEq,
    AmpAmpEq,
    BarBarEq,
    QuestionQuestionEq,
    // keywords
    KAbstract,
    KAny,
    KAs,
    KAsync,
    KAwait,
    KBigint,
    KBoolean,
    KBreak,
    KCase,
    KCatch,
    KClass,
    KConst,
    KContinue,
    KDeclare,
    KDefault,
    KDelete,
    KDo,
    KElse,
    KEnum,
    KExport,
    KExtends,
    KFalse,
    KFinally,
    KFor,
    KFrom,
    KFunction,
    KGet,
    KIf,
    KImplements,
    KImport,
    KIn,
    KInstanceof,
    KInterface,
    KIs,
    KKeyof,
    KLet,
    KNever,
    KNew,
    KNull,
    KNumber,
    KObject,
    KOf,
    KOverride,
    KPrivate,
    KProtected,
    KPublic,
    KReadonly,
    KReturn,
    KSatisfies,
    KSet,
    KStatic,
    KString,
    KSuper,
    KSwitch,
    KSymbol,
    KThis,
    KThrow,
    KTrue,
    KTry,
    KType,
    KTypeof,
    KUndefined,
    KUnknown,
    KVar,
    KVoid,
    KWhile,
    KWith,
    KYield,
}

impl Tok {
    pub fn is_keyword(self) -> bool {
        (self as u32) >= (Tok::KAbstract as u32)
    }
    /// Keywords that may be used as identifiers (non-reserved).
    pub fn is_contextual_keyword(self) -> bool {
        use Tok::*;
        matches!(
            self,
            KAbstract
                | KAny
                | KAs
                | KAsync
                | KAwait
                | KBigint
                | KBoolean
                | KDeclare
                | KFrom
                | KGet
                | KIs
                | KKeyof
                | KLet
                | KNever
                | KNumber
                | KObject
                | KOf
                | KOverride
                | KReadonly
                | KSatisfies
                | KSet
                | KStatic
                | KString
                | KSymbol
                | KType
                | KUndefined
                | KUnknown
        )
    }
    /// Words reserved only in strict mode; usable as identifiers in sloppy
    /// code (a strict-mode use is a grammar error, not a parse error).
    pub fn is_strict_reserved_word(self) -> bool {
        use Tok::*;
        matches!(
            self,
            KImplements | KInterface | KPrivate | KProtected | KPublic | KYield
        )
    }
    pub fn text(self) -> &'static str {
        use Tok::*;
        match self {
            OpenBrace => "{",
            CloseBrace => "}",
            OpenParen => "(",
            CloseParen => ")",
            OpenBracket => "[",
            CloseBracket => "]",
            Dot => ".",
            DotDotDot => "...",
            Semicolon => ";",
            Comma => ",",
            Lt => "<",
            Gt => ">",
            LtEq => "<=",
            GtEq => ">=",
            EqEq => "==",
            BangEq => "!=",
            EqEqEq => "===",
            BangEqEq => "!==",
            Arrow => "=>",
            Plus => "+",
            Minus => "-",
            Star => "*",
            StarStar => "**",
            Slash => "/",
            Percent => "%",
            PlusPlus => "++",
            MinusMinus => "--",
            LtLt => "<<",
            GtGt => ">>",
            GtGtGt => ">>>",
            Amp => "&",
            Bar => "|",
            Caret => "^",
            Bang => "!",
            Tilde => "~",
            AmpAmp => "&&",
            BarBar => "||",
            Question => "?",
            QuestionQuestion => "??",
            QuestionDot => "?.",
            Colon => ":",
            At => "@",
            Eq => "=",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            StarStarEq => "**=",
            SlashEq => "/=",
            PercentEq => "%=",
            LtLtEq => "<<=",
            GtGtEq => ">>=",
            GtGtGtEq => ">>>=",
            AmpEq => "&=",
            BarEq => "|=",
            CaretEq => "^=",
            AmpAmpEq => "&&=",
            BarBarEq => "||=",
            QuestionQuestionEq => "??=",
            KAbstract => "abstract",
            KAny => "any",
            KAs => "as",
            KAsync => "async",
            KAwait => "await",
            KBigint => "bigint",
            KBoolean => "boolean",
            KBreak => "break",
            KCase => "case",
            KCatch => "catch",
            KClass => "class",
            KConst => "const",
            KContinue => "continue",
            KDeclare => "declare",
            KDefault => "default",
            KDelete => "delete",
            KDo => "do",
            KElse => "else",
            KEnum => "enum",
            KExport => "export",
            KExtends => "extends",
            KFalse => "false",
            KFinally => "finally",
            KFor => "for",
            KFrom => "from",
            KFunction => "function",
            KGet => "get",
            KIf => "if",
            KImplements => "implements",
            KImport => "import",
            KIn => "in",
            KInstanceof => "instanceof",
            KInterface => "interface",
            KIs => "is",
            KKeyof => "keyof",
            KLet => "let",
            KNever => "never",
            KNew => "new",
            KNull => "null",
            KNumber => "number",
            KObject => "object",
            KOf => "of",
            KOverride => "override",
            KPrivate => "private",
            KProtected => "protected",
            KPublic => "public",
            KReadonly => "readonly",
            KReturn => "return",
            KSatisfies => "satisfies",
            KSet => "set",
            KStatic => "static",
            KString => "string",
            KSuper => "super",
            KSwitch => "switch",
            KSymbol => "symbol",
            KThis => "this",
            KThrow => "throw",
            KTrue => "true",
            KTry => "try",
            KType => "type",
            KTypeof => "typeof",
            KUndefined => "undefined",
            KUnknown => "unknown",
            KVar => "var",
            KVoid => "void",
            KWhile => "while",
            KWith => "with",
            KYield => "yield",
            Eof => "<eof>",
            Unknown => "<unknown>",
            Ident => "<identifier>",
            NumLit => "<number>",
            BigIntLit => "<bigint>",
            StrLit => "<string>",
            NoSubTemplate | TemplateHead | TemplateMiddle | TemplateTail => "<template>",
            JsxText => "<jsx text>",
            PrivateIdent => "<private identifier>",
            RegexLit => "<regex>",
        }
    }
}

fn keyword_for(text: &str) -> Option<Tok> {
    use Tok::*;
    Some(match text {
        "abstract" => KAbstract,
        "any" => KAny,
        "as" => KAs,
        "async" => KAsync,
        "await" => KAwait,
        "bigint" => KBigint,
        "boolean" => KBoolean,
        "break" => KBreak,
        "case" => KCase,
        "catch" => KCatch,
        "class" => KClass,
        "const" => KConst,
        "continue" => KContinue,
        "declare" => KDeclare,
        "default" => KDefault,
        "delete" => KDelete,
        "do" => KDo,
        "else" => KElse,
        "enum" => KEnum,
        "export" => KExport,
        "extends" => KExtends,
        "false" => KFalse,
        "finally" => KFinally,
        "for" => KFor,
        "from" => KFrom,
        "function" => KFunction,
        "get" => KGet,
        "if" => KIf,
        "implements" => KImplements,
        "import" => KImport,
        "in" => KIn,
        "instanceof" => KInstanceof,
        "interface" => KInterface,
        "is" => KIs,
        "keyof" => KKeyof,
        "let" => KLet,
        "never" => KNever,
        "new" => KNew,
        "null" => KNull,
        "number" => KNumber,
        "object" => KObject,
        "of" => KOf,
        "override" => KOverride,
        "private" => KPrivate,
        "protected" => KProtected,
        "public" => KPublic,
        "readonly" => KReadonly,
        "return" => KReturn,
        "satisfies" => KSatisfies,
        "set" => KSet,
        "static" => KStatic,
        "string" => KString,
        "super" => KSuper,
        "switch" => KSwitch,
        "symbol" => KSymbol,
        "this" => KThis,
        "throw" => KThrow,
        "true" => KTrue,
        "try" => KTry,
        "type" => KType,
        "typeof" => KTypeof,
        "undefined" => KUndefined,
        "unknown" => KUnknown,
        "var" => KVar,
        "void" => KVoid,
        "while" => KWhile,
        "with" => KWith,
        "yield" => KYield,
        _ => return None,
    })
}

#[derive(Clone)]
pub struct ScannerState {
    pub pos: usize,
    pub token: Tok,
    pub token_start: usize,
    pub token_value: String,
    pub token_wtf8: Wtf8Buf,
    pub preceding_line_break: bool,
    pub diag_count: usize,
}

pub struct Scanner<'a> {
    pub text: &'a str,
    bytes: &'a [u8],
    pub pos: usize,
    pub token: Tok,
    pub token_start: usize,
    pub token_value: String,
    /// Faithful WTF-8 value of the current string/template token (may hold lone
    /// surrogates that `token_value` — a `String` — cannot). Only meaningful for
    /// StrLit/Template tokens; mirrors `token_value` for well-formed text.
    pub token_wtf8: Wtf8Buf,
    pub preceding_line_break: bool,
    pub file: usize,
    pub diags: Vec<Diagnostic>,
    /// When set, the next scanned string literal is a JSX attribute value, in
    /// which backslash is a literal character (no escape processing).
    pub jsx_attr_string: bool,
    /// (start, end, is_expect_error) of `@ts-…` comment directives
    pub comment_directives: Vec<(u32, u32, bool)>,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$' || (!c.is_ascii() && c.is_alphabetic())
}
/// Non-ASCII whitespace and line separators recognized by tsc's scanner
/// (`isWhiteSpaceLike`). ASCII whitespace is handled inline on the byte path;
/// this covers the multi-byte cases, including the BOM/ZWNBSP (U+FEFF).
fn is_unicode_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}
fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || c == '$'
        || (!c.is_ascii()
            && (c.is_alphanumeric()
                || c == '\u{200C}'
                || c == '\u{200D}'
                || is_id_continue_mark(c)))
}

/// Unicode Mn (Nonspacing Mark), Mc (Spacing Combining Mark), and Pc (Connector
/// Punctuation) code points that JS `ID_Continue` admits beyond
/// `char::is_alphanumeric`. Covers the common scripts (not exhaustive).
fn is_id_continue_mark(c: char) -> bool {
    matches!(c as u32,
        // Connector punctuation (Pc)
        0x203F..=0x2040 | 0x2054 | 0xFE33..=0xFE34 | 0xFE4D..=0xFE4F | 0xFF3F |
        // Combining Diacritical Marks / Cyrillic
        0x0300..=0x036F | 0x0483..=0x0489 |
        // Hebrew
        0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7 |
        // Arabic / Syriac / Thaana / NKo
        0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 |
        0x06E7..=0x06E8 | 0x06EA..=0x06ED | 0x0711 | 0x0730..=0x074A | 0x07A6..=0x07B0 |
        0x07EB..=0x07F3 | 0x07FD |
        // Devanagari
        0x0900..=0x0903 | 0x093A..=0x093C | 0x093E..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963 |
        // Bengali
        0x0981..=0x0983 | 0x09BC | 0x09BE..=0x09C4 | 0x09C7..=0x09C8 | 0x09CB..=0x09CD |
        0x09D7 | 0x09E2..=0x09E3 | 0x09FE |
        // Gurmukhi
        0x0A01..=0x0A03 | 0x0A3C | 0x0A3E..=0x0A42 | 0x0A47..=0x0A48 | 0x0A4B..=0x0A4D |
        0x0A51 | 0x0A70..=0x0A71 | 0x0A75 |
        // Gujarati
        0x0A81..=0x0A83 | 0x0ABC | 0x0ABE..=0x0AC5 | 0x0AC7..=0x0AC9 | 0x0ACB..=0x0ACD |
        0x0AE2..=0x0AE3 | 0x0AFA..=0x0AFF |
        // Oriya
        0x0B01..=0x0B03 | 0x0B3C | 0x0B3E..=0x0B44 | 0x0B47..=0x0B48 | 0x0B4B..=0x0B4D |
        0x0B55..=0x0B57 | 0x0B62..=0x0B63 |
        // Tamil
        0x0B82 | 0x0BBE..=0x0BC2 | 0x0BC6..=0x0BC8 | 0x0BCA..=0x0BCD | 0x0BD7 |
        // Telugu
        0x0C00..=0x0C04 | 0x0C3E..=0x0C44 | 0x0C46..=0x0C48 | 0x0C4A..=0x0C4D | 0x0C55..=0x0C56 |
        0x0C62..=0x0C63 |
        // Kannada
        0x0C81..=0x0C83 | 0x0CBC | 0x0CBE..=0x0CC4 | 0x0CC6..=0x0CC8 | 0x0CCA..=0x0CCD |
        0x0CD5..=0x0CD6 | 0x0CE2..=0x0CE3 |
        // Malayalam
        0x0D00..=0x0D03 | 0x0D3B..=0x0D3C | 0x0D3E..=0x0D44 | 0x0D46..=0x0D48 | 0x0D4A..=0x0D4D |
        0x0D57 | 0x0D62..=0x0D63 |
        // Sinhala
        0x0D81..=0x0D83 | 0x0DCA | 0x0DCF..=0x0DD4 | 0x0DD6 | 0x0DD8..=0x0DDF | 0x0DF2..=0x0DF3 |
        // Thai / Lao
        0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E | 0x0EB1 | 0x0EB4..=0x0EBC | 0x0EC8..=0x0ECD |
        // Tibetan
        0x0F18..=0x0F19 | 0x0F35 | 0x0F37 | 0x0F39 | 0x0F3E..=0x0F3F | 0x0F71..=0x0F84 |
        0x0F86..=0x0F87 | 0x0F8D..=0x0F97 | 0x0F99..=0x0FBC | 0x0FC6 |
        // Combining Half Marks / CJK voiced sound marks
        0xFE20..=0xFE2F | 0x3099..=0x309A
    )
}

/// WTF-8 buffer from well-formed UTF-8 text.
fn wb(s: &str) -> Wtf8Buf {
    Wtf8Buf::from_str(s)
}
/// WTF-8 buffer holding a single scalar value.
fn wb_char(c: char) -> Wtf8Buf {
    let mut b = Wtf8Buf::new();
    b.push_char(c);
    b
}
/// WTF-8 buffer holding the single code point `v` (a lone surrogate is kept as
/// such; a value outside 0..=0x10FFFF yields an empty buffer).
fn one_cp(v: u32) -> Wtf8Buf {
    let mut b = Wtf8Buf::new();
    if let Some(cp) = CodePoint::from_u32(v) {
        b.push(cp);
    }
    b
}

impl<'a> Scanner<'a> {
    pub fn new(text: &'a str, file: usize) -> Scanner<'a> {
        Scanner {
            text,
            bytes: text.as_bytes(),
            pos: 0,
            token: Tok::Unknown,
            token_start: 0,
            token_value: String::new(),
            token_wtf8: Wtf8Buf::new(),
            preceding_line_break: false,
            file,
            diags: Vec::new(),
            jsx_attr_string: false,
            comment_directives: Vec::new(),
        }
    }

    pub fn save(&self) -> ScannerState {
        ScannerState {
            pos: self.pos,
            token: self.token,
            token_start: self.token_start,
            token_value: self.token_value.clone(),
            token_wtf8: self.token_wtf8.clone(),
            preceding_line_break: self.preceding_line_break,
            diag_count: self.diags.len(),
        }
    }
    pub fn restore(&mut self, s: ScannerState) {
        self.pos = s.pos;
        self.token = s.token;
        self.token_start = s.token_start;
        self.token_value = s.token_value;
        self.token_wtf8 = s.token_wtf8;
        self.preceding_line_break = s.preceding_line_break;
        self.diags.truncate(s.diag_count);
    }

    fn error_at(
        &mut self,
        start: usize,
        length: usize,
        msg: &'static crate::diagnostics::DiagnosticMessage,
        args: &[String],
    ) {
        self.diags.push(Diagnostic {
            file: Some(self.file),
            start: start as u32,
            length: length as u32,
            message: MessageChain::new(msg, args),
            related: Vec::new(),
        });
    }

    fn ch(&self) -> u8 {
        if self.pos < self.bytes.len() {
            self.bytes[self.pos]
        } else {
            0
        }
    }
    /// Decode a `\u`-style escape that forms part of an identifier. `self.pos`
    /// points at the `u` (the backslash is already consumed). Returns the
    /// decoded character, advancing past the escape.
    fn scan_ident_unicode_escape(&mut self) -> Option<char> {
        if self.ch() != b'u' {
            return None;
        }
        let esc_start = self.pos - 1; // the backslash
        self.pos += 1; // u
        let v: u32 = if self.ch() == b'{' {
            self.pos += 1;
            let mut v: u32 = 0;
            while let Some(d) = (self.ch() as char).to_digit(16) {
                v = v.wrapping_mul(16).wrapping_add(d);
                self.pos += 1;
            }
            if self.ch() == b'}' {
                self.pos += 1;
            }
            v
        } else {
            let mut v: u32 = 0;
            for _ in 0..4 {
                if let Some(d) = (self.ch() as char).to_digit(16) {
                    v = v * 16 + d;
                    self.pos += 1;
                } else {
                    return None;
                }
            }
            v
        };
        // Unlike string literals, an identifier escape is NOT permitted to form a
        // surrogate pair: each `\uXXXX` must itself denote a valid identifier
        // code point. A surrogate value (or one past U+10FFFF) is therefore an
        // invalid character — `char::from_u32` rejects it, and tsc reports
        // TS1127 at the escape, which we mirror here.
        match char::from_u32(v) {
            Some(c) => Some(c),
            None => {
                self.error_at(esc_start, 0, &gen::Invalid_character, &[]);
                None
            }
        }
    }
    fn ch_at(&self, off: usize) -> u8 {
        if self.pos + off < self.bytes.len() {
            self.bytes[self.pos + off]
        } else {
            0
        }
    }
    fn char_at_pos(&self) -> char {
        self.text[self.pos..].chars().next().unwrap_or('\0')
    }

    pub fn token_end(&self) -> usize {
        self.pos
    }

    /// Scan the next token.
    pub fn scan(&mut self) -> Tok {
        self.preceding_line_break = false;
        self.token_value.clear();
        loop {
            self.token_start = self.pos;
            if self.pos >= self.bytes.len() {
                self.token = Tok::Eof;
                return self.token;
            }
            let c = self.ch();
            if matches!(c, b'<' | b'=' | b'>') && self.check_merge_marker() {
                continue;
            }
            match c {
                b' ' | b'\t' | 0x0B | 0x0C => {
                    self.pos += 1;
                    continue;
                }
                b'\n' => {
                    self.preceding_line_break = true;
                    self.pos += 1;
                    continue;
                }
                b'\r' => {
                    self.preceding_line_break = true;
                    self.pos += 1;
                    if self.ch() == b'\n' {
                        self.pos += 1;
                    }
                    continue;
                }
                b'/' => {
                    if self.ch_at(1) == b'/' {
                        let cstart = self.pos;
                        self.pos += 2;
                        while self.pos < self.bytes.len() && !matches!(self.ch(), b'\n' | b'\r') {
                            self.pos += 1;
                        }
                        // @ts-expect-error / @ts-ignore comment directives
                        let body = &self.text[cstart + 2..self.pos];
                        let trimmed = body.trim_start_matches('/').trim_start();
                        let expect = trimmed.starts_with("@ts-expect-error");
                        let ignore = trimmed.starts_with("@ts-ignore");
                        if expect || ignore {
                            self.comment_directives
                                .push((cstart as u32, self.pos as u32, expect));
                        }
                        continue;
                    }
                    if self.ch_at(1) == b'*' {
                        self.pos += 2;
                        let start = self.token_start;
                        let mut closed = false;
                        while self.pos < self.bytes.len() {
                            if self.ch() == b'*' && self.ch_at(1) == b'/' {
                                self.pos += 2;
                                closed = true;
                                break;
                            }
                            if matches!(self.ch(), b'\n' | b'\r') {
                                self.preceding_line_break = true;
                            }
                            self.pos += 1;
                        }
                        if !closed {
                            self.error_at(self.pos, 0, &gen::Asterisk_Slash_expected, &[]);
                            let _ = start;
                        }
                        continue;
                    }
                    // division or /=; regex rescans are parser-driven
                    self.pos += 1;
                    if self.ch() == b'=' {
                        self.pos += 1;
                        self.token = Tok::SlashEq;
                    } else {
                        self.token = Tok::Slash;
                    }
                    return self.token;
                }
                b'"' | b'\'' => {
                    let v = self.scan_string(c);
                    self.token_value = v.to_string_lossy().into_owned();
                    self.token_wtf8 = v;
                    self.token = Tok::StrLit;
                    return self.token;
                }
                b'`' => {
                    return self.scan_template(true);
                }
                b'0'..=b'9' => {
                    return self.scan_number();
                }
                b'.' => {
                    if self.ch_at(1).is_ascii_digit() {
                        return self.scan_number();
                    }
                    if self.ch_at(1) == b'.' && self.ch_at(2) == b'.' {
                        self.pos += 3;
                        self.token = Tok::DotDotDot;
                    } else {
                        self.pos += 1;
                        self.token = Tok::Dot;
                    }
                    return self.token;
                }
                b'(' => return self.punct(1, Tok::OpenParen),
                b')' => return self.punct(1, Tok::CloseParen),
                b'{' => return self.punct(1, Tok::OpenBrace),
                b'}' => return self.punct(1, Tok::CloseBrace),
                b'[' => return self.punct(1, Tok::OpenBracket),
                b']' => return self.punct(1, Tok::CloseBracket),
                b';' => return self.punct(1, Tok::Semicolon),
                b',' => return self.punct(1, Tok::Comma),
                b'@' => return self.punct(1, Tok::At),
                b'~' => return self.punct(1, Tok::Tilde),
                b':' => return self.punct(1, Tok::Colon),
                b'<' => {
                    if self.ch_at(1) == b'<' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::LtLtEq);
                        }
                        return self.punct(2, Tok::LtLt);
                    }
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::LtEq);
                    }
                    return self.punct(1, Tok::Lt);
                }
                b'>' => {
                    // '>' sequences are scanned as single '>' except in expressions;
                    // the parser calls rescan_greater when it wants shifts.
                    return self.punct(1, Tok::Gt);
                }
                b'=' => {
                    if self.ch_at(1) == b'=' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::EqEqEq);
                        }
                        return self.punct(2, Tok::EqEq);
                    }
                    if self.ch_at(1) == b'>' {
                        return self.punct(2, Tok::Arrow);
                    }
                    return self.punct(1, Tok::Eq);
                }
                b'!' => {
                    if self.ch_at(1) == b'=' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::BangEqEq);
                        }
                        return self.punct(2, Tok::BangEq);
                    }
                    return self.punct(1, Tok::Bang);
                }
                b'+' => {
                    if self.ch_at(1) == b'+' {
                        return self.punct(2, Tok::PlusPlus);
                    }
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::PlusEq);
                    }
                    return self.punct(1, Tok::Plus);
                }
                b'-' => {
                    if self.ch_at(1) == b'-' {
                        return self.punct(2, Tok::MinusMinus);
                    }
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::MinusEq);
                    }
                    return self.punct(1, Tok::Minus);
                }
                b'*' => {
                    if self.ch_at(1) == b'*' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::StarStarEq);
                        }
                        return self.punct(2, Tok::StarStar);
                    }
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::StarEq);
                    }
                    return self.punct(1, Tok::Star);
                }
                b'%' => {
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::PercentEq);
                    }
                    return self.punct(1, Tok::Percent);
                }
                b'&' => {
                    if self.ch_at(1) == b'&' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::AmpAmpEq);
                        }
                        return self.punct(2, Tok::AmpAmp);
                    }
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::AmpEq);
                    }
                    return self.punct(1, Tok::Amp);
                }
                b'|' => {
                    if self.ch_at(1) == b'|' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::BarBarEq);
                        }
                        return self.punct(2, Tok::BarBar);
                    }
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::BarEq);
                    }
                    return self.punct(1, Tok::Bar);
                }
                b'^' => {
                    if self.ch_at(1) == b'=' {
                        return self.punct(2, Tok::CaretEq);
                    }
                    return self.punct(1, Tok::Caret);
                }
                b'?' => {
                    if self.ch_at(1) == b'?' {
                        if self.ch_at(2) == b'=' {
                            return self.punct(3, Tok::QuestionQuestionEq);
                        }
                        return self.punct(2, Tok::QuestionQuestion);
                    }
                    if self.ch_at(1) == b'.' && !self.ch_at(2).is_ascii_digit() {
                        return self.punct(2, Tok::QuestionDot);
                    }
                    return self.punct(1, Tok::Question);
                }
                _ => {
                    let ch = self.char_at_pos();
                    // Unicode whitespace and line separators (tsc
                    // isWhiteSpaceLike): includes the BOM/ZWNBSP U+FEFF, so a
                    // leading byte-order mark is silently consumed.
                    if is_unicode_whitespace(ch) {
                        if matches!(ch, '\u{2028}' | '\u{2029}') {
                            self.preceding_line_break = true;
                        }
                        self.pos += ch.len_utf8();
                        continue;
                    }
                    if is_ident_start(ch) || (ch == '\\' && self.ch_at(1) == b'u') {
                        let start = self.pos;
                        let mut value = String::new();
                        let mut had_escape = false;
                        // first char (literal or `\u` escape)
                        if ch == '\\' {
                            had_escape = true;
                            let esc_start = self.pos;
                            self.pos += 1; // backslash
                            if let Some(c) = self.scan_ident_unicode_escape() {
                                // The escaped code point must itself be a valid
                                // identifier-start; tsc reports TS1127 otherwise
                                // (e.g. `\u{1F600}` — an emoji is not ID_Start).
                                if is_ident_start(c) {
                                    value.push(c);
                                } else {
                                    self.error_at(esc_start, 0, &gen::Invalid_character, &[]);
                                }
                            }
                        } else {
                            value.push(ch);
                            self.pos += ch.len_utf8();
                        }
                        // continuation chars
                        loop {
                            if self.pos >= self.bytes.len() {
                                break;
                            }
                            let c2 = self.char_at_pos();
                            if c2 == '\\' && self.ch_at(1) == b'u' {
                                had_escape = true;
                                let esc_start = self.pos;
                                self.pos += 1; // backslash
                                if let Some(c) = self.scan_ident_unicode_escape() {
                                    if is_ident_part(c) {
                                        value.push(c);
                                    } else {
                                        self.error_at(esc_start, 0, &gen::Invalid_character, &[]);
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else if is_ident_part(c2) {
                                value.push(c2);
                                self.pos += c2.len_utf8();
                            } else {
                                break;
                            }
                        }
                        self.token_value = value.clone();
                        // An identifier written with escapes is never a reserved
                        // word (e.g. `\u0069f` is the identifier `if`).
                        let _ = start;
                        self.token = if had_escape {
                            Tok::Ident
                        } else {
                            keyword_for(&value).unwrap_or(Tok::Ident)
                        };
                        return self.token;
                    }
                    // #private identifier
                    if ch == '#' {
                        let start = self.pos;
                        self.pos += 1;
                        if self.pos < self.bytes.len() && is_ident_start(self.char_at_pos()) {
                            while self.pos < self.bytes.len() && is_ident_part(self.char_at_pos()) {
                                self.pos += self.char_at_pos().len_utf8();
                            }
                            self.token_value = self.text[start..self.pos].to_string();
                            self.token = Tok::PrivateIdent;
                            return self.token;
                        }
                        self.pos = start;
                    }
                    // invalid character
                    self.error_at(self.pos, ch.len_utf8(), &gen::Invalid_character, &[]);
                    self.pos += ch.len_utf8();
                    self.token = Tok::Unknown;
                    return self.token;
                }
            }
        }
    }

    fn punct(&mut self, len: usize, tok: Tok) -> Tok {
        self.pos += len;
        self.token = tok;
        self.token = tok;
        tok
    }

    /// In type-argument / binary-operator position the parser may need to turn
    /// a previously scanned `>` into >>, >>>, >=, >>=, >>>=.
    pub fn rescan_greater(&mut self) -> Tok {
        debug_assert_eq!(self.token, Tok::Gt);
        if self.ch() == b'>' {
            if self.ch_at(1) == b'>' {
                if self.ch_at(2) == b'=' {
                    return self.punct(3, Tok::GtGtGtEq);
                }
                return self.punct(2, Tok::GtGtGt);
            }
            if self.ch_at(1) == b'=' {
                return self.punct(2, Tok::GtGtEq);
            }
            return self.punct(1, Tok::GtGt);
        }
        if self.ch() == b'=' {
            return self.punct(1, Tok::GtEq);
        }
        self.token
    }

    /// merge conflict markers at line starts (<<<<<<< ======= >>>>>>>)
    fn check_merge_marker(&mut self) -> bool {
        let c = self.ch();
        if !matches!(c, b'<' | b'=' | b'>') {
            return false;
        }
        let at_line_start = self.pos == 0 || matches!(self.bytes[self.pos - 1], b'\n' | b'\r');
        if !at_line_start {
            return false;
        }
        let mut n = 0;
        while self.pos + n < self.bytes.len() && self.bytes[self.pos + n] == c {
            n += 1;
        }
        if n < 7 {
            return false;
        }
        self.error_at(self.pos, 7, &gen::Merge_conflict_marker_encountered, &[]);
        // skip the marker line entirely
        while self.pos < self.bytes.len() && !matches!(self.ch(), b'\n' | b'\r') {
            self.pos += 1;
        }
        true
    }

    /// JSX children: raw text starting at the current token (which was
    /// pre-scanned by a normal `next()` and must be re-interpreted as text).
    pub fn rescan_jsx_text(&mut self) -> Tok {
        self.pos = self.token_start;
        self.scan_jsx_text()
    }

    /// JSX children: raw text from the current position until `<`, `{` or EOF.
    /// Call while positioned just past a `>`/`}` token (no lookahead consumed).
    pub fn scan_jsx_text(&mut self) -> Tok {
        self.token_start = self.pos;
        let start = self.pos;
        while self.pos < self.bytes.len() && !matches!(self.ch(), b'<' | b'{' | b'}') {
            self.pos += 1;
        }
        self.token_value = self.text[start..self.pos].to_string();
        self.token = Tok::JsxText;
        self.token
    }

    /// Rescan `/` or `/=` as a regular expression literal (expression position).
    pub fn rescan_slash_as_regex(&mut self) -> Tok {
        debug_assert!(matches!(self.token, Tok::Slash | Tok::SlashEq));
        let start = self.token_start;
        self.pos = start + 1;
        let mut in_class = false;
        let mut closed = false;
        while self.pos < self.bytes.len() {
            let c = self.ch();
            match c {
                b'\n' | b'\r' => break,
                b'\\' => {
                    self.pos += 2;
                    continue;
                }
                b'[' => in_class = true,
                b']' => in_class = false,
                b'/' if !in_class => {
                    self.pos += 1;
                    closed = true;
                    break;
                }
                _ => {}
            }
            self.pos += 1;
        }
        if !closed {
            self.error_at(
                self.token_start,
                0,
                &gen::Unterminated_regular_expression_literal,
                &[],
            );
        } else {
            // character-class ranges out of order: [z-a] (1517)
            {
                let b = self.text[start..self.pos].as_bytes().to_vec();
                let mut i = 0;
                let mut in_class = false;
                while i < b.len() {
                    match b[i] {
                        b'\\' => {
                            i += 2;
                            continue;
                        }
                        b'[' => in_class = true,
                        b']' => in_class = false,
                        b'-' if in_class && i > 0 && i + 1 < b.len() => {
                            let lo = b[i - 1];
                            let hi = b[i + 1];
                            if lo.is_ascii_alphanumeric() && hi.is_ascii_alphanumeric() && lo > hi {
                                self.error_at(
                                    start + i - 1,
                                    1,
                                    &gen::Range_out_of_order_in_character_class,
                                    &[],
                                );
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            // quantifier bounds out of order: {m,n} with m > n (1506)
            {
                let b = self.text[start..self.pos].as_bytes().to_vec();
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b'{' && (i == 0 || b[i - 1] != b'\\') {
                        let mut j = i + 1;
                        let mut first = String::new();
                        while j < b.len() && b[j].is_ascii_digit() {
                            first.push(b[j] as char);
                            j += 1;
                        }
                        if !first.is_empty() && j < b.len() && b[j] == b',' {
                            j += 1;
                            let snd_start = j;
                            let mut second = String::new();
                            while j < b.len() && b[j].is_ascii_digit() {
                                second.push(b[j] as char);
                                j += 1;
                            }
                            let _ = snd_start;
                            if !second.is_empty() && j < b.len() && b[j] == b'}' {
                                if let (Ok(m), Ok(n)) =
                                    (first.parse::<u64>(), second.parse::<u64>())
                                {
                                    if m > n {
                                        self.error_at(
                                            start + i + 1,
                                            first.len(),
                                            &gen::Numbers_out_of_order_in_quantifier,
                                            &[],
                                        );
                                    }
                                }
                            }
                        }
                    }
                    i += 1;
                }
            }
            // Named-group availability (TS1503), duplicate names, and
            // `\\k<name>` resolution are checker concerns, not scan-time errors.

            // flags — validity (unknown/duplicate flags, TS1499/1500) and
            // target availability (TS1501) are checker concerns; just consume
            // the flag characters here.
            while self.pos < self.bytes.len() && is_ident_part(self.char_at_pos()) {
                let c = self.char_at_pos();
                self.pos += c.len_utf8();
            }
        }
        self.token_value = self.text[start..self.pos].to_string();
        self.token = Tok::RegexLit;
        self.token
    }

    fn scan_string(&mut self, quote: u8) -> Wtf8Buf {
        let jsx = self.jsx_attr_string;
        self.pos += 1;
        let mut value = Wtf8Buf::new();
        loop {
            if self.pos >= self.bytes.len() {
                self.error_at(self.pos, 0, &gen::Unterminated_string_literal, &[]);
                break;
            }
            let c = self.ch();
            if c == quote {
                self.pos += 1;
                break;
            }
            match c {
                // In a JSX attribute value, backslash is an ordinary character.
                b'\\' if !jsx => {
                    let e = self.scan_escape();
                    value.push_wtf8(&e);
                }
                b'\n' | b'\r' => {
                    self.error_at(self.pos, 0, &gen::Unterminated_string_literal, &[]);
                    break;
                }
                _ => {
                    let ch = self.char_at_pos();
                    value.push_char(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        value
    }

    /// Scan a string/template escape, returning its WTF-8 value. Lone surrogate
    /// escapes (`\uD800`, `\u{D800}`) are preserved faithfully — JS keeps them as
    /// UTF-16 units — rather than being lossily folded to U+FFFD.
    fn scan_escape(&mut self) -> Wtf8Buf {
        self.pos += 1; // backslash
        if self.pos >= self.bytes.len() {
            return Wtf8Buf::new();
        }
        let c = self.ch();
        self.pos += 1;
        match c {
            b'0' if !self.ch().is_ascii_digit() => wb("\0"),
            b'b' => wb("\u{8}"),
            b't' => wb("\t"),
            b'n' => wb("\n"),
            b'v' => wb("\u{B}"),
            b'f' => wb("\u{C}"),
            b'r' => wb("\r"),
            b'\'' => wb("'"),
            b'"' => wb("\""),
            b'`' => wb("`"),
            b'\\' => wb("\\"),
            b'\r' => {
                if self.ch() == b'\n' {
                    self.pos += 1;
                }
                Wtf8Buf::new()
            }
            b'\n' => Wtf8Buf::new(),
            b'x' => {
                let mut v = 0u32;
                for _ in 0..2 {
                    if let Some(d) = (self.ch() as char).to_digit(16) {
                        v = v * 16 + d;
                        self.pos += 1;
                    } else {
                        self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected, &[]);
                        return Wtf8Buf::new();
                    }
                }
                one_cp(v)
            }
            b'u' => {
                if self.ch() == b'{' {
                    self.pos += 1;
                    let vstart = self.pos;
                    let mut v = 0u64;
                    let mut any = false;
                    while let Some(d) = (self.ch() as char).to_digit(16) {
                        v = v * 16 + d as u64;
                        self.pos += 1;
                        any = true;
                    }
                    if !any {
                        self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected, &[]);
                    }
                    if v > 0x10FFFF {
                        self.error_at(
                            vstart,
                            self.pos - vstart,
                            &gen::An_extended_Unicode_escape_value_must_be_between_0x0_and_0x10FFFF_inclusive,
                            &[],
                        );
                    }
                    if self.ch() == b'}' {
                        self.pos += 1;
                    } else {
                        self.error_at(self.pos, 0, &gen::Unterminated_Unicode_escape_sequence, &[]);
                    }
                    // `\u{D800}` is a permitted way to write a lone surrogate.
                    one_cp(v as u32)
                } else {
                    let mut v = 0u32;
                    for _ in 0..4 {
                        if let Some(d) = (self.ch() as char).to_digit(16) {
                            v = v * 16 + d;
                            self.pos += 1;
                        } else {
                            self.error_at(self.pos, 0, &gen::Hexadecimal_digit_expected, &[]);
                            return Wtf8Buf::new();
                        }
                    }
                    // A high surrogate immediately followed by a `\uXXXX` low
                    // surrogate forms one code point, exactly as JS combines
                    // `"\uD842\uDFB7"` into U+20BB7. (`\u{…}` is always a single
                    // code point, so it is excluded as the pair's second half.)
                    if (0xD800..=0xDBFF).contains(&v)
                        && self.ch() == b'\\'
                        && self.ch_at(1) == b'u'
                        && self.ch_at(2) != b'{'
                    {
                        let save = self.pos;
                        self.pos += 2; // \u
                        let mut lo = 0u32;
                        let mut ok = true;
                        for _ in 0..4 {
                            if let Some(d) = (self.ch() as char).to_digit(16) {
                                lo = lo * 16 + d;
                                self.pos += 1;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        if ok && (0xDC00..=0xDFFF).contains(&lo) {
                            let cp = 0x10000 + ((v - 0xD800) << 10) + (lo - 0xDC00);
                            return one_cp(cp);
                        }
                        self.pos = save; // not a valid low surrogate; rewind
                    }
                    // A lone surrogate is preserved as a UTF-16 unit (WTF-8),
                    // matching the JS string value instead of dropping it.
                    one_cp(v)
                }
            }
            _ => {
                let prev = self.pos - 1;
                let ch = self.text[prev..].chars().next().unwrap_or('\0');
                self.pos = prev + ch.len_utf8();
                wb_char(ch)
            }
        }
    }

    /// `start_backtick`: true when at `` ` ``, false when rescanning after `}`
    /// of a template span (parser calls with token at CloseBrace).
    pub fn scan_template(&mut self, start_backtick: bool) -> Tok {
        let head = start_backtick;
        if start_backtick {
            debug_assert_eq!(self.ch(), b'`');
        } else {
            // rescan from the `}` that closed the substitution
            self.pos = self.token_start;
            debug_assert_eq!(self.ch(), b'}');
        }
        self.token_start = self.pos;
        self.pos += 1; // ` or }
        let mut value = Wtf8Buf::new();
        loop {
            if self.pos >= self.bytes.len() {
                self.error_at(self.pos, 0, &gen::Unterminated_template_literal, &[]);
                self.token = if head {
                    Tok::NoSubTemplate
                } else {
                    Tok::TemplateTail
                };
                break;
            }
            let c = self.ch();
            if c == b'`' {
                self.pos += 1;
                self.token = if head {
                    Tok::NoSubTemplate
                } else {
                    Tok::TemplateTail
                };
                break;
            }
            if c == b'$' && self.ch_at(1) == b'{' {
                self.pos += 2;
                self.token = if head {
                    Tok::TemplateHead
                } else {
                    Tok::TemplateMiddle
                };
                break;
            }
            if c == b'\\' {
                let e = self.scan_escape();
                value.push_wtf8(&e);
                continue;
            }
            let ch = self.char_at_pos();
            value.push_char(ch);
            self.pos += ch.len_utf8();
        }
        self.token_value = value.to_string_lossy().into_owned();
        self.token_wtf8 = value;
        self.token
    }

    /// digit run with numeric-separator validation (6188/6189)
    fn scan_digits_with_separators(&mut self, _leading: bool) {
        let mut prev_sep = false;
        let mut any_digit = false;
        loop {
            let c = self.ch();
            if c == b'_' {
                if prev_sep {
                    self.error_at(
                        self.pos,
                        1,
                        &gen::Multiple_consecutive_numeric_separators_are_not_permitted,
                        &[],
                    );
                } else if !any_digit {
                    self.error_at(
                        self.pos,
                        1,
                        &gen::Numeric_separators_are_not_allowed_here,
                        &[],
                    );
                }
                prev_sep = true;
                self.pos += 1;
                continue;
            }
            if c.is_ascii_digit() {
                any_digit = true;
                prev_sep = false;
                self.pos += 1;
                continue;
            }
            break;
        }
        if prev_sep {
            self.error_at(
                self.pos - 1,
                1,
                &gen::Numeric_separators_are_not_allowed_here,
                &[],
            );
        }
    }

    fn scan_number(&mut self) -> Tok {
        let start = self.pos;
        let mut is_int_radix = false;
        if self.ch() == b'0' && matches!(self.ch_at(1), b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
            is_int_radix = true;
            let radix: u32 = match self.ch_at(1) {
                b'x' | b'X' => 16,
                b'o' | b'O' => 8,
                _ => 2,
            };
            self.pos += 2;
            let digits_start = self.pos;
            let mut value: f64 = 0.0;
            while self.pos < self.bytes.len() {
                let c = self.ch() as char;
                if c == '_' {
                    self.pos += 1;
                    continue;
                }
                if let Some(d) = c.to_digit(radix) {
                    value = value * radix as f64 + d as f64;
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.pos == digits_start {
                let msg = match radix {
                    16 => &gen::Hexadecimal_digit_expected,
                    8 => &gen::Octal_digit_expected,
                    _ => &gen::Binary_digit_expected,
                };
                self.error_at(self.pos, 0, msg, &[]);
            }
            self.token_value = value.to_string();
        } else if self.ch() == b'0' && self.ch_at(1).is_ascii_digit() {
            // legacy octal literal (010): error + parse as octal
            self.pos += 1;
            let digits_start = self.pos;
            let mut value: f64 = 0.0;
            let mut pure_octal = true;
            while self.ch().is_ascii_digit() {
                if self.ch() >= b'8' {
                    pure_octal = false;
                }
                value = value * 8.0 + (self.ch() - b'0') as f64;
                self.pos += 1;
            }
            let digits = &self.text[digits_start..self.pos];
            if pure_octal {
                self.error_at(
                    start,
                    self.pos - start,
                    &gen::Octal_literals_are_not_allowed_Use_the_syntax_0,
                    &[format!("0o{}", digits)],
                );
                self.token_value = crate::js_num::to_js_string(value);
            } else {
                // leading-zero decimal: keep decimal value (unfixtured edge)
                self.token_value = digits.trim_start_matches('0').to_string();
                if self.token_value.is_empty() {
                    self.token_value = "0".to_string();
                }
            }
            self.token = Tok::NumLit;
            return self.token;
        } else {
            self.scan_digits_with_separators(true);
            if self.ch() == b'.' {
                self.pos += 1;
                self.scan_digits_with_separators(false);
            }
            if matches!(self.ch(), b'e' | b'E') {
                let save = self.pos;
                self.pos += 1;
                if matches!(self.ch(), b'+' | b'-') {
                    self.pos += 1;
                }
                if self.ch().is_ascii_digit() {
                    self.scan_digits_with_separators(false);
                } else {
                    self.pos = save;
                    self.error_at(self.pos + 1, 0, &gen::Digit_expected, &[]);
                    self.pos = save + 1;
                }
            }
            let raw: String = self.text[start..self.pos]
                .chars()
                .filter(|c| *c != '_')
                .collect();
            self.token_value = raw;
        }
        if self.ch() == b'n' {
            if is_int_radix_float(&self.token_value) {
                // 1.5n — bigint literals must be integers (1353)
                self.pos += 1;
                self.error_at(
                    start,
                    self.pos - start,
                    &gen::A_bigint_literal_must_be_an_integer,
                    &[],
                );
                self.token = Tok::NumLit;
                return self.token;
            }
            self.pos += 1;
            self.token = Tok::BigIntLit;
            self.token_value = self.text[start..self.pos]
                .chars()
                .filter(|c| *c != '_')
                .collect();
            return self.token;
        }
        let _ = is_int_radix;
        // An identifier or keyword cannot immediately follow a numeric literal.
        if self.pos < self.bytes.len() && is_ident_start(self.char_at_pos()) {
            self.error_at(
                self.pos,
                self.char_at_pos().len_utf8(),
                &gen::An_identifier_or_keyword_cannot_immediately_follow_a_numeric_literal,
                &[],
            );
        }
        self.token = Tok::NumLit;
        self.token
    }
}

fn is_int_radix_float(s: &str) -> bool {
    s.contains('.') || s.contains('e') || s.contains('E')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<(Tok, String)> {
        let mut s = Scanner::new(src, 0);
        let mut out = Vec::new();
        loop {
            let t = s.scan();
            if t == Tok::Eof {
                break;
            }
            out.push((t, s.token_value.clone()));
        }
        out
    }

    #[test]
    fn basic_tokens() {
        let t = toks("const x: number = 1.5;");
        assert_eq!(t[0].0, Tok::KConst);
        assert_eq!(t[1], (Tok::Ident, "x".to_string()));
        assert_eq!(t[2].0, Tok::Colon);
        assert_eq!(t[3].0, Tok::KNumber);
        assert_eq!(t[4].0, Tok::Eq);
        assert_eq!(t[5], (Tok::NumLit, "1.5".to_string()));
        assert_eq!(t[6].0, Tok::Semicolon);
    }

    #[test]
    fn strings_and_unterminated() {
        let mut s = Scanner::new("\"ab\\nc\"", 0);
        assert_eq!(s.scan(), Tok::StrLit);
        assert_eq!(s.token_value, "ab\nc");

        let mut s = Scanner::new("\"abc", 0);
        s.scan();
        assert_eq!(s.diags.len(), 1);
        assert_eq!(s.diags[0].code(), 1002);
    }

    #[test]
    fn punctuation_maximal_munch() {
        let t = toks("a === c ?? d?.e ... ** &&=");
        let kinds: Vec<Tok> = t.iter().map(|x| x.0).collect();
        assert!(kinds.contains(&Tok::EqEqEq));
        assert!(kinds.contains(&Tok::QuestionQuestion));
        assert!(kinds.contains(&Tok::QuestionDot));
        assert!(kinds.contains(&Tok::DotDotDot));
        assert!(kinds.contains(&Tok::StarStar));
        assert!(kinds.contains(&Tok::AmpAmpEq));
    }

    #[test]
    fn rescan_greater_like_parser() {
        // scanner yields single `>`; the parser rescans in operator position
        let mut s = Scanner::new("a >= b >> c", 0);
        assert_eq!(s.scan(), Tok::Ident);
        assert_eq!(s.scan(), Tok::Gt);
        assert_eq!(s.rescan_greater(), Tok::GtEq);
        assert_eq!(s.scan(), Tok::Ident);
        assert_eq!(s.scan(), Tok::Gt);
        assert_eq!(s.rescan_greater(), Tok::GtGt);
    }

    #[test]
    fn template_parts() {
        let mut s = Scanner::new("`a${x}b`", 0);
        assert_eq!(s.scan(), Tok::TemplateHead);
        assert_eq!(s.token_value, "a");
        assert_eq!(s.scan(), Tok::Ident);
        assert_eq!(s.scan(), Tok::CloseBrace);
        assert_eq!(s.scan_template(false), Tok::TemplateTail);
        assert_eq!(s.token_value, "b");
    }

    #[test]
    fn unterminated_template() {
        let mut s = Scanner::new("`abc", 0);
        s.scan();
        assert_eq!(s.diags[0].code(), 1160);
    }

    #[test]
    fn numbers() {
        let t = toks("0x10 0b11 1_000 1e3 2n");
        assert_eq!(t[0], (Tok::NumLit, "16".to_string()));
        assert_eq!(t[1], (Tok::NumLit, "3".to_string()));
        assert_eq!(t[2], (Tok::NumLit, "1000".to_string()));
        assert_eq!(t[3], (Tok::NumLit, "1e3".to_string()));
        assert_eq!(t[4].0, Tok::BigIntLit);
    }
}

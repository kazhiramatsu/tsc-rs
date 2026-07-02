//! Regular-expression validation: a verbatim port of tsc's
//! checkGrammarRegularExpressionLiteral → reScanSlashToken(reportErrors=true)
//! → scanRegularExpressionWorker pipeline (scanner.ts).
//!
//! The parser scans regex literals without reporting (only the
//! unterminated-literal error is a parse diagnostic); when the checker reaches
//! a RegularExpressionLiteral it re-scans the token with errors enabled, so
//! every diagnostic here lands in the SEMANTIC bucket. tsc gates the re-scan
//! on `!hasParseDiagnostics(sourceFile)`; tsrs never runs the checker when any
//! parse diagnostic exists, which subsumes that gate.
//!
//! Diagnostic emission replicates the onError closure installed by
//! checkGrammarRegularExpressionLiteral: at most one error per start position,
//! compared against the LAST regex error of this literal only. Message-category
//! follow-ups (Did_you_mean_0 from getSpellingSuggestion) become
//! relatedInformation on the previous error in tsc — invisible in plain
//! formatting — and are therefore not emitted at all (they never update the
//! last-error position either, so suppression state is unaffected).
//!
//! Positions are byte offsets into the token text added to the token's span
//! start. tsc operates on UTF-16 offsets of the whole file; the two agree for
//! BMP text (the output pipeline converts to UTF-16 columns later), and
//! charCodeChecked is modeled as "full code point" rather than "UTF-16 unit",
//! which only diverges on astral characters in non-unicode-mode patterns.

use crate::diagnostics::{gen, Diagnostic, DiagnosticMessage, MessageChain};
use crate::scanner::{is_ident_part, is_ident_start};

// RegularExpressionFlags (tsc ast.ts)
const FLAG_HAS_INDICES: u32 = 1; // d
const FLAG_GLOBAL: u32 = 2; // g
const FLAG_IGNORE_CASE: u32 = 4; // i
const FLAG_MULTILINE: u32 = 8; // m
const FLAG_DOT_ALL: u32 = 16; // s
const FLAG_UNICODE: u32 = 32; // u
const FLAG_UNICODE_SETS: u32 = 64; // v
const FLAG_STICKY: u32 = 128; // y
const FLAG_MODIFIERS: u32 = FLAG_IGNORE_CASE | FLAG_MULTILINE | FLAG_DOT_ALL;
const FLAG_ANY_UNICODE_MODE: u32 = FLAG_UNICODE | FLAG_UNICODE_SETS;

// EscapeSequenceScanningFlags (tsc scanner.ts)
const ESC_STRING: u32 = 1;
const ESC_REGULAR_EXPRESSION: u32 = 4;
const ESC_ANNEX_B: u32 = 8;
const ESC_ANY_UNICODE_MODE: u32 = 16;
const ESC_ATOM_ESCAPE: u32 = 32;
const ESC_ALLOW_EXTENDED_UNICODE_ESCAPE: u32 = ESC_STRING | ESC_ANY_UNICODE_MODE;

fn char_to_flag(c: i32) -> Option<u32> {
    match c {
        0x64 => Some(FLAG_HAS_INDICES),
        0x67 => Some(FLAG_GLOBAL),
        0x69 => Some(FLAG_IGNORE_CASE),
        0x6D => Some(FLAG_MULTILINE),
        0x73 => Some(FLAG_DOT_ALL),
        0x75 => Some(FLAG_UNICODE),
        0x76 => Some(FLAG_UNICODE_SETS),
        0x79 => Some(FLAG_STICKY),
        _ => None,
    }
}

/// regExpFlagToFirstAvailableLanguageVersion + getNameOfScriptTarget
fn flag_minimum_target(flag: u32) -> Option<(u32, &'static str)> {
    match flag {
        FLAG_HAS_INDICES => Some((9, "es2022")),
        FLAG_DOT_ALL => Some((5, "es2018")),
        FLAG_UNICODE => Some((2, "es6")),
        FLAG_UNICODE_SETS => Some((11, "es2024")),
        FLAG_STICKY => Some((2, "es6")),
        _ => None,
    }
}

pub(crate) struct RegexCheck<'a> {
    b: &'a [u8],
    s: &'a str,
    pos: usize,
    /// scan limit (scanRange): body end during the worker, token end for flags
    end: usize,
    base: u32,
    file: usize,
    language_version: u32,
    // worker state (scanRegularExpressionWorker locals; annexB is always true
    // at the rescan call site, so anyUnicodeModeOrNonAnnexB == any_unicode_mode)
    unicode_sets_mode: bool,
    any_unicode_mode: bool,
    named_capture_groups: bool,
    may_contain_strings: bool,
    num_capturing_groups: u32,
    group_specifiers: Vec<String>,
    group_name_refs: Vec<(usize, usize, String)>,
    decimal_escapes: Vec<(usize, usize, f64)>,
    scope_stack: Vec<Vec<String>>,
    top_scope: Vec<String>,
    token_value: String,
    diags: &'a mut Vec<Diagnostic>,
    last_error_start: Option<u32>,
}

/// Entry point: validate one regex literal token (`/body/flags`) whose text
/// starts at `base` in file `file`. The token is terminated (an unterminated
/// literal produces a parse diagnostic, which keeps the checker from running).
pub(crate) fn check_grammar_regex_literal(
    token: &str,
    base: u32,
    file: usize,
    language_version: u32,
    diags: &mut Vec<Diagnostic>,
) {
    let b = token.as_bytes();
    // re-derive endOfRegExpBody and namedCaptureGroups exactly like
    // reScanSlashToken's body pre-scan
    let mut pos = 1;
    let mut in_escape = false;
    let mut in_class = false;
    let mut named_capture_groups = false;
    let end_of_body = loop {
        if pos >= b.len() {
            return; // defensive: unterminated never reaches the checker
        }
        let c = b[pos];
        if in_escape {
            in_escape = false;
        } else if c == b'/' && !in_class {
            break pos;
        } else if c == b'[' {
            in_class = true;
        } else if c == b'\\' {
            in_escape = true;
        } else if c == b']' {
            in_class = false;
        } else if !in_class
            && c == b'('
            && b.get(pos + 1) == Some(&b'?')
            && b.get(pos + 2) == Some(&b'<')
            && b.get(pos + 3).map_or(false, |c| *c != b'=' && *c != b'!')
        {
            named_capture_groups = true;
        }
        pos += 1;
    };

    let mut rc = RegexCheck {
        b,
        s: token,
        pos: end_of_body + 1,
        end: token.len(),
        base,
        file,
        language_version,
        unicode_sets_mode: false,
        any_unicode_mode: false,
        named_capture_groups,
        may_contain_strings: false,
        num_capturing_groups: 0,
        group_specifiers: Vec::new(),
        group_name_refs: Vec::new(),
        decimal_escapes: Vec::new(),
        scope_stack: Vec::new(),
        top_scope: Vec::new(),
        token_value: String::new(),
        diags,
        last_error_start: None,
    };

    // flag scan with errors (the reportErrors arm of reScanSlashToken)
    let mut regexp_flags = 0u32;
    loop {
        let ch = rc.ch(rc.pos);
        if ch == -1 || !is_ident_part_cp(ch) {
            break;
        }
        let size = char_size(ch);
        match char_to_flag(ch) {
            None => rc.error_at(&gen::Unknown_regular_expression_flag, rc.pos, size, &[]),
            Some(flag) => {
                if regexp_flags & flag != 0 {
                    rc.error_at(&gen::Duplicate_regular_expression_flag, rc.pos, size, &[]);
                } else if (regexp_flags | flag) & FLAG_ANY_UNICODE_MODE == FLAG_ANY_UNICODE_MODE {
                    rc.error_at(
                        &gen::The_Unicode_u_flag_and_the_Unicode_Sets_v_flag_cannot_be_set_simultaneously,
                        rc.pos,
                        size,
                        &[],
                    );
                } else {
                    regexp_flags |= flag;
                    rc.check_flag_availability(flag, size);
                }
            }
        }
        rc.pos += size;
    }

    // scanRange(startOfRegExpBody, …, () => scanRegularExpressionWorker(...))
    rc.unicode_sets_mode = regexp_flags & FLAG_UNICODE_SETS != 0;
    rc.any_unicode_mode = regexp_flags & FLAG_ANY_UNICODE_MODE != 0;
    rc.pos = 1;
    rc.end = end_of_body;
    rc.scan_disjunction(false);

    // post-scan reference checks (suggestions are Message-category → invisible)
    let refs = std::mem::take(&mut rc.group_name_refs);
    for (start, end, name) in refs {
        if !rc.group_specifiers.iter().any(|g| g == &name) {
            rc.error_at(
                &gen::There_is_no_capturing_group_named_0_in_this_regular_expression,
                start,
                end - start,
                &[name],
            );
        }
    }
    let escapes = std::mem::take(&mut rc.decimal_escapes);
    for (start, end, value) in escapes {
        if value > rc.num_capturing_groups as f64 {
            if rc.num_capturing_groups > 0 {
                rc.error_at(
                    &gen::This_backreference_refers_to_a_group_that_does_not_exist_There_are_only_0_capturing_groups_in_this_regular_expression,
                    start,
                    end - start,
                    &[rc.num_capturing_groups.to_string()],
                );
            } else {
                rc.error_at(
                    &gen::This_backreference_refers_to_a_group_that_does_not_exist_There_are_no_capturing_groups_in_this_regular_expression,
                    start,
                    end - start,
                    &[],
                );
            }
        }
    }
}

fn is_ident_part_cp(ch: i32) -> bool {
    char::from_u32(ch as u32).map_or(false, is_ident_part)
}

fn char_size(ch: i32) -> usize {
    char::from_u32(ch as u32).map_or(1, |c| c.len_utf8())
}

fn from_char_code(ch: i32) -> String {
    char::from_u32(ch as u32).map_or(String::new(), |c| c.to_string())
}

/// JS Number() over a digit string (used for quantifier and backreference
/// comparisons; matches parseInt semantics for plain digit runs)
fn js_number(digits: &str) -> f64 {
    digits.parse::<f64>().unwrap_or(f64::NAN)
}

impl<'a> RegexCheck<'a> {
    /// charCodeChecked: code point at byte offset `pos`, -1 at/after the limit
    fn ch(&self, pos: usize) -> i32 {
        if pos >= self.end {
            return -1;
        }
        self.s[pos..].chars().next().map_or(-1, |c| c as i32)
    }

    /// the onError closure of checkGrammarRegularExpressionLiteral: at most
    /// one diagnostic per start position, compared against the previous one
    fn error_at(&mut self, msg: &'static DiagnosticMessage, pos: usize, len: usize, args: &[String]) {
        let start = self.base + pos as u32;
        if self.last_error_start == Some(start) {
            return;
        }
        self.last_error_start = Some(start);
        self.diags.push(Diagnostic {
            file: Some(self.file),
            start,
            length: len as u32,
            message: MessageChain::new(msg, args),
        related: Vec::new(),
        });
    }

    /// error2 with default errPos=pos, length 0
    fn error_here(&mut self, msg: &'static DiagnosticMessage, args: &[String]) {
        self.error_at(msg, self.pos, 0, args);
    }

    fn check_flag_availability(&mut self, flag: u32, size: usize) {
        if let Some((min, name)) = flag_minimum_target(flag) {
            if self.language_version < min {
                self.error_at(
                    &gen::This_regular_expression_flag_is_only_available_when_targeting_0_or_later,
                    self.pos,
                    size,
                    &[name.to_string()],
                );
            }
        }
    }

    fn scan_expected_char(&mut self, ch: i32) {
        if self.ch(self.pos) == ch {
            self.pos += 1;
        } else {
            self.error_at(&gen::_0_expected, self.pos, 0, &[from_char_code(ch)]);
        }
    }

    fn scan_digits(&mut self) {
        let start = self.pos;
        while matches!(self.ch(self.pos), 0x30..=0x39) {
            self.pos += 1;
        }
        self.token_value = self.s[start..self.pos].to_string();
    }

    fn scan_word_characters(&mut self) -> String {
        let mut value = String::new();
        loop {
            let ch = self.ch(self.pos);
            if ch == -1 || !is_word_character(ch) {
                break;
            }
            value.push(ch as u8 as char);
            self.pos += 1;
        }
        value
    }

    fn scan_source_character(&mut self) -> String {
        // non-unicode mode consumes one UTF-16 unit in tsc; consuming the full
        // code point only diverges for astral characters in that mode
        let ch = self.ch(self.pos);
        if ch == -1 {
            return String::new();
        }
        let size = char_size(ch);
        self.pos += size;
        self.s[self.pos - size..self.pos].to_string()
    }

    fn scan_disjunction(&mut self, is_in_group: bool) {
        loop {
            self.scope_stack.push(std::mem::take(&mut self.top_scope));
            self.scan_alternative(is_in_group);
            self.top_scope = self.scope_stack.pop().unwrap_or_default();
            if self.ch(self.pos) != 0x7C {
                return; // not '|'
            }
            self.pos += 1;
        }
    }

    fn scan_alternative(&mut self, is_in_group: bool) {
        let mut is_previous_term_quantifiable = false;
        loop {
            let start = self.pos;
            let ch = self.ch(self.pos);
            // shared tail of the `{`/`*`/`+`/`?` quantifier cases
            macro_rules! quantifier_tail {
                () => {{
                    self.pos += 1;
                    if self.ch(self.pos) == 0x3F {
                        self.pos += 1; // non-greedy '?'
                    }
                    if !is_previous_term_quantifiable {
                        self.error_at(
                            &gen::There_is_nothing_available_for_repetition,
                            start,
                            self.pos - start,
                            &[],
                        );
                    }
                    is_previous_term_quantifiable = false;
                }};
            }
            match ch {
                -1 => return,
                0x5E | 0x24 => {
                    // ^ $
                    self.pos += 1;
                    is_previous_term_quantifiable = false;
                }
                0x5C => {
                    // backslash
                    self.pos += 1;
                    match self.ch(self.pos) {
                        0x62 | 0x42 => {
                            // \b \B
                            self.pos += 1;
                            is_previous_term_quantifiable = false;
                        }
                        _ => {
                            self.scan_atom_escape();
                            is_previous_term_quantifiable = true;
                        }
                    }
                }
                0x28 => {
                    // (
                    self.pos += 1;
                    if self.ch(self.pos) == 0x3F {
                        self.pos += 1;
                        match self.ch(self.pos) {
                            0x3D | 0x21 => {
                                // (?= (?!
                                self.pos += 1;
                                is_previous_term_quantifiable = !self.any_unicode_mode;
                            }
                            0x3C => {
                                // (?<
                                let group_name_start = self.pos;
                                self.pos += 1;
                                match self.ch(self.pos) {
                                    0x3D | 0x21 => {
                                        self.pos += 1;
                                        is_previous_term_quantifiable = false;
                                    }
                                    _ => {
                                        self.scan_group_name(false);
                                        self.scan_expected_char(0x3E); // '>'
                                        if self.language_version < 5 {
                                            self.error_at(
                                                &gen::Named_capturing_groups_are_only_available_when_targeting_ES2018_or_later,
                                                group_name_start,
                                                self.pos - group_name_start,
                                                &[],
                                            );
                                        }
                                        self.num_capturing_groups += 1;
                                        is_previous_term_quantifiable = true;
                                    }
                                }
                            }
                            _ => {
                                let start3 = self.pos;
                                let set_flags = self.scan_pattern_modifiers(0);
                                if self.ch(self.pos) == 0x2D {
                                    self.pos += 1;
                                    self.scan_pattern_modifiers(set_flags);
                                    if self.pos == start3 + 1 {
                                        self.error_at(
                                            &gen::Subpattern_flags_must_be_present_when_there_is_a_minus_sign,
                                            start3,
                                            self.pos - start3,
                                            &[],
                                        );
                                    }
                                }
                                self.scan_expected_char(0x3A); // ':'
                                is_previous_term_quantifiable = true;
                            }
                        }
                    } else {
                        self.num_capturing_groups += 1;
                        is_previous_term_quantifiable = true;
                    }
                    self.scan_disjunction(true);
                    self.scan_expected_char(0x29); // ')'
                }
                0x7B => {
                    // '{' — quantifier or literal brace
                    self.pos += 1;
                    let digits_start = self.pos;
                    self.scan_digits();
                    let min = std::mem::take(&mut self.token_value);
                    if !self.any_unicode_mode && min.is_empty() {
                        is_previous_term_quantifiable = true;
                        continue;
                    }
                    if self.ch(self.pos) == 0x2C {
                        self.pos += 1;
                        self.scan_digits();
                        let max = std::mem::take(&mut self.token_value);
                        if min.is_empty() {
                            if !max.is_empty() || self.ch(self.pos) == 0x7D {
                                self.error_at(&gen::Incomplete_quantifier_Digit_expected, digits_start, 0, &[]);
                            } else {
                                self.error_at(
                                    &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                                    start,
                                    1,
                                    &["{".to_string()],
                                );
                                is_previous_term_quantifiable = true;
                                continue;
                            }
                        } else if !max.is_empty()
                            && js_number(&min) > js_number(&max)
                            && (self.any_unicode_mode || self.ch(self.pos) == 0x7D)
                        {
                            self.error_at(
                                &gen::Numbers_out_of_order_in_quantifier,
                                digits_start,
                                self.pos - digits_start,
                                &[],
                            );
                        }
                    } else if min.is_empty() {
                        if self.any_unicode_mode {
                            self.error_at(
                                &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                                start,
                                1,
                                &["{".to_string()],
                            );
                        }
                        is_previous_term_quantifiable = true;
                        continue;
                    }
                    if self.ch(self.pos) != 0x7D {
                        if self.any_unicode_mode {
                            self.error_at(&gen::_0_expected, self.pos, 0, &["}".to_string()]);
                            self.pos -= 1;
                        } else {
                            is_previous_term_quantifiable = true;
                            continue;
                        }
                    }
                    quantifier_tail!();
                }
                0x2A | 0x2B | 0x3F => quantifier_tail!(), // * + ?
                0x2E => {
                    // .
                    self.pos += 1;
                    is_previous_term_quantifiable = true;
                }
                0x5B => {
                    // [
                    self.pos += 1;
                    if self.unicode_sets_mode {
                        self.scan_class_set_expression();
                    } else {
                        self.scan_class_ranges();
                    }
                    self.scan_expected_char(0x5D); // ']'
                    is_previous_term_quantifiable = true;
                }
                0x29 if is_in_group => return, // ')'
                0x29 | 0x5D | 0x7D => {
                    // stray ) ] }
                    if self.any_unicode_mode || ch == 0x29 {
                        self.error_at(
                            &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                            self.pos,
                            1,
                            &[from_char_code(ch)],
                        );
                    }
                    self.pos += 1;
                    is_previous_term_quantifiable = true;
                }
                0x2F | 0x7C => return, // / |
                _ => {
                    self.scan_source_character();
                    is_previous_term_quantifiable = true;
                }
            }
        }
    }

    fn scan_pattern_modifiers(&mut self, mut curr_flags: u32) -> u32 {
        loop {
            let ch = self.ch(self.pos);
            if ch == -1 || !is_ident_part_cp(ch) {
                break;
            }
            let size = char_size(ch);
            match char_to_flag(ch) {
                None => self.error_at(&gen::Unknown_regular_expression_flag, self.pos, size, &[]),
                Some(flag) => {
                    if curr_flags & flag != 0 {
                        self.error_at(&gen::Duplicate_regular_expression_flag, self.pos, size, &[]);
                    } else if flag & FLAG_MODIFIERS == 0 {
                        self.error_at(
                            &gen::This_regular_expression_flag_cannot_be_toggled_within_a_subpattern,
                            self.pos,
                            size,
                            &[],
                        );
                    } else {
                        curr_flags |= flag;
                        self.check_flag_availability(flag, size);
                    }
                }
            }
            self.pos += size;
        }
        curr_flags
    }

    fn scan_atom_escape(&mut self) {
        // pos is just past the backslash
        match self.ch(self.pos) {
            0x6B => {
                // k
                self.pos += 1;
                if self.ch(self.pos) == 0x3C {
                    self.pos += 1;
                    self.scan_group_name(true);
                    self.scan_expected_char(0x3E);
                } else if self.any_unicode_mode || self.named_capture_groups {
                    self.error_at(
                        &gen::k_must_be_followed_by_a_capturing_group_name_enclosed_in_angle_brackets,
                        self.pos - 2,
                        2,
                        &[],
                    );
                }
            }
            0x71 if self.unicode_sets_mode => {
                // q outside a class
                self.pos += 1;
                self.error_at(&gen::q_is_only_available_inside_character_class, self.pos - 2, 2, &[]);
            }
            _ => {
                if !self.scan_character_class_escape() && !self.scan_decimal_escape() {
                    self.scan_character_escape(true);
                }
            }
        }
    }

    fn scan_decimal_escape(&mut self) -> bool {
        let ch = self.ch(self.pos);
        if (0x31..=0x39).contains(&ch) {
            let start = self.pos;
            self.scan_digits();
            let value = js_number(&self.token_value);
            self.decimal_escapes.push((start, self.pos, value));
            return true;
        }
        false
    }

    fn scan_character_escape(&mut self, atom_escape: bool) -> String {
        // pos is just past the backslash
        let ch = self.ch(self.pos);
        match ch {
            -1 => {
                self.error_at(&gen::Undetermined_character_escape, self.pos - 1, 1, &[]);
                "\\".to_string()
            }
            0x63 => {
                // c
                self.pos += 1;
                let ch2 = self.ch(self.pos);
                if (0x41..=0x5A).contains(&ch2) || (0x61..=0x7A).contains(&ch2) {
                    self.pos += 1;
                    return from_char_code(ch2 & 31);
                }
                if self.any_unicode_mode {
                    self.error_at(&gen::c_must_be_followed_by_an_ASCII_letter, self.pos - 2, 2, &[]);
                } else if atom_escape {
                    self.pos -= 1;
                    return "\\".to_string();
                }
                from_char_code(ch2)
            }
            // ^ $ / \ . * + ? ( ) [ ] { } |
            0x5E | 0x24 | 0x2F | 0x5C | 0x2E | 0x2A | 0x2B | 0x3F | 0x28 | 0x29 | 0x5B | 0x5D
            | 0x7B | 0x7D | 0x7C => {
                self.pos += 1;
                from_char_code(ch)
            }
            _ => {
                self.pos -= 1; // back to the backslash
                let mut flags = ESC_REGULAR_EXPRESSION | ESC_ANNEX_B;
                if self.any_unicode_mode {
                    flags |= ESC_ANY_UNICODE_MODE;
                }
                if atom_escape {
                    flags |= ESC_ATOM_ESCAPE;
                }
                self.scan_escape_sequence(flags)
            }
        }
    }

    /// scanEscapeSequence restricted to the regex flag combinations (string
    /// literals have their own copy in scanner.rs; ReportInvalidEscapeErrors
    /// is always on for RegularExpression contexts)
    fn scan_escape_sequence(&mut self, flags: u32) -> String {
        let start2 = self.pos;
        self.pos += 1; // backslash
        if self.pos >= self.end {
            self.error_here(&gen::Unexpected_end_of_text, &[]);
            return String::new();
        }
        let ch = self.ch(self.pos);
        self.pos += char_size(ch);
        match ch {
            0x30..=0x37 => {
                if ch == 0x30 && !matches!(self.ch(self.pos), 0x30..=0x39) {
                    return "\0".to_string();
                }
                // octal ladder: \0-\3 take up to two further octal digits, \4-\7 one
                if (0x30..=0x33).contains(&ch) && matches!(self.ch(self.pos), 0x30..=0x37) {
                    self.pos += 1;
                }
                if matches!(self.ch(self.pos), 0x30..=0x37) {
                    self.pos += 1;
                }
                let code = u32::from_str_radix(&self.s[start2 + 1..self.pos], 8).unwrap_or(0);
                if flags & ESC_REGULAR_EXPRESSION != 0 && flags & ESC_ATOM_ESCAPE == 0 && ch != 0x30 {
                    self.error_at(
                        &gen::Octal_escape_sequences_and_backreferences_are_not_allowed_in_a_character_class_If_this_was_intended_as_an_escape_sequence_use_the_syntax_0_instead,
                        start2,
                        self.pos - start2,
                        &[format!("\\x{:02x}", code)],
                    );
                } else {
                    self.error_at(
                        &gen::Octal_escape_sequences_are_not_allowed_Use_the_syntax_0,
                        start2,
                        self.pos - start2,
                        &[format!("\\x{:02x}", code)],
                    );
                }
                from_char_code(code as i32)
            }
            0x38 | 0x39 => {
                if flags & ESC_REGULAR_EXPRESSION != 0 && flags & ESC_ATOM_ESCAPE == 0 {
                    self.error_at(
                        &gen::Decimal_escape_sequences_and_backreferences_are_not_allowed_in_a_character_class,
                        start2,
                        self.pos - start2,
                        &[],
                    );
                } else {
                    let lit = self.s[start2..self.pos].to_string();
                    self.error_at(&gen::Escape_sequence_0_is_not_allowed, start2, self.pos - start2, &[lit]);
                }
                from_char_code(ch)
            }
            0x62 => "\u{8}".to_string(),  // b
            0x74 => "\t".to_string(),     // t
            0x6E => "\n".to_string(),     // n
            0x76 => "\u{B}".to_string(),  // v
            0x66 => "\u{C}".to_string(),  // f
            0x72 => "\r".to_string(),     // r
            0x27 => "'".to_string(),
            0x22 => "\"".to_string(),
            0x75 => {
                // u
                if self.ch(self.pos) == 0x7B {
                    // \u{...}: scanExtendedUnicodeEscape
                    self.pos -= 2;
                    let result = self.scan_extended_unicode_escape();
                    if flags & ESC_ALLOW_EXTENDED_UNICODE_ESCAPE == 0 {
                        self.error_at(
                            &gen::Unicode_escape_sequences_are_only_available_when_the_Unicode_u_flag_or_the_Unicode_Sets_v_flag_is_set,
                            start2,
                            self.pos - start2,
                            &[],
                        );
                    }
                    return result;
                }
                while self.pos < start2 + 6 {
                    if !(self.pos < self.end && is_hex_digit(self.ch(self.pos))) {
                        self.error_here(&gen::Hexadecimal_digit_expected, &[]);
                        return self.s[start2..self.pos].to_string();
                    }
                    self.pos += 1;
                }
                let escaped_value =
                    u32::from_str_radix(&self.s[start2 + 2..self.pos], 16).unwrap_or(0);
                // AnyUnicodeMode surrogate-pair pairing affects only the
                // returned string (range comparisons); high surrogates are not
                // valid chars in Rust, so the unpaired value maps to ""
                if flags & ESC_ANY_UNICODE_MODE != 0
                    && (0xD800..=0xDBFF).contains(&escaped_value)
                    && self.pos + 6 < self.end
                    && self.s.as_bytes().get(self.pos) == Some(&b'\\')
                    && self.s.as_bytes().get(self.pos + 1) == Some(&b'u')
                    && self.s.as_bytes().get(self.pos + 2) != Some(&b'{')
                {
                    let next_start = self.pos;
                    let mut next_pos = self.pos + 2;
                    while next_pos < next_start + 6 {
                        if !is_hex_digit(self.ch(next_pos)) {
                            return from_char_code(escaped_value as i32);
                        }
                        next_pos += 1;
                    }
                    let next_value =
                        u32::from_str_radix(&self.s[next_start + 2..next_pos], 16).unwrap_or(0);
                    if (0xDC00..=0xDFFF).contains(&next_value) {
                        self.pos = next_pos;
                        let combined =
                            0x10000 + ((escaped_value - 0xD800) << 10) + (next_value - 0xDC00);
                        return from_char_code(combined as i32);
                    }
                }
                from_char_code(escaped_value as i32)
            }
            0x78 => {
                // x
                while self.pos < start2 + 4 {
                    if !(self.pos < self.end && is_hex_digit(self.ch(self.pos))) {
                        self.error_here(&gen::Hexadecimal_digit_expected, &[]);
                        return self.s[start2..self.pos].to_string();
                    }
                    self.pos += 1;
                }
                let v = u32::from_str_radix(&self.s[start2 + 2..self.pos], 16).unwrap_or(0);
                from_char_code(v as i32)
            }
            0x0D => {
                // CR (+LF): line continuation — unreachable in regex bodies
                if self.pos < self.end && self.ch(self.pos) == 0x0A {
                    self.pos += 1;
                }
                String::new()
            }
            0x0A | 0x2028 | 0x2029 => String::new(),
            _ => {
                if flags & ESC_ANY_UNICODE_MODE != 0
                    || flags & ESC_REGULAR_EXPRESSION != 0
                        && flags & ESC_ANNEX_B == 0
                        && is_ident_part_cp(ch)
                {
                    self.error_at(
                        &gen::This_character_cannot_be_escaped_in_a_regular_expression,
                        start2,
                        2,
                        &[],
                    );
                }
                from_char_code(ch)
            }
        }
    }

    fn scan_extended_unicode_escape(&mut self) -> String {
        // pos is at the backslash of `\u{`
        self.pos += 3;
        let escaped_start = self.pos;
        let mut any = false;
        while is_hex_digit(self.ch(self.pos)) {
            self.pos += 1;
            any = true;
        }
        let escaped_value = if any {
            u64::from_str_radix(&self.s[escaped_start..self.pos], 16).unwrap_or(u64::MAX)
        } else {
            self.error_here(&gen::Hexadecimal_digit_expected, &[]);
            0
        };
        let mut invalid = !any;
        if any && escaped_value > 0x10FFFF {
            self.error_at(
                &gen::An_extended_Unicode_escape_value_must_be_between_0x0_and_0x10FFFF_inclusive,
                escaped_start,
                self.pos - escaped_start,
                &[],
            );
            invalid = true;
        }
        if self.pos >= self.end {
            self.error_here(&gen::Unexpected_end_of_text, &[]);
            invalid = true;
        } else if self.ch(self.pos) == 0x7D {
            self.pos += 1;
        } else {
            self.error_here(&gen::Unterminated_Unicode_escape_sequence, &[]);
            invalid = true;
        }
        if invalid {
            return String::new();
        }
        from_char_code(escaped_value as i32)
    }

    fn scan_group_name(&mut self, is_reference: bool) {
        // pos is just past '<'; tsc scanIdentifier (no unicode-escape names)
        let token_start = self.pos;
        let ch = self.ch(self.pos);
        if ch != -1 && char::from_u32(ch as u32).map_or(false, is_ident_start) {
            self.pos += char_size(ch);
            loop {
                let c = self.ch(self.pos);
                if c == -1 || !is_ident_part_cp(c) {
                    break;
                }
                self.pos += char_size(c);
            }
        }
        if self.pos == token_start {
            self.error_here(&gen::Expected_a_capturing_group_name, &[]);
            return;
        }
        let name = self.s[token_start..self.pos].to_string();
        if is_reference {
            self.group_name_refs.push((token_start, self.pos, name));
        } else if self.top_scope.contains(&name) || self.scope_stack.iter().any(|s| s.contains(&name)) {
            self.error_at(
                &gen::Named_capturing_groups_with_the_same_name_must_be_mutually_exclusive_to_each_other,
                token_start,
                self.pos - token_start,
                &[],
            );
        } else {
            self.top_scope.push(name.clone());
            self.group_specifiers.push(name);
        }
    }

    fn is_class_content_exit(&self, ch: i32) -> bool {
        ch == 0x5D || ch == -1 || self.pos >= self.end
    }

    fn scan_class_ranges(&mut self) {
        // pos is just past '['
        if self.ch(self.pos) == 0x5E {
            self.pos += 1; // '^'
        }
        loop {
            let ch = self.ch(self.pos);
            if self.is_class_content_exit(ch) {
                return;
            }
            let min_start = self.pos;
            let min_character = self.scan_class_atom();
            if self.ch(self.pos) == 0x2D {
                self.pos += 1;
                let ch2 = self.ch(self.pos);
                if self.is_class_content_exit(ch2) {
                    return;
                }
                if min_character.is_empty() && self.any_unicode_mode {
                    self.error_at(
                        &gen::A_character_class_range_must_not_be_bounded_by_another_character_class,
                        min_start,
                        self.pos - 1 - min_start,
                        &[],
                    );
                }
                let max_start = self.pos;
                let max_character = self.scan_class_atom();
                if max_character.is_empty() && self.any_unicode_mode {
                    self.error_at(
                        &gen::A_character_class_range_must_not_be_bounded_by_another_character_class,
                        max_start,
                        self.pos - max_start,
                        &[],
                    );
                    continue;
                }
                if min_character.is_empty() {
                    continue;
                }
                if let (Some(min_cp), Some(max_cp)) = (single_code_point(&min_character), single_code_point(&max_character)) {
                    if min_cp > max_cp {
                        self.error_at(
                            &gen::Range_out_of_order_in_character_class,
                            min_start,
                            self.pos - min_start,
                            &[],
                        );
                    }
                }
            }
        }
    }

    fn scan_class_set_expression(&mut self) {
        // pos is just past '['
        let mut is_character_complement = false;
        if self.ch(self.pos) == 0x5E {
            self.pos += 1;
            is_character_complement = true;
        }
        let mut expression_may_contain_strings = false;
        let mut ch = self.ch(self.pos);
        if self.is_class_content_exit(ch) {
            return;
        }
        let mut start = self.pos;
        let mut operand;
        if self.two_at(self.pos) == Some("--") || self.two_at(self.pos) == Some("&&") {
            self.error_here(&gen::Expected_a_class_set_operand, &[]);
            self.may_contain_strings = false;
            operand = String::new();
        } else {
            operand = self.scan_class_set_operand();
        }
        let c0 = self.ch(self.pos);
        if c0 == 0x2D && self.ch(self.pos + 1) == 0x2D {
            if is_character_complement && self.may_contain_strings {
                self.error_at(
                    &gen::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                    start,
                    self.pos - start,
                    &[],
                );
            }
            expression_may_contain_strings = self.may_contain_strings;
            self.scan_class_set_sub_expression(SetExprType::Subtraction);
            self.may_contain_strings = !is_character_complement && expression_may_contain_strings;
            return;
        } else if c0 == 0x26 {
            if self.ch(self.pos + 1) == 0x26 {
                self.scan_class_set_sub_expression(SetExprType::Intersection);
                if is_character_complement && self.may_contain_strings {
                    self.error_at(
                        &gen::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                        start,
                        self.pos - start,
                        &[],
                    );
                }
                expression_may_contain_strings = self.may_contain_strings;
                self.may_contain_strings = !is_character_complement && expression_may_contain_strings;
                return;
            }
            // single '&': tsc reports the STALE first character of the
            // class here (String.fromCharCode(ch) with ch from above)
            self.error_at(
                &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                self.pos,
                1,
                &[from_char_code(ch)],
            );
        } else if c0 != 0x2D {
            // single '-' breaks out of the switch without the default logic
            if is_character_complement && self.may_contain_strings {
                self.error_at(
                    &gen::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                    start,
                    self.pos - start,
                    &[],
                );
            }
            expression_may_contain_strings = self.may_contain_strings;
        }
        loop {
            ch = self.ch(self.pos);
            if ch == -1 {
                break;
            }
            match ch {
                0x2D => {
                    // '-'
                    self.pos += 1;
                    ch = self.ch(self.pos);
                    if self.is_class_content_exit(ch) {
                        self.may_contain_strings = !is_character_complement && expression_may_contain_strings;
                        return;
                    }
                    if ch == 0x2D {
                        self.pos += 1;
                        self.error_at(
                            &gen::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                            self.pos - 2,
                            2,
                            &[],
                        );
                        start = self.pos - 2;
                        operand = self.s[start..self.pos].to_string();
                        continue;
                    } else {
                        if operand.is_empty() {
                            self.error_at(
                                &gen::A_character_class_range_must_not_be_bounded_by_another_character_class,
                                start,
                                self.pos - 1 - start,
                                &[],
                            );
                        }
                        let second_start = self.pos;
                        let second_operand = self.scan_class_set_operand();
                        if is_character_complement && self.may_contain_strings {
                            self.error_at(
                                &gen::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                                second_start,
                                self.pos - second_start,
                                &[],
                            );
                        }
                        expression_may_contain_strings =
                            expression_may_contain_strings || self.may_contain_strings;
                        if second_operand.is_empty() {
                            self.error_at(
                                &gen::A_character_class_range_must_not_be_bounded_by_another_character_class,
                                second_start,
                                self.pos - second_start,
                                &[],
                            );
                        } else if !operand.is_empty() {
                            if let (Some(min_cp), Some(max_cp)) =
                                (single_code_point(&operand), single_code_point(&second_operand))
                            {
                                if min_cp > max_cp {
                                    self.error_at(
                                        &gen::Range_out_of_order_in_character_class,
                                        start,
                                        self.pos - start,
                                        &[],
                                    );
                                }
                            }
                        }
                        // !secondOperand and !operand both `break` the switch in tsc
                    }
                }
                0x26 => {
                    // '&'
                    start = self.pos;
                    self.pos += 1;
                    if self.ch(self.pos) == 0x26 {
                        self.pos += 1;
                        self.error_at(
                            &gen::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                            self.pos - 2,
                            2,
                            &[],
                        );
                        if self.ch(self.pos) == 0x26 {
                            self.error_at(
                                &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                                self.pos,
                                1,
                                &[from_char_code(ch)],
                            );
                            self.pos += 1;
                        }
                    } else {
                        self.error_at(
                            &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                            self.pos - 1,
                            1,
                            &[from_char_code(ch)],
                        );
                    }
                    operand = self.s[start..self.pos].to_string();
                    continue;
                }
                _ => {}
            }
            if self.is_class_content_exit(self.ch(self.pos)) {
                break;
            }
            start = self.pos;
            if self.two_at(self.pos) == Some("--") || self.two_at(self.pos) == Some("&&") {
                self.error_at(
                    &gen::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                    self.pos,
                    2,
                    &[],
                );
                self.pos += 2;
                operand = self.s[start..self.pos].to_string();
            } else {
                operand = self.scan_class_set_operand();
            }
        }
        self.may_contain_strings = !is_character_complement && expression_may_contain_strings;
    }

    fn scan_class_set_sub_expression(&mut self, expression_type: SetExprType) {
        let mut expression_may_contain_strings = self.may_contain_strings;
        loop {
            let mut ch = self.ch(self.pos);
            if self.is_class_content_exit(ch) {
                break;
            }
            match ch {
                0x2D => {
                    self.pos += 1;
                    if self.ch(self.pos) == 0x2D {
                        self.pos += 1;
                        if expression_type != SetExprType::Subtraction {
                            self.error_at(
                                &gen::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                                self.pos - 2,
                                2,
                                &[],
                            );
                        }
                    } else {
                        self.error_at(
                            &gen::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                            self.pos - 1,
                            1,
                            &[],
                        );
                    }
                }
                0x26 => {
                    self.pos += 1;
                    if self.ch(self.pos) == 0x26 {
                        self.pos += 1;
                        if expression_type != SetExprType::Intersection {
                            self.error_at(
                                &gen::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                                self.pos - 2,
                                2,
                                &[],
                            );
                        }
                        if self.ch(self.pos) == 0x26 {
                            self.error_at(
                                &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                                self.pos,
                                1,
                                &[from_char_code(ch)],
                            );
                            self.pos += 1;
                        }
                    } else {
                        self.error_at(
                            &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                            self.pos - 1,
                            1,
                            &[from_char_code(ch)],
                        );
                    }
                }
                _ => match expression_type {
                    SetExprType::Subtraction => {
                        self.error_at(&gen::_0_expected, self.pos, 0, &["--".to_string()])
                    }
                    SetExprType::Intersection => {
                        self.error_at(&gen::_0_expected, self.pos, 0, &["&&".to_string()])
                    }
                },
            }
            ch = self.ch(self.pos);
            if self.is_class_content_exit(ch) {
                self.error_here(&gen::Expected_a_class_set_operand, &[]);
                break;
            }
            self.scan_class_set_operand();
            // expressionMayContainStrings &&= mayContainStrings
            expression_may_contain_strings =
                expression_may_contain_strings && self.may_contain_strings;
        }
        self.may_contain_strings = expression_may_contain_strings;
    }

    fn scan_class_set_operand(&mut self) -> String {
        self.may_contain_strings = false;
        match self.ch(self.pos) {
            -1 => String::new(),
            0x5B => {
                // nested class
                self.pos += 1;
                self.scan_class_set_expression();
                self.scan_expected_char(0x5D);
                String::new()
            }
            0x5C => {
                self.pos += 1;
                if self.scan_character_class_escape() {
                    return String::new();
                }
                if self.ch(self.pos) == 0x71 {
                    // q
                    self.pos += 1;
                    if self.ch(self.pos) == 0x7B {
                        self.pos += 1;
                        self.scan_class_string_disjunction_contents();
                        self.scan_expected_char(0x7D);
                        return String::new();
                    }
                    self.error_at(
                        &gen::q_must_be_followed_by_string_alternatives_enclosed_in_braces,
                        self.pos - 2,
                        2,
                        &[],
                    );
                    return "q".to_string();
                }
                self.pos -= 1;
                self.scan_class_set_character()
            }
            _ => self.scan_class_set_character(),
        }
    }

    fn scan_class_string_disjunction_contents(&mut self) {
        // pos is just past '{'
        let mut character_count = 0;
        loop {
            let ch = self.ch(self.pos);
            match ch {
                -1 => return,
                0x7D => {
                    // '}'
                    if character_count != 1 {
                        self.may_contain_strings = true;
                    }
                    return;
                }
                0x7C => {
                    // '|'
                    if character_count != 1 {
                        self.may_contain_strings = true;
                    }
                    self.pos += 1;
                    character_count = 0;
                }
                _ => {
                    self.scan_class_set_character();
                    character_count += 1;
                }
            }
        }
    }

    fn scan_class_set_character(&mut self) -> String {
        let ch = self.ch(self.pos);
        if ch == -1 {
            return String::new();
        }
        if ch == 0x5C {
            self.pos += 1;
            let ch2 = self.ch(self.pos);
            match ch2 {
                0x62 => {
                    self.pos += 1;
                    return "\u{8}".to_string();
                }
                // ClassSetReservedPunctuator escapes: & - ! # % , : ; < = > @ ` ~
                0x26 | 0x2D | 0x21 | 0x23 | 0x25 | 0x2C | 0x3A | 0x3B | 0x3C | 0x3D | 0x3E
                | 0x40 | 0x60 | 0x7E => {
                    self.pos += 1;
                    return from_char_code(ch2);
                }
                _ => return self.scan_character_escape(false),
            }
        } else if ch == self.ch(self.pos + 1) {
            // reserved double punctuators: && !! ## %% ** ++ ,, .. :: ;; << == >> ?? @@ `` ~~
            match ch {
                0x26 | 0x21 | 0x23 | 0x25 | 0x2A | 0x2B | 0x2C | 0x2E | 0x3A | 0x3B | 0x3C
                | 0x3D | 0x3E | 0x3F | 0x40 | 0x60 | 0x7E => {
                    self.error_at(
                        &gen::A_character_class_must_not_contain_a_reserved_double_punctuator_Did_you_mean_to_escape_it_with_backslash,
                        self.pos,
                        2,
                        &[],
                    );
                    self.pos += 2;
                    return self.s[self.pos - 2..self.pos].to_string();
                }
                _ => {}
            }
        }
        match ch {
            // / ( ) [ ] { } - |
            0x2F | 0x28 | 0x29 | 0x5B | 0x5D | 0x7B | 0x7D | 0x2D | 0x7C => {
                self.error_at(
                    &gen::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
                    self.pos,
                    1,
                    &[from_char_code(ch)],
                );
                self.pos += 1;
                from_char_code(ch)
            }
            _ => self.scan_source_character(),
        }
    }

    fn scan_class_atom(&mut self) -> String {
        if self.ch(self.pos) == 0x5C {
            self.pos += 1;
            let ch = self.ch(self.pos);
            match ch {
                0x62 => {
                    self.pos += 1;
                    "\u{8}".to_string()
                }
                0x2D => {
                    self.pos += 1;
                    from_char_code(ch)
                }
                _ => {
                    if self.scan_character_class_escape() {
                        return String::new();
                    }
                    self.scan_character_escape(false)
                }
            }
        } else {
            self.scan_source_character()
        }
    }

    fn scan_character_class_escape(&mut self) -> bool {
        // pos is just past the backslash
        let mut is_character_complement = false;
        let start = self.pos - 1;
        let ch = self.ch(self.pos);
        match ch {
            // d D s S w W
            0x64 | 0x44 | 0x73 | 0x53 | 0x77 | 0x57 => {
                self.pos += 1;
                true
            }
            0x50 | 0x70 => {
                if ch == 0x50 {
                    is_character_complement = true;
                }
                self.pos += 1;
                if self.ch(self.pos) == 0x7B {
                    self.pos += 1;
                    let nv_start = self.pos;
                    let name_or_value = self.scan_word_characters();
                    if self.ch(self.pos) == 0x3D {
                        let property_name = non_binary_property(&name_or_value);
                        if self.pos == nv_start {
                            self.error_here(&gen::Expected_a_Unicode_property_name, &[]);
                        } else if property_name.is_none() {
                            self.error_at(
                                &gen::Unknown_Unicode_property_name,
                                nv_start,
                                self.pos - nv_start,
                                &[],
                            );
                        }
                        self.pos += 1;
                        let value_start = self.pos;
                        let value = self.scan_word_characters();
                        if self.pos == value_start {
                            self.error_here(&gen::Expected_a_Unicode_property_value, &[]);
                        } else if let Some(name) = property_name {
                            if !non_binary_values(name).contains(&value.as_str()) {
                                self.error_at(
                                    &gen::Unknown_Unicode_property_value,
                                    value_start,
                                    self.pos - value_start,
                                    &[],
                                );
                            }
                        }
                    } else if self.pos == nv_start {
                        self.error_here(&gen::Expected_a_Unicode_property_name_or_value, &[]);
                    } else if BINARY_UNICODE_PROPERTIES_OF_STRINGS.contains(&name_or_value.as_str()) {
                        if !self.unicode_sets_mode {
                            self.error_at(
                                &gen::Any_Unicode_property_that_would_possibly_match_more_than_a_single_character_is_only_available_when_the_Unicode_Sets_v_flag_is_set,
                                nv_start,
                                self.pos - nv_start,
                                &[],
                            );
                        } else if is_character_complement {
                            self.error_at(
                                &gen::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                                nv_start,
                                self.pos - nv_start,
                                &[],
                            );
                        } else {
                            self.may_contain_strings = true;
                        }
                    } else if !GENERAL_CATEGORY_VALUES.contains(&name_or_value.as_str())
                        && !BINARY_UNICODE_PROPERTIES.contains(&name_or_value.as_str())
                    {
                        self.error_at(
                            &gen::Unknown_Unicode_property_name_or_value,
                            nv_start,
                            self.pos - nv_start,
                            &[],
                        );
                    }
                    self.scan_expected_char(0x7D);
                    if !self.any_unicode_mode {
                        self.error_at(
                            &gen::Unicode_property_value_expressions_are_only_available_when_the_Unicode_u_flag_or_the_Unicode_Sets_v_flag_is_set,
                            start,
                            self.pos - start,
                            &[],
                        );
                    }
                } else if self.any_unicode_mode {
                    self.error_at(
                        &gen::_0_must_be_followed_by_a_Unicode_property_value_expression_enclosed_in_braces,
                        self.pos - 2,
                        2,
                        &[from_char_code(ch)],
                    );
                } else {
                    self.pos -= 1;
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    fn two_at(&self, pos: usize) -> Option<&str> {
        if pos + 2 <= self.b.len() {
            self.s.get(pos..pos + 2)
        } else {
            None
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum SetExprType {
    Intersection,
    Subtraction,
}

fn is_word_character(ch: i32) -> bool {
    (0x41..=0x5A).contains(&ch) || (0x61..=0x7A).contains(&ch) || (0x30..=0x39).contains(&ch) || ch == 0x5F
}

fn is_hex_digit(ch: i32) -> bool {
    (0x30..=0x39).contains(&ch) || (0x41..=0x46).contains(&ch) || (0x61..=0x66).contains(&ch)
}

/// "exactly one code point" (tsc: s.length === charSize(codePointAt(s, 0)))
fn single_code_point(s: &str) -> Option<u32> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c as u32),
        _ => None,
    }
}

fn non_binary_property(name: &str) -> Option<&'static str> {
    match name {
        "General_Category" | "gc" => Some("General_Category"),
        "Script" | "sc" => Some("Script"),
        "Script_Extensions" | "scx" => Some("Script_Extensions"),
        _ => None,
    }
}

fn non_binary_values(name: &str) -> &'static [&'static str] {
    match name {
        "General_Category" => GENERAL_CATEGORY_VALUES,
        _ => SCRIPT_VALUES, // Script and Script_Extensions share the value set
    }
}

// Unicode property tables, extracted verbatim from tsc 6.0's scanner
// (nonBinaryUnicodeProperties / valuesOfNonBinaryUnicodeProperties /
// binaryUnicodeProperties / binaryUnicodePropertiesOfStrings).
static GENERAL_CATEGORY_VALUES: &[&str] = &[
    "C", "Other", "Cc", "Control", "cntrl", "Cf", "Format", "Cn", "Unassigned", "Co", "Private_Use", "Cs",
    "Surrogate", "L", "Letter", "LC", "Cased_Letter", "Ll", "Lowercase_Letter", "Lm", "Modifier_Letter",
    "Lo", "Other_Letter", "Lt", "Titlecase_Letter", "Lu", "Uppercase_Letter", "M", "Mark", "Combining_Mark",
    "Mc", "Spacing_Mark", "Me", "Enclosing_Mark", "Mn", "Nonspacing_Mark", "N", "Number", "Nd", "Decimal_Number",
    "digit", "Nl", "Letter_Number", "No", "Other_Number", "P", "Punctuation", "punct", "Pc", "Connector_Punctuation",
    "Pd", "Dash_Punctuation", "Pe", "Close_Punctuation", "Pf", "Final_Punctuation", "Pi", "Initial_Punctuation",
    "Po", "Other_Punctuation", "Ps", "Open_Punctuation", "S", "Symbol", "Sc", "Currency_Symbol", "Sk",
    "Modifier_Symbol", "Sm", "Math_Symbol", "So", "Other_Symbol", "Z", "Separator", "Zl", "Line_Separator",
    "Zp", "Paragraph_Separator", "Zs", "Space_Separator",
];
static SCRIPT_VALUES: &[&str] = &[
    "Adlm", "Adlam", "Aghb", "Caucasian_Albanian", "Ahom", "Arab", "Arabic", "Armi", "Imperial_Aramaic",
    "Armn", "Armenian", "Avst", "Avestan", "Bali", "Balinese", "Bamu", "Bamum", "Bass", "Bassa_Vah", "Batk",
    "Batak", "Beng", "Bengali", "Bhks", "Bhaiksuki", "Bopo", "Bopomofo", "Brah", "Brahmi", "Brai", "Braille",
    "Bugi", "Buginese", "Buhd", "Buhid", "Cakm", "Chakma", "Cans", "Canadian_Aboriginal", "Cari", "Carian",
    "Cham", "Cher", "Cherokee", "Chrs", "Chorasmian", "Copt", "Coptic", "Qaac", "Cpmn", "Cypro_Minoan",
    "Cprt", "Cypriot", "Cyrl", "Cyrillic", "Deva", "Devanagari", "Diak", "Dives_Akuru", "Dogr", "Dogra",
    "Dsrt", "Deseret", "Dupl", "Duployan", "Egyp", "Egyptian_Hieroglyphs", "Elba", "Elbasan", "Elym",
    "Elymaic", "Ethi", "Ethiopic", "Geor", "Georgian", "Glag", "Glagolitic", "Gong", "Gunjala_Gondi",
    "Gonm", "Masaram_Gondi", "Goth", "Gothic", "Gran", "Grantha", "Grek", "Greek", "Gujr", "Gujarati",
    "Guru", "Gurmukhi", "Hang", "Hangul", "Hani", "Han", "Hano", "Hanunoo", "Hatr", "Hatran", "Hebr",
    "Hebrew", "Hira", "Hiragana", "Hluw", "Anatolian_Hieroglyphs", "Hmng", "Pahawh_Hmong", "Hmnp",
    "Nyiakeng_Puachue_Hmong", "Hrkt", "Katakana_Or_Hiragana", "Hung", "Old_Hungarian", "Ital", "Old_Italic",
    "Java", "Javanese", "Kali", "Kayah_Li", "Kana", "Katakana", "Kawi", "Khar", "Kharoshthi", "Khmr",
    "Khmer", "Khoj", "Khojki", "Kits", "Khitan_Small_Script", "Knda", "Kannada", "Kthi", "Kaithi", "Lana",
    "Tai_Tham", "Laoo", "Lao", "Latn", "Latin", "Lepc", "Lepcha", "Limb", "Limbu", "Lina", "Linear_A",
    "Linb", "Linear_B", "Lisu", "Lyci", "Lycian", "Lydi", "Lydian", "Mahj", "Mahajani", "Maka", "Makasar",
    "Mand", "Mandaic", "Mani", "Manichaean", "Marc", "Marchen", "Medf", "Medefaidrin", "Mend", "Mende_Kikakui",
    "Merc", "Meroitic_Cursive", "Mero", "Meroitic_Hieroglyphs", "Mlym", "Malayalam", "Modi", "Mong",
    "Mongolian", "Mroo", "Mro", "Mtei", "Meetei_Mayek", "Mult", "Multani", "Mymr", "Myanmar", "Nagm",
    "Nag_Mundari", "Nand", "Nandinagari", "Narb", "Old_North_Arabian", "Nbat", "Nabataean", "Newa", "Nkoo",
    "Nko", "Nshu", "Nushu", "Ogam", "Ogham", "Olck", "Ol_Chiki", "Orkh", "Old_Turkic", "Orya", "Oriya",
    "Osge", "Osage", "Osma", "Osmanya", "Ougr", "Old_Uyghur", "Palm", "Palmyrene", "Pauc", "Pau_Cin_Hau",
    "Perm", "Old_Permic", "Phag", "Phags_Pa", "Phli", "Inscriptional_Pahlavi", "Phlp", "Psalter_Pahlavi",
    "Phnx", "Phoenician", "Plrd", "Miao", "Prti", "Inscriptional_Parthian", "Rjng", "Rejang", "Rohg",
    "Hanifi_Rohingya", "Runr", "Runic", "Samr", "Samaritan", "Sarb", "Old_South_Arabian", "Saur",
    "Saurashtra", "Sgnw", "SignWriting", "Shaw", "Shavian", "Shrd", "Sharada", "Sidd", "Siddham", "Sind",
    "Khudawadi", "Sinh", "Sinhala", "Sogd", "Sogdian", "Sogo", "Old_Sogdian", "Sora", "Sora_Sompeng",
    "Soyo", "Soyombo", "Sund", "Sundanese", "Sylo", "Syloti_Nagri", "Syrc", "Syriac", "Tagb", "Tagbanwa",
    "Takr", "Takri", "Tale", "Tai_Le", "Talu", "New_Tai_Lue", "Taml", "Tamil", "Tang", "Tangut", "Tavt",
    "Tai_Viet", "Telu", "Telugu", "Tfng", "Tifinagh", "Tglg", "Tagalog", "Thaa", "Thaana", "Thai", "Tibt",
    "Tibetan", "Tirh", "Tirhuta", "Tnsa", "Tangsa", "Toto", "Ugar", "Ugaritic", "Vaii", "Vai", "Vith",
    "Vithkuqi", "Wara", "Warang_Citi", "Wcho", "Wancho", "Xpeo", "Old_Persian", "Xsux", "Cuneiform",
    "Yezi", "Yezidi", "Yiii", "Yi", "Zanb", "Zanabazar_Square", "Zinh", "Inherited", "Qaai", "Zyyy",
    "Common", "Zzzz", "Unknown",
];
static BINARY_UNICODE_PROPERTIES: &[&str] = &[
    "ASCII", "ASCII_Hex_Digit", "AHex", "Alphabetic", "Alpha", "Any", "Assigned", "Bidi_Control", "Bidi_C",
    "Bidi_Mirrored", "Bidi_M", "Case_Ignorable", "CI", "Cased", "Changes_When_Casefolded", "CWCF",
    "Changes_When_Casemapped", "CWCM", "Changes_When_Lowercased", "CWL", "Changes_When_NFKC_Casefolded",
    "CWKCF", "Changes_When_Titlecased", "CWT", "Changes_When_Uppercased", "CWU", "Dash",
    "Default_Ignorable_Code_Point", "DI", "Deprecated", "Dep", "Diacritic", "Dia", "Emoji", "Emoji_Component",
    "EComp", "Emoji_Modifier", "EMod", "Emoji_Modifier_Base", "EBase", "Emoji_Presentation", "EPres",
    "Extended_Pictographic", "ExtPict", "Extender", "Ext", "Grapheme_Base", "Gr_Base", "Grapheme_Extend",
    "Gr_Ext", "Hex_Digit", "Hex", "IDS_Binary_Operator", "IDSB", "IDS_Trinary_Operator", "IDST",
    "ID_Continue", "IDC", "ID_Start", "IDS", "Ideographic", "Ideo", "Join_Control", "Join_C",
    "Logical_Order_Exception", "LOE", "Lowercase", "Lower", "Math", "Noncharacter_Code_Point", "NChar",
    "Pattern_Syntax", "Pat_Syn", "Pattern_White_Space", "Pat_WS", "Quotation_Mark", "QMark", "Radical",
    "Regional_Indicator", "RI", "Sentence_Terminal", "STerm", "Soft_Dotted", "SD", "Terminal_Punctuation",
    "Term", "Unified_Ideograph", "UIdeo", "Uppercase", "Upper", "Variation_Selector", "VS", "White_Space",
    "space", "XID_Continue", "XIDC", "XID_Start", "XIDS",
];
static BINARY_UNICODE_PROPERTIES_OF_STRINGS: &[&str] = &[
    "Basic_Emoji", "Emoji_Keycap_Sequence", "RGI_Emoji_Modifier_Sequence", "RGI_Emoji_Flag_Sequence",
    "RGI_Emoji_Tag_Sequence", "RGI_Emoji_ZWJ_Sequence", "RGI_Emoji",
];

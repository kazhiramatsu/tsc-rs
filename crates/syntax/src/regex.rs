//! TypeScript regular-expression grammar validation.
//!
//! This is deliberately separate from the parser's UTF-8 byte scanner.
//! The upstream worker indexes JavaScript strings in UTF-16 code units,
//! and every diagnostic offset below therefore uses a `Vec<u16>`.

use crate::chars::{is_identifier_part, is_identifier_start};
use crate::regex_unicode::{
    BINARY_UNICODE_PROPERTIES, BINARY_UNICODE_PROPERTIES_OF_STRINGS, GENERAL_CATEGORY_VALUES,
    NON_BINARY_UNICODE_PROPERTIES, SCRIPT_VALUES,
};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_types::ScriptTarget;

const EOF: i32 = -1;

const FLAG_HAS_INDICES: u16 = 1;
const FLAG_GLOBAL: u16 = 2;
const FLAG_IGNORE_CASE: u16 = 4;
const FLAG_MULTILINE: u16 = 8;
const FLAG_DOT_ALL: u16 = 16;
const FLAG_UNICODE: u16 = 32;
const FLAG_UNICODE_SETS: u16 = 64;
const FLAG_STICKY: u16 = 128;
const FLAG_ANY_UNICODE: u16 = FLAG_UNICODE | FLAG_UNICODE_SETS;
const FLAG_MODIFIERS: u16 = FLAG_IGNORE_CASE | FLAG_MULTILINE | FLAG_DOT_ALL;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegexDiagnostic {
    pub message: &'static DiagnosticMessage,
    pub start_utf16: u32,
    pub length_utf16: u32,
    pub args: Vec<String>,
}

#[derive(Clone, Debug)]
struct CaptureReference {
    pos: usize,
    end: usize,
    name: String,
}

#[derive(Clone, Copy, Debug)]
struct DecimalEscape {
    pos: usize,
    end: usize,
    value: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassExpressionType {
    Intersection,
    Subtraction,
}

/// tsc-port: reScanSlashToken @6.0.3
/// tsc-hash: 774d2e674de1c2c69b5150aaf6a6a8fdf0360d6f7af59a686894c2b1ea67398a
/// tsc-span: _tsc.js:9893-9996
///
/// Input is the complete literal token (`/.../flags`). Returned
/// positions are relative to its first slash and use UTF-16 units. The
/// reporting path includes `scanRegularExpressionWorker` and its nested
/// closure through `_tsc.js:10844`.
pub fn validate_regular_expression_literal(
    literal_text: &str,
    target: ScriptTarget,
) -> Vec<RegexDiagnostic> {
    let text: Vec<u16> = literal_text.encode_utf16().collect();
    if text.first().copied() != Some(b'/' as u16) {
        return Vec::new();
    }

    let mut pos = 1usize;
    let mut in_escape = false;
    let mut named_capture_groups = false;
    let mut in_character_class = false;
    while pos < text.len() {
        let ch = text[pos];
        if ch == b'\n' as u16 || ch == b'\r' as u16 || ch == 0x2028 || ch == 0x2029 {
            return Vec::new();
        }
        if in_escape {
            in_escape = false;
        } else if ch == b'/' as u16 && !in_character_class {
            break;
        } else if ch == b'[' as u16 {
            in_character_class = true;
        } else if ch == b'\\' as u16 {
            in_escape = true;
        } else if ch == b']' as u16 {
            in_character_class = false;
        } else if !in_character_class
            && ch == b'(' as u16
            && text.get(pos + 1).copied() == Some(b'?' as u16)
            && text.get(pos + 2).copied() == Some(b'<' as u16)
            && !matches!(
                text.get(pos + 3).copied(),
                Some(value) if value == b'=' as u16 || value == b'!' as u16
            )
        {
            named_capture_groups = true;
        }
        pos += 1;
    }
    if pos >= text.len() || text[pos] != b'/' as u16 {
        return Vec::new();
    }

    let body_end = pos;
    pos += 1;
    let mut diagnostics = Vec::new();
    let mut flags = 0u16;
    while let Some((code_point, size)) = code_point_at(&text, pos) {
        if !is_identifier_part_code_point(code_point, target) {
            break;
        }
        let start = pos;
        if let Some(flag) = regular_expression_flag(code_point) {
            if flags & flag != 0 {
                push_diagnostic(
                    &mut diagnostics,
                    &diagnostics::Duplicate_regular_expression_flag,
                    start,
                    size,
                    Vec::new(),
                );
            } else if (flags | flag) & FLAG_ANY_UNICODE == FLAG_ANY_UNICODE {
                push_diagnostic(
                    &mut diagnostics,
                    &diagnostics::The_Unicode_u_flag_and_the_Unicode_Sets_v_flag_cannot_be_set_simultaneously,
                    start,
                    size,
                    Vec::new(),
                );
            } else {
                flags |= flag;
                check_flag_availability(&mut diagnostics, target, flag, start, size);
            }
        } else {
            push_diagnostic(
                &mut diagnostics,
                &diagnostics::Unknown_regular_expression_flag,
                start,
                size,
                Vec::new(),
            );
        }
        pos += size;
    }

    let mut scanner = RegexScanner {
        text,
        pos: 1,
        end: body_end,
        target,
        diagnostics,
        unicode_sets_mode: flags & FLAG_UNICODE_SETS != 0,
        any_unicode_mode: flags & FLAG_ANY_UNICODE != 0,
        any_unicode_mode_or_non_annex_b: flags & FLAG_ANY_UNICODE != 0,
        named_capture_groups,
        may_contain_strings: false,
        number_of_capturing_groups: 0,
        group_specifiers: Vec::new(),
        group_name_references: Vec::new(),
        decimal_escapes: Vec::new(),
        named_capturing_groups_scope_stack: Vec::new(),
        top_named_capturing_groups_scope: None,
    };
    scanner.scan_disjunction(false);
    scanner.finish_references();
    scanner.diagnostics
}

struct RegexScanner {
    text: Vec<u16>,
    pos: usize,
    end: usize,
    target: ScriptTarget,
    diagnostics: Vec<RegexDiagnostic>,
    unicode_sets_mode: bool,
    any_unicode_mode: bool,
    any_unicode_mode_or_non_annex_b: bool,
    named_capture_groups: bool,
    may_contain_strings: bool,
    number_of_capturing_groups: u64,
    group_specifiers: Vec<String>,
    group_name_references: Vec<CaptureReference>,
    decimal_escapes: Vec<DecimalEscape>,
    named_capturing_groups_scope_stack: Vec<Option<Vec<String>>>,
    top_named_capturing_groups_scope: Option<Vec<String>>,
}

impl RegexScanner {
    fn error(
        &mut self,
        message: &'static DiagnosticMessage,
        start: usize,
        length: usize,
        args: Vec<String>,
    ) {
        push_diagnostic(&mut self.diagnostics, message, start, length, args);
    }

    fn error_here(&mut self, message: &'static DiagnosticMessage) {
        self.error(message, self.pos, 0, Vec::new());
    }

    fn char_code(&self, pos: usize) -> i32 {
        if pos < self.end {
            i32::from(self.text[pos])
        } else {
            EOF
        }
    }

    fn code_point(&self, pos: usize) -> Option<(u32, usize)> {
        if pos >= self.end {
            None
        } else {
            code_point_at(&self.text[..self.end], pos)
        }
    }

    fn slice(&self, start: usize, end: usize) -> Vec<u16> {
        self.text[start.min(self.end)..end.min(self.end)].to_vec()
    }

    fn slice_string(&self, start: usize, end: usize) -> String {
        String::from_utf16_lossy(&self.slice(start, end))
    }

    fn pair_is(&self, pos: usize, first: u8, second: u8) -> bool {
        self.char_code(pos) == i32::from(first) && self.char_code(pos + 1) == i32::from(second)
    }

    fn scan_disjunction(&mut self, is_in_group: bool) {
        loop {
            self.named_capturing_groups_scope_stack
                .push(self.top_named_capturing_groups_scope.take());
            self.scan_alternative(is_in_group);
            self.top_named_capturing_groups_scope = self
                .named_capturing_groups_scope_stack
                .pop()
                .expect("regular-expression scope stack is balanced");
            if self.char_code(self.pos) != i32::from(b'|') {
                return;
            }
            self.pos += 1;
        }
    }

    fn scan_alternative(&mut self, is_in_group: bool) {
        let mut previous_term_quantifiable = false;
        loop {
            let start = self.pos;
            let ch = self.char_code(self.pos);
            match ch {
                EOF => return,
                value if value == i32::from(b'^') || value == i32::from(b'$') => {
                    self.pos += 1;
                    previous_term_quantifiable = false;
                }
                value if value == i32::from(b'\\') => {
                    self.pos += 1;
                    match self.char_code(self.pos) {
                        value if value == i32::from(b'b') || value == i32::from(b'B') => {
                            self.pos += 1;
                            previous_term_quantifiable = false;
                        }
                        _ => {
                            self.scan_atom_escape();
                            previous_term_quantifiable = true;
                        }
                    }
                }
                value if value == i32::from(b'(') => {
                    self.pos += 1;
                    if self.char_code(self.pos) == i32::from(b'?') {
                        self.pos += 1;
                        match self.char_code(self.pos) {
                            value if value == i32::from(b'=') || value == i32::from(b'!') => {
                                self.pos += 1;
                                previous_term_quantifiable = !self.any_unicode_mode_or_non_annex_b;
                            }
                            value if value == i32::from(b'<') => {
                                let group_name_start = self.pos;
                                self.pos += 1;
                                match self.char_code(self.pos) {
                                    value
                                        if value == i32::from(b'=') || value == i32::from(b'!') =>
                                    {
                                        self.pos += 1;
                                        previous_term_quantifiable = false;
                                    }
                                    _ => {
                                        self.scan_group_name(false);
                                        self.scan_expected_char(b'>');
                                        if self.target < ScriptTarget::ES2018 {
                                            self.error(
                                                &diagnostics::Named_capturing_groups_are_only_available_when_targeting_ES2018_or_later,
                                                group_name_start,
                                                self.pos.saturating_sub(group_name_start),
                                                Vec::new(),
                                            );
                                        }
                                        self.number_of_capturing_groups += 1;
                                        previous_term_quantifiable = true;
                                    }
                                }
                            }
                            _ => {
                                let modifier_start = self.pos;
                                let set_flags = self.scan_pattern_modifiers(0);
                                if self.char_code(self.pos) == i32::from(b'-') {
                                    self.pos += 1;
                                    self.scan_pattern_modifiers(set_flags);
                                    if self.pos == modifier_start + 1 {
                                        self.error(
                                            &diagnostics::Subpattern_flags_must_be_present_when_there_is_a_minus_sign,
                                            modifier_start,
                                            self.pos - modifier_start,
                                            Vec::new(),
                                        );
                                    }
                                }
                                self.scan_expected_char(b':');
                                previous_term_quantifiable = true;
                            }
                        }
                    } else {
                        self.number_of_capturing_groups += 1;
                        previous_term_quantifiable = true;
                    }
                    self.scan_disjunction(true);
                    self.scan_expected_char(b')');
                }
                value if value == i32::from(b'{') => {
                    self.pos += 1;
                    let digits_start = self.pos;
                    let minimum = self.scan_digits();
                    if !self.any_unicode_mode_or_non_annex_b && minimum.is_empty() {
                        previous_term_quantifiable = true;
                        continue;
                    }
                    if self.char_code(self.pos) == i32::from(b',') {
                        self.pos += 1;
                        let maximum = self.scan_digits();
                        if minimum.is_empty() {
                            if !maximum.is_empty() || self.char_code(self.pos) == i32::from(b'}') {
                                self.error(
                                    &diagnostics::Incomplete_quantifier_Digit_expected,
                                    digits_start,
                                    0,
                                    Vec::new(),
                                );
                            } else {
                                self.unexpected_character(start, 1, ch);
                                previous_term_quantifiable = true;
                                continue;
                            }
                        } else if !maximum.is_empty()
                            && parse_decimal_number(&minimum) > parse_decimal_number(&maximum)
                            && (self.any_unicode_mode_or_non_annex_b
                                || self.char_code(self.pos) == i32::from(b'}'))
                        {
                            self.error(
                                &diagnostics::Numbers_out_of_order_in_quantifier,
                                digits_start,
                                self.pos - digits_start,
                                Vec::new(),
                            );
                        }
                    } else if minimum.is_empty() {
                        if self.any_unicode_mode_or_non_annex_b {
                            self.unexpected_character(start, 1, ch);
                        }
                        previous_term_quantifiable = true;
                        continue;
                    }
                    if self.char_code(self.pos) != i32::from(b'}') {
                        if self.any_unicode_mode_or_non_annex_b {
                            self.expected_character(self.pos, "}");
                            self.pos = self.pos.saturating_sub(1);
                        } else {
                            previous_term_quantifiable = true;
                            continue;
                        }
                    }
                    self.scan_quantifier_tail(start, &mut previous_term_quantifiable);
                }
                value
                    if value == i32::from(b'*')
                        || value == i32::from(b'+')
                        || value == i32::from(b'?') =>
                {
                    self.scan_quantifier_tail(start, &mut previous_term_quantifiable);
                }
                value if value == i32::from(b'.') => {
                    self.pos += 1;
                    previous_term_quantifiable = true;
                }
                value if value == i32::from(b'[') => {
                    self.pos += 1;
                    if self.unicode_sets_mode {
                        self.scan_class_set_expression();
                    } else {
                        self.scan_class_ranges();
                    }
                    self.scan_expected_char(b']');
                    previous_term_quantifiable = true;
                }
                value if value == i32::from(b')') => {
                    if is_in_group {
                        return;
                    }
                    self.unexpected_character(self.pos, 1, ch);
                    self.pos += 1;
                    previous_term_quantifiable = true;
                }
                value if value == i32::from(b']') || value == i32::from(b'}') => {
                    if self.any_unicode_mode_or_non_annex_b {
                        self.unexpected_character(self.pos, 1, ch);
                    }
                    self.pos += 1;
                    previous_term_quantifiable = true;
                }
                value if value == i32::from(b'/') || value == i32::from(b'|') => return,
                _ => {
                    self.scan_source_character();
                    previous_term_quantifiable = true;
                }
            }
        }
    }

    fn scan_quantifier_tail(&mut self, start: usize, previous_term_quantifiable: &mut bool) {
        self.pos += 1;
        if self.char_code(self.pos) == i32::from(b'?') {
            self.pos += 1;
        }
        if !*previous_term_quantifiable {
            self.error(
                &diagnostics::There_is_nothing_available_for_repetition,
                start,
                self.pos - start,
                Vec::new(),
            );
        }
        *previous_term_quantifiable = false;
    }

    fn scan_pattern_modifiers(&mut self, mut current_flags: u16) -> u16 {
        while let Some((code_point, size)) = self.code_point(self.pos) {
            if !is_identifier_part_code_point(code_point, self.target) {
                break;
            }
            let start = self.pos;
            if let Some(flag) = regular_expression_flag(code_point) {
                if current_flags & flag != 0 {
                    self.error(
                        &diagnostics::Duplicate_regular_expression_flag,
                        start,
                        size,
                        Vec::new(),
                    );
                } else if flag & FLAG_MODIFIERS == 0 {
                    self.error(
                        &diagnostics::This_regular_expression_flag_cannot_be_toggled_within_a_subpattern,
                        start,
                        size,
                        Vec::new(),
                    );
                } else {
                    current_flags |= flag;
                    check_flag_availability(&mut self.diagnostics, self.target, flag, start, size);
                }
            } else {
                self.error(
                    &diagnostics::Unknown_regular_expression_flag,
                    start,
                    size,
                    Vec::new(),
                );
            }
            self.pos += size;
        }
        current_flags
    }

    fn scan_atom_escape(&mut self) {
        debug_assert_eq!(self.char_code(self.pos.saturating_sub(1)), i32::from(b'\\'));
        match self.char_code(self.pos) {
            value if value == i32::from(b'k') => {
                self.pos += 1;
                if self.char_code(self.pos) == i32::from(b'<') {
                    self.pos += 1;
                    self.scan_group_name(true);
                    self.scan_expected_char(b'>');
                } else if self.any_unicode_mode_or_non_annex_b || self.named_capture_groups {
                    self.error(
                        &diagnostics::k_must_be_followed_by_a_capturing_group_name_enclosed_in_angle_brackets,
                        self.pos.saturating_sub(2),
                        2,
                        Vec::new(),
                    );
                }
            }
            value if value == i32::from(b'q') && self.unicode_sets_mode => {
                self.pos += 1;
                self.error(
                    &diagnostics::q_is_only_available_inside_character_class,
                    self.pos.saturating_sub(2),
                    2,
                    Vec::new(),
                );
            }
            _ => {
                if !self.scan_character_class_escape() && !self.scan_decimal_escape() {
                    self.scan_character_escape(true);
                }
            }
        }
    }

    fn scan_decimal_escape(&mut self) -> bool {
        let ch = self.char_code(self.pos);
        if ch < i32::from(b'1') || ch > i32::from(b'9') {
            return false;
        }
        let start = self.pos;
        let value = self.scan_digits();
        self.decimal_escapes.push(DecimalEscape {
            pos: start,
            end: self.pos,
            value: parse_decimal(&value),
        });
        true
    }

    fn scan_character_escape(&mut self, atom_escape: bool) -> Vec<u16> {
        let slash = self.pos.saturating_sub(1);
        let ch = self.char_code(self.pos);
        match ch {
            EOF => {
                self.error(
                    &diagnostics::Undetermined_character_escape,
                    slash,
                    1,
                    Vec::new(),
                );
                vec![b'\\' as u16]
            }
            value if value == i32::from(b'c') => {
                self.pos += 1;
                let next = self.char_code(self.pos);
                if is_ascii_letter(next) {
                    self.pos += 1;
                    vec![(next as u16) & 31]
                } else {
                    if self.any_unicode_mode_or_non_annex_b {
                        self.error(
                            &diagnostics::c_must_be_followed_by_an_ASCII_letter,
                            self.pos.saturating_sub(2),
                            2,
                            Vec::new(),
                        );
                    } else if atom_escape {
                        self.pos = self.pos.saturating_sub(1);
                        return vec![b'\\' as u16];
                    }
                    code_unit_result(next)
                }
            }
            value
                if matches!(
                    value as u8,
                    b'^' | b'$'
                        | b'/'
                        | b'\\'
                        | b'.'
                        | b'*'
                        | b'+'
                        | b'?'
                        | b'('
                        | b')'
                        | b'['
                        | b']'
                        | b'{'
                        | b'}'
                        | b'|'
                ) =>
            {
                self.pos += 1;
                vec![value as u16]
            }
            _ => {
                self.pos = self.pos.saturating_sub(1);
                self.scan_escape_sequence(atom_escape)
            }
        }
    }

    fn scan_escape_sequence(&mut self, atom_escape: bool) -> Vec<u16> {
        let start = self.pos;
        self.pos += 1;
        if self.pos >= self.end {
            self.error_here(&diagnostics::Unexpected_end_of_text);
            return Vec::new();
        }
        let ch = self.char_code(self.pos);
        self.pos += 1;
        match ch {
            value if value >= i32::from(b'0') && value <= i32::from(b'7') => {
                let first = value;
                if first == i32::from(b'0')
                    && (self.pos >= self.end || !is_digit(self.char_code(self.pos)))
                {
                    return vec![0];
                }
                if first <= i32::from(b'3') && is_octal_digit(self.char_code(self.pos)) {
                    self.pos += 1;
                }
                if is_octal_digit(self.char_code(self.pos)) {
                    self.pos += 1;
                }
                let raw = self.slice_string(start + 1, self.pos);
                let code = u16::from_str_radix(&raw, 8).unwrap_or(0);
                let replacement = format!("\\x{code:02x}");
                if !atom_escape && first != i32::from(b'0') {
                    self.error(
                        &diagnostics::Octal_escape_sequences_and_backreferences_are_not_allowed_in_a_character_class_If_this_was_intended_as_an_escape_sequence_use_the_syntax_0_instead,
                        start,
                        self.pos - start,
                        vec![replacement],
                    );
                } else {
                    self.error(
                        &diagnostics::Octal_escape_sequences_are_not_allowed_Use_the_syntax_0,
                        start,
                        self.pos - start,
                        vec![replacement],
                    );
                }
                vec![code]
            }
            value if value == i32::from(b'8') || value == i32::from(b'9') => {
                if !atom_escape {
                    self.error(
                        &diagnostics::Decimal_escape_sequences_and_backreferences_are_not_allowed_in_a_character_class,
                        start,
                        self.pos - start,
                        Vec::new(),
                    );
                } else {
                    let raw = self.slice_string(start, self.pos);
                    self.error(
                        &diagnostics::Escape_sequence_0_is_not_allowed,
                        start,
                        self.pos - start,
                        vec![raw],
                    );
                }
                vec![value as u16]
            }
            value if value == i32::from(b'b') => vec![8],
            value if value == i32::from(b't') => vec![9],
            value if value == i32::from(b'n') => vec![10],
            value if value == i32::from(b'v') => vec![11],
            value if value == i32::from(b'f') => vec![12],
            value if value == i32::from(b'r') => vec![13],
            value if value == i32::from(b'\'') => vec![b'\'' as u16],
            value if value == i32::from(b'"') => vec![b'"' as u16],
            value if value == i32::from(b'u') => {
                if self.char_code(self.pos) == i32::from(b'{') {
                    self.pos = self.pos.saturating_sub(2);
                    let result = self.scan_extended_unicode_escape(true);
                    if !self.any_unicode_mode {
                        self.error(
                            &diagnostics::Unicode_escape_sequences_are_only_available_when_the_Unicode_u_flag_or_the_Unicode_Sets_v_flag_is_set,
                            start,
                            self.pos - start,
                            Vec::new(),
                        );
                    }
                    return result;
                }
                while self.pos < start + 6 {
                    if self.pos >= self.end || !is_hex_digit(self.char_code(self.pos)) {
                        self.error_here(&diagnostics::Hexadecimal_digit_expected);
                        return self.slice(start, self.pos);
                    }
                    self.pos += 1;
                }
                let escaped =
                    u16::from_str_radix(&self.slice_string(start + 2, self.pos), 16).unwrap_or(0);
                let mut result = vec![escaped];
                if self.any_unicode_mode
                    && (0xD800..=0xDBFF).contains(&escaped)
                    && self.pos + 6 < self.end
                    && self.pair_is(self.pos, b'\\', b'u')
                    && self.char_code(self.pos + 2) != i32::from(b'{')
                {
                    let next_start = self.pos;
                    let next_end = next_start + 6;
                    if self.text[next_start + 2..next_end]
                        .iter()
                        .all(|unit| is_hex_digit(i32::from(*unit)))
                    {
                        let next = u16::from_str_radix(
                            &String::from_utf16_lossy(&self.text[next_start + 2..next_end]),
                            16,
                        )
                        .unwrap_or(0);
                        if (0xDC00..=0xDFFF).contains(&next) {
                            self.pos = next_end;
                            result.push(next);
                        }
                    }
                }
                result
            }
            value if value == i32::from(b'x') => {
                while self.pos < start + 4 {
                    if self.pos >= self.end || !is_hex_digit(self.char_code(self.pos)) {
                        self.error_here(&diagnostics::Hexadecimal_digit_expected);
                        return self.slice(start, self.pos);
                    }
                    self.pos += 1;
                }
                let escaped =
                    u16::from_str_radix(&self.slice_string(start + 2, self.pos), 16).unwrap_or(0);
                vec![escaped]
            }
            13 => {
                if self.char_code(self.pos) == 10 {
                    self.pos += 1;
                }
                Vec::new()
            }
            10 | 0x2028 | 0x2029 => Vec::new(),
            value => {
                if self.any_unicode_mode && is_identifier_part_code_point(value as u32, self.target)
                {
                    self.error(
                        &diagnostics::This_character_cannot_be_escaped_in_a_regular_expression,
                        self.pos.saturating_sub(2),
                        2,
                        Vec::new(),
                    );
                }
                code_unit_result(value)
            }
        }
    }

    fn scan_extended_unicode_escape(&mut self, report_error: bool) -> Vec<u16> {
        let start = self.pos;
        self.pos += 3;
        let escaped_start = self.pos;
        let digits = self.scan_hex_digits(1, true);
        let escaped = digits
            .as_deref()
            .map(|value| u64::from_str_radix(value, 16).unwrap_or(u64::MAX));
        let mut invalid = false;
        match escaped {
            None => {
                if report_error {
                    self.error_here(&diagnostics::Hexadecimal_digit_expected);
                }
                invalid = true;
            }
            Some(value) if value > 0x10FFFF => {
                if report_error {
                    self.error(
                        &diagnostics::An_extended_Unicode_escape_value_must_be_between_0x0_and_0x10FFFF_inclusive,
                        escaped_start,
                        self.pos - escaped_start,
                        Vec::new(),
                    );
                }
                invalid = true;
            }
            _ => {}
        }
        if self.pos >= self.end {
            if report_error {
                self.error_here(&diagnostics::Unexpected_end_of_text);
            }
            invalid = true;
        } else if self.char_code(self.pos) == i32::from(b'}') {
            self.pos += 1;
        } else {
            if report_error {
                self.error_here(&diagnostics::Unterminated_Unicode_escape_sequence);
            }
            invalid = true;
        }
        if invalid {
            self.slice(start, self.pos)
        } else {
            encode_code_point(
                u32::try_from(escaped.expect("valid extended Unicode escape"))
                    .expect("validated Unicode code point"),
            )
        }
    }

    fn scan_hex_digits(&mut self, minimum: usize, many: bool) -> Option<String> {
        let mut units = Vec::new();
        while units.len() < minimum || many {
            let ch = self.char_code(self.pos);
            if !is_hex_digit(ch) {
                break;
            }
            units.push((ch as u8).to_ascii_lowercase() as u16);
            self.pos += 1;
        }
        (units.len() >= minimum).then(|| String::from_utf16_lossy(&units))
    }

    fn scan_group_name(&mut self, is_reference: bool) {
        let start = self.pos;
        let name = self.scan_identifier();
        if self.pos == start {
            self.error_here(&diagnostics::Expected_a_capturing_group_name);
            return;
        }
        if is_reference {
            self.group_name_references.push(CaptureReference {
                pos: start,
                end: self.pos,
                name,
            });
            return;
        }
        let duplicate_in_top = self
            .top_named_capturing_groups_scope
            .as_ref()
            .is_some_and(|scope| scope.contains(&name));
        let duplicate_in_stack = self
            .named_capturing_groups_scope_stack
            .iter()
            .flatten()
            .any(|scope| scope.contains(&name));
        if duplicate_in_top || duplicate_in_stack {
            self.error(
                &diagnostics::Named_capturing_groups_with_the_same_name_must_be_mutually_exclusive_to_each_other,
                start,
                self.pos - start,
                Vec::new(),
            );
        } else {
            self.top_named_capturing_groups_scope
                .get_or_insert_with(Vec::new)
                .push(name.clone());
            if !self.group_specifiers.contains(&name) {
                self.group_specifiers.push(name);
            }
        }
    }

    fn scan_identifier(&mut self) -> String {
        let start = self.pos;
        let Some((first, size)) = self.code_point(self.pos) else {
            return String::new();
        };
        if !is_identifier_start_code_point(first, self.target) {
            return String::new();
        }
        self.pos += size;
        while let Some((code_point, size)) = self.code_point(self.pos) {
            if !is_identifier_part_code_point(code_point, self.target) {
                break;
            }
            self.pos += size;
        }
        if self.char_code(self.pos) == i32::from(b'\\') {
            let mut result = self.slice_string(start, self.pos);
            result.push_str(&self.scan_identifier_parts());
            result
        } else {
            self.slice_string(start, self.pos)
        }
    }

    fn scan_identifier_parts(&mut self) -> String {
        let mut result = String::new();
        let mut raw_start = self.pos;
        while self.pos < self.end {
            if let Some((code_point, size)) = self.code_point(self.pos) {
                if is_identifier_part_code_point(code_point, self.target) {
                    self.pos += size;
                    continue;
                }
            }
            if self.char_code(self.pos) != i32::from(b'\\') {
                break;
            }
            let saved = self.pos;
            if self.pair_is(self.pos, b'\\', b'u')
                && self.char_code(self.pos + 2) == i32::from(b'{')
            {
                let mut probe = self.pos + 3;
                let digit_start = probe;
                while is_hex_digit(self.char_code(probe)) {
                    probe += 1;
                }
                let value = (probe > digit_start)
                    .then(|| self.slice_string(digit_start, probe))
                    .and_then(|digits| u32::from_str_radix(&digits, 16).ok());
                if value.is_some_and(|value| {
                    value <= 0x10FFFF && is_identifier_part_code_point(value, self.target)
                }) {
                    result.push_str(&self.slice_string(raw_start, saved));
                    let decoded = self.scan_extended_unicode_escape(true);
                    result.push_str(&String::from_utf16_lossy(&decoded));
                    raw_start = self.pos;
                    continue;
                }
            } else if self.pair_is(self.pos, b'\\', b'u') && self.pos + 6 <= self.end {
                let units = &self.text[self.pos + 2..self.pos + 6];
                if units.iter().all(|unit| is_hex_digit(i32::from(*unit))) {
                    let value = u32::from_str_radix(&String::from_utf16_lossy(units), 16)
                        .unwrap_or(u32::MAX);
                    if is_identifier_part_code_point(value, self.target) {
                        result.push_str(&self.slice_string(raw_start, saved));
                        result.push_str(&String::from_utf16_lossy(&encode_code_point(value)));
                        self.pos += 6;
                        raw_start = self.pos;
                        continue;
                    }
                }
            }
            break;
        }
        result.push_str(&self.slice_string(raw_start, self.pos));
        result
    }

    fn is_class_content_exit(&self, ch: i32) -> bool {
        ch == i32::from(b']') || ch == EOF || self.pos >= self.end
    }

    fn scan_class_ranges(&mut self) {
        if self.char_code(self.pos) == i32::from(b'^') {
            self.pos += 1;
        }
        loop {
            let ch = self.char_code(self.pos);
            if self.is_class_content_exit(ch) {
                return;
            }
            let minimum_start = self.pos;
            let minimum = self.scan_class_atom();
            if self.char_code(self.pos) != i32::from(b'-') {
                continue;
            }
            self.pos += 1;
            let next = self.char_code(self.pos);
            if self.is_class_content_exit(next) {
                return;
            }
            if minimum.is_empty() && self.any_unicode_mode_or_non_annex_b {
                self.error(
                    &diagnostics::A_character_class_range_must_not_be_bounded_by_another_character_class,
                    minimum_start,
                    self.pos.saturating_sub(1 + minimum_start),
                    Vec::new(),
                );
            }
            let maximum_start = self.pos;
            let maximum = self.scan_class_atom();
            if maximum.is_empty() && self.any_unicode_mode_or_non_annex_b {
                self.error(
                    &diagnostics::A_character_class_range_must_not_be_bounded_by_another_character_class,
                    maximum_start,
                    self.pos - maximum_start,
                    Vec::new(),
                );
                continue;
            }
            if minimum.is_empty() {
                continue;
            }
            if single_code_point_value(&minimum)
                .zip(single_code_point_value(&maximum))
                .is_some_and(|(left, right)| left > right)
            {
                self.error(
                    &diagnostics::Range_out_of_order_in_character_class,
                    minimum_start,
                    self.pos - minimum_start,
                    Vec::new(),
                );
            }
        }
    }

    fn scan_class_set_expression(&mut self) {
        let mut character_complement = false;
        if self.char_code(self.pos) == i32::from(b'^') {
            self.pos += 1;
            character_complement = true;
        }
        let mut expression_may_contain_strings = false;
        let mut ch = self.char_code(self.pos);
        if self.is_class_content_exit(ch) {
            return;
        }
        let mut start = self.pos;
        let mut operand =
            if self.pair_is(self.pos, b'-', b'-') || self.pair_is(self.pos, b'&', b'&') {
                self.error_here(&diagnostics::Expected_a_class_set_operand);
                self.may_contain_strings = false;
                Vec::new()
            } else {
                self.scan_class_set_operand()
            };

        match self.char_code(self.pos) {
            value if value == i32::from(b'-') && self.pair_is(self.pos, b'-', b'-') => {
                if character_complement && self.may_contain_strings {
                    self.error(
                        &diagnostics::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                        start,
                        self.pos - start,
                        Vec::new(),
                    );
                }
                expression_may_contain_strings = self.may_contain_strings;
                self.scan_class_set_sub_expression(ClassExpressionType::Subtraction);
                self.may_contain_strings = !character_complement && expression_may_contain_strings;
                return;
            }
            value if value == i32::from(b'&') && self.pair_is(self.pos, b'&', b'&') => {
                self.scan_class_set_sub_expression(ClassExpressionType::Intersection);
                if character_complement && self.may_contain_strings {
                    self.error(
                        &diagnostics::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                        start,
                        self.pos - start,
                        Vec::new(),
                    );
                }
                expression_may_contain_strings = self.may_contain_strings;
                self.may_contain_strings = !character_complement && expression_may_contain_strings;
                return;
            }
            value if value == i32::from(b'&') => {
                self.unexpected_character(self.pos, 1, value);
            }
            _ => {
                if character_complement && self.may_contain_strings {
                    self.error(
                        &diagnostics::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                        start,
                        self.pos - start,
                        Vec::new(),
                    );
                }
                expression_may_contain_strings = self.may_contain_strings;
            }
        }

        loop {
            ch = self.char_code(self.pos);
            if ch == EOF {
                break;
            }
            match ch {
                value if value == i32::from(b'-') => {
                    self.pos += 1;
                    ch = self.char_code(self.pos);
                    if self.is_class_content_exit(ch) {
                        self.may_contain_strings =
                            !character_complement && expression_may_contain_strings;
                        return;
                    }
                    if ch == i32::from(b'-') {
                        self.pos += 1;
                        self.error(
                            &diagnostics::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                            self.pos - 2,
                            2,
                            Vec::new(),
                        );
                        start = self.pos - 2;
                        operand = self.slice(start, self.pos);
                        continue;
                    }
                    if operand.is_empty() {
                        self.error(
                            &diagnostics::A_character_class_range_must_not_be_bounded_by_another_character_class,
                            start,
                            self.pos.saturating_sub(1 + start),
                            Vec::new(),
                        );
                    }
                    let second_start = self.pos;
                    let second = self.scan_class_set_operand();
                    if character_complement && self.may_contain_strings {
                        self.error(
                            &diagnostics::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                            second_start,
                            self.pos - second_start,
                            Vec::new(),
                        );
                    }
                    expression_may_contain_strings |= self.may_contain_strings;
                    if second.is_empty() {
                        self.error(
                            &diagnostics::A_character_class_range_must_not_be_bounded_by_another_character_class,
                            second_start,
                            self.pos - second_start,
                            Vec::new(),
                        );
                        break;
                    }
                    if operand.is_empty() {
                        break;
                    }
                    if single_code_point_value(&operand)
                        .zip(single_code_point_value(&second))
                        .is_some_and(|(left, right)| left > right)
                    {
                        self.error(
                            &diagnostics::Range_out_of_order_in_character_class,
                            start,
                            self.pos - start,
                            Vec::new(),
                        );
                    }
                }
                value if value == i32::from(b'&') => {
                    start = self.pos;
                    self.pos += 1;
                    if self.char_code(self.pos) == i32::from(b'&') {
                        self.pos += 1;
                        self.error(
                            &diagnostics::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                            self.pos - 2,
                            2,
                            Vec::new(),
                        );
                        if self.char_code(self.pos) == i32::from(b'&') {
                            self.unexpected_character(self.pos, 1, value);
                            self.pos += 1;
                        }
                    } else {
                        self.unexpected_character(self.pos - 1, 1, value);
                    }
                    operand = self.slice(start, self.pos);
                    continue;
                }
                _ => {}
            }
            if self.is_class_content_exit(self.char_code(self.pos)) {
                break;
            }
            start = self.pos;
            if self.pair_is(self.pos, b'-', b'-') || self.pair_is(self.pos, b'&', b'&') {
                self.error(
                    &diagnostics::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                    self.pos,
                    2,
                    Vec::new(),
                );
                self.pos += 2;
                operand = self.slice(start, self.pos);
            } else {
                operand = self.scan_class_set_operand();
            }
        }
        self.may_contain_strings = !character_complement && expression_may_contain_strings;
    }

    fn scan_class_set_sub_expression(&mut self, expression_type: ClassExpressionType) {
        let mut expression_may_contain_strings = self.may_contain_strings;
        loop {
            let mut ch = self.char_code(self.pos);
            if self.is_class_content_exit(ch) {
                break;
            }
            match ch {
                value if value == i32::from(b'-') => {
                    self.pos += 1;
                    if self.char_code(self.pos) == i32::from(b'-') {
                        self.pos += 1;
                        if expression_type != ClassExpressionType::Subtraction {
                            self.error(
                                &diagnostics::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                                self.pos - 2,
                                2,
                                Vec::new(),
                            );
                        }
                    } else {
                        self.error(
                            &diagnostics::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                            self.pos - 1,
                            1,
                            Vec::new(),
                        );
                    }
                }
                value if value == i32::from(b'&') => {
                    self.pos += 1;
                    if self.char_code(self.pos) == i32::from(b'&') {
                        self.pos += 1;
                        if expression_type != ClassExpressionType::Intersection {
                            self.error(
                                &diagnostics::Operators_must_not_be_mixed_within_a_character_class_Wrap_it_in_a_nested_class_instead,
                                self.pos - 2,
                                2,
                                Vec::new(),
                            );
                        }
                        if self.char_code(self.pos) == i32::from(b'&') {
                            self.unexpected_character(self.pos, 1, value);
                            self.pos += 1;
                        }
                    } else {
                        self.unexpected_character(self.pos - 1, 1, value);
                    }
                }
                _ => {
                    let expected = match expression_type {
                        ClassExpressionType::Subtraction => "--",
                        ClassExpressionType::Intersection => "&&",
                    };
                    self.expected_character(self.pos, expected);
                }
            }
            ch = self.char_code(self.pos);
            if self.is_class_content_exit(ch) {
                self.error_here(&diagnostics::Expected_a_class_set_operand);
                break;
            }
            self.scan_class_set_operand();
            expression_may_contain_strings &= self.may_contain_strings;
        }
        self.may_contain_strings = expression_may_contain_strings;
    }

    fn scan_class_set_operand(&mut self) -> Vec<u16> {
        self.may_contain_strings = false;
        match self.char_code(self.pos) {
            EOF => Vec::new(),
            value if value == i32::from(b'[') => {
                self.pos += 1;
                self.scan_class_set_expression();
                self.scan_expected_char(b']');
                Vec::new()
            }
            value if value == i32::from(b'\\') => {
                self.pos += 1;
                if self.scan_character_class_escape() {
                    Vec::new()
                } else if self.char_code(self.pos) == i32::from(b'q') {
                    self.pos += 1;
                    if self.char_code(self.pos) == i32::from(b'{') {
                        self.pos += 1;
                        self.scan_class_string_disjunction_contents();
                        self.scan_expected_char(b'}');
                        Vec::new()
                    } else {
                        self.error(
                            &diagnostics::q_must_be_followed_by_string_alternatives_enclosed_in_braces,
                            self.pos.saturating_sub(2),
                            2,
                            Vec::new(),
                        );
                        vec![b'q' as u16]
                    }
                } else {
                    self.pos = self.pos.saturating_sub(1);
                    self.scan_class_set_character()
                }
            }
            _ => self.scan_class_set_character(),
        }
    }

    fn scan_class_string_disjunction_contents(&mut self) {
        let mut character_count = 0usize;
        loop {
            match self.char_code(self.pos) {
                EOF => return,
                value if value == i32::from(b'}') => {
                    if character_count != 1 {
                        self.may_contain_strings = true;
                    }
                    return;
                }
                value if value == i32::from(b'|') => {
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

    fn scan_class_set_character(&mut self) -> Vec<u16> {
        let ch = self.char_code(self.pos);
        if ch == EOF {
            return Vec::new();
        }
        if ch == i32::from(b'\\') {
            self.pos += 1;
            let escaped = self.char_code(self.pos);
            match escaped {
                value if value == i32::from(b'b') => {
                    self.pos += 1;
                    return vec![8];
                }
                value
                    if matches!(
                        value as u8,
                        b'&' | b'-'
                            | b'!'
                            | b'#'
                            | b'%'
                            | b','
                            | b':'
                            | b';'
                            | b'<'
                            | b'='
                            | b'>'
                            | b'@'
                            | b'`'
                            | b'~'
                    ) =>
                {
                    self.pos += 1;
                    return vec![value as u16];
                }
                _ => return self.scan_character_escape(false),
            }
        }
        if ch == self.char_code(self.pos + 1)
            && matches!(
                ch as u8,
                b'&' | b'!'
                    | b'#'
                    | b'%'
                    | b'*'
                    | b'+'
                    | b','
                    | b'.'
                    | b':'
                    | b';'
                    | b'<'
                    | b'='
                    | b'>'
                    | b'?'
                    | b'@'
                    | b'`'
                    | b'~'
            )
        {
            self.error(
                &diagnostics::A_character_class_must_not_contain_a_reserved_double_punctuator_Did_you_mean_to_escape_it_with_backslash,
                self.pos,
                2,
                Vec::new(),
            );
            let start = self.pos;
            self.pos += 2;
            return self.slice(start, self.pos);
        }
        if matches!(
            ch as u8,
            b'/' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'-' | b'|'
        ) {
            self.unexpected_character(self.pos, 1, ch);
            self.pos += 1;
            return vec![ch as u16];
        }
        self.scan_source_character()
    }

    fn scan_class_atom(&mut self) -> Vec<u16> {
        if self.char_code(self.pos) != i32::from(b'\\') {
            return self.scan_source_character();
        }
        self.pos += 1;
        let ch = self.char_code(self.pos);
        match ch {
            value if value == i32::from(b'b') => {
                self.pos += 1;
                vec![8]
            }
            value if value == i32::from(b'-') => {
                self.pos += 1;
                vec![b'-' as u16]
            }
            _ => {
                if self.scan_character_class_escape() {
                    Vec::new()
                } else {
                    self.scan_character_escape(false)
                }
            }
        }
    }

    fn scan_character_class_escape(&mut self) -> bool {
        let start = self.pos.saturating_sub(1);
        let ch = self.char_code(self.pos);
        match ch {
            value if matches!(value as u8, b'd' | b'D' | b's' | b'S' | b'w' | b'W') => {
                self.pos += 1;
                true
            }
            value if value == i32::from(b'p') || value == i32::from(b'P') => {
                let character_complement = value == i32::from(b'P');
                self.pos += 1;
                if self.char_code(self.pos) == i32::from(b'{') {
                    self.pos += 1;
                    let name_or_value_start = self.pos;
                    let name_or_value = self.scan_word_characters();
                    if self.char_code(self.pos) == i32::from(b'=') {
                        let property_name =
                            NON_BINARY_UNICODE_PROPERTIES
                                .iter()
                                .find_map(|(name, canonical)| {
                                    (*name == name_or_value).then_some(*canonical)
                                });
                        if self.pos == name_or_value_start {
                            self.error_here(&diagnostics::Expected_a_Unicode_property_name);
                        } else if property_name.is_none() {
                            self.error(
                                &diagnostics::Unknown_Unicode_property_name,
                                name_or_value_start,
                                self.pos - name_or_value_start,
                                Vec::new(),
                            );
                            if let Some(suggestion) = get_spelling_suggestion_strs(
                                &name_or_value,
                                NON_BINARY_UNICODE_PROPERTIES.iter().map(|(name, _)| *name),
                            ) {
                                self.error(
                                    &diagnostics::Did_you_mean_0,
                                    name_or_value_start,
                                    self.pos - name_or_value_start,
                                    vec![suggestion.to_owned()],
                                );
                            }
                        }
                        self.pos += 1;
                        let value_start = self.pos;
                        let property_value = self.scan_word_characters();
                        if self.pos == value_start {
                            self.error_here(&diagnostics::Expected_a_Unicode_property_value);
                        } else if let Some(property_name) = property_name {
                            let values = unicode_property_values(property_name);
                            if !values.contains(&property_value.as_str()) {
                                self.error(
                                    &diagnostics::Unknown_Unicode_property_value,
                                    value_start,
                                    self.pos - value_start,
                                    Vec::new(),
                                );
                                if let Some(suggestion) = get_spelling_suggestion_strs(
                                    &property_value,
                                    values.iter().copied(),
                                ) {
                                    self.error(
                                        &diagnostics::Did_you_mean_0,
                                        value_start,
                                        self.pos - value_start,
                                        vec![suggestion.to_owned()],
                                    );
                                }
                            }
                        }
                    } else if self.pos == name_or_value_start {
                        self.error_here(&diagnostics::Expected_a_Unicode_property_name_or_value);
                    } else if BINARY_UNICODE_PROPERTIES_OF_STRINGS.contains(&name_or_value.as_str())
                    {
                        if !self.unicode_sets_mode {
                            self.error(
                                &diagnostics::Any_Unicode_property_that_would_possibly_match_more_than_a_single_character_is_only_available_when_the_Unicode_Sets_v_flag_is_set,
                                name_or_value_start,
                                self.pos - name_or_value_start,
                                Vec::new(),
                            );
                        } else if character_complement {
                            self.error(
                                &diagnostics::Anything_that_would_possibly_match_more_than_a_single_character_is_invalid_inside_a_negated_character_class,
                                name_or_value_start,
                                self.pos - name_or_value_start,
                                Vec::new(),
                            );
                        } else {
                            self.may_contain_strings = true;
                        }
                    } else if !GENERAL_CATEGORY_VALUES.contains(&name_or_value.as_str())
                        && !BINARY_UNICODE_PROPERTIES.contains(&name_or_value.as_str())
                    {
                        self.error(
                            &diagnostics::Unknown_Unicode_property_name_or_value,
                            name_or_value_start,
                            self.pos - name_or_value_start,
                            Vec::new(),
                        );
                        let candidates = GENERAL_CATEGORY_VALUES
                            .iter()
                            .chain(BINARY_UNICODE_PROPERTIES)
                            .chain(BINARY_UNICODE_PROPERTIES_OF_STRINGS)
                            .copied();
                        if let Some(suggestion) =
                            get_spelling_suggestion_strs(&name_or_value, candidates)
                        {
                            self.error(
                                &diagnostics::Did_you_mean_0,
                                name_or_value_start,
                                self.pos - name_or_value_start,
                                vec![suggestion.to_owned()],
                            );
                        }
                    }
                    self.scan_expected_char(b'}');
                    if !self.any_unicode_mode {
                        self.error(
                            &diagnostics::Unicode_property_value_expressions_are_only_available_when_the_Unicode_u_flag_or_the_Unicode_Sets_v_flag_is_set,
                            start,
                            self.pos - start,
                            Vec::new(),
                        );
                    }
                } else if self.any_unicode_mode_or_non_annex_b {
                    self.error(
                        &diagnostics::_0_must_be_followed_by_a_Unicode_property_value_expression_enclosed_in_braces,
                        self.pos.saturating_sub(2),
                        2,
                        vec![code_unit_string(ch)],
                    );
                } else {
                    self.pos = self.pos.saturating_sub(1);
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    fn scan_word_characters(&mut self) -> String {
        let start = self.pos;
        while is_word_character(self.char_code(self.pos)) {
            self.pos += 1;
        }
        self.slice_string(start, self.pos)
    }

    fn scan_digits(&mut self) -> String {
        let start = self.pos;
        while is_digit(self.char_code(self.pos)) {
            self.pos += 1;
        }
        self.slice_string(start, self.pos)
    }

    fn scan_source_character(&mut self) -> Vec<u16> {
        let size = if self.any_unicode_mode {
            self.code_point(self.pos).map_or(0, |(_, size)| size)
        } else if self.pos < self.end {
            1
        } else {
            0
        };
        let start = self.pos;
        self.pos += size;
        self.slice(start, self.pos)
    }

    fn scan_expected_char(&mut self, ch: u8) {
        if self.char_code(self.pos) == i32::from(ch) {
            self.pos += 1;
        } else {
            self.expected_character(self.pos, &String::from(ch as char));
        }
    }

    fn expected_character(&mut self, start: usize, expected: &str) {
        self.error(
            &diagnostics::_0_expected,
            start,
            0,
            vec![expected.to_owned()],
        );
    }

    fn unexpected_character(&mut self, start: usize, length: usize, ch: i32) {
        self.error(
            &diagnostics::Unexpected_0_Did_you_mean_to_escape_it_with_backslash,
            start,
            length,
            vec![code_unit_string(ch)],
        );
    }

    fn finish_references(&mut self) {
        let references = std::mem::take(&mut self.group_name_references);
        for reference in references {
            if !self.group_specifiers.contains(&reference.name) {
                self.error(
                    &diagnostics::There_is_no_capturing_group_named_0_in_this_regular_expression,
                    reference.pos,
                    reference.end - reference.pos,
                    vec![reference.name.clone()],
                );
                if !self.group_specifiers.is_empty() {
                    if let Some(suggestion) =
                        get_spelling_suggestion(&reference.name, &self.group_specifiers)
                    {
                        self.error(
                            &diagnostics::Did_you_mean_0,
                            reference.pos,
                            reference.end - reference.pos,
                            vec![suggestion.to_owned()],
                        );
                    }
                }
            }
        }
        let escapes = std::mem::take(&mut self.decimal_escapes);
        for escape in escapes {
            if escape.value > self.number_of_capturing_groups {
                if self.number_of_capturing_groups == 0 {
                    self.error(
                        &diagnostics::This_backreference_refers_to_a_group_that_does_not_exist_There_are_no_capturing_groups_in_this_regular_expression,
                        escape.pos,
                        escape.end - escape.pos,
                        Vec::new(),
                    );
                } else {
                    self.error(
                        &diagnostics::This_backreference_refers_to_a_group_that_does_not_exist_There_are_only_0_capturing_groups_in_this_regular_expression,
                        escape.pos,
                        escape.end - escape.pos,
                        vec![self.number_of_capturing_groups.to_string()],
                    );
                }
            }
        }
    }
}

fn push_diagnostic(
    diagnostics: &mut Vec<RegexDiagnostic>,
    message: &'static DiagnosticMessage,
    start: usize,
    length: usize,
    args: Vec<String>,
) {
    diagnostics.push(RegexDiagnostic {
        message,
        start_utf16: start as u32,
        length_utf16: length as u32,
        args,
    });
}

fn check_flag_availability(
    diagnostics: &mut Vec<RegexDiagnostic>,
    target: ScriptTarget,
    flag: u16,
    start: usize,
    size: usize,
) {
    let available_from = match flag {
        FLAG_HAS_INDICES => Some((ScriptTarget::ES2022, "es2022")),
        FLAG_DOT_ALL => Some((ScriptTarget::ES2018, "es2018")),
        FLAG_UNICODE | FLAG_STICKY => Some((ScriptTarget::ES2015, "es6")),
        FLAG_UNICODE_SETS => Some((ScriptTarget::ES2024, "es2024")),
        _ => None,
    };
    if let Some((minimum, name)) = available_from {
        if target < minimum {
            push_diagnostic(
                diagnostics,
                &diagnostics::This_regular_expression_flag_is_only_available_when_targeting_0_or_later,
                start,
                size,
                vec![name.to_owned()],
            );
        }
    }
}

fn regular_expression_flag(code_point: u32) -> Option<u16> {
    match code_point {
        value if value == u32::from(b'd') => Some(FLAG_HAS_INDICES),
        value if value == u32::from(b'g') => Some(FLAG_GLOBAL),
        value if value == u32::from(b'i') => Some(FLAG_IGNORE_CASE),
        value if value == u32::from(b'm') => Some(FLAG_MULTILINE),
        value if value == u32::from(b's') => Some(FLAG_DOT_ALL),
        value if value == u32::from(b'u') => Some(FLAG_UNICODE),
        value if value == u32::from(b'v') => Some(FLAG_UNICODE_SETS),
        value if value == u32::from(b'y') => Some(FLAG_STICKY),
        _ => None,
    }
}

fn code_point_at(text: &[u16], pos: usize) -> Option<(u32, usize)> {
    let first = *text.get(pos)?;
    if (0xD800..=0xDBFF).contains(&first) {
        if let Some(&second) = text.get(pos + 1) {
            if (0xDC00..=0xDFFF).contains(&second) {
                let code_point =
                    0x10000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00);
                return Some((code_point, 2));
            }
        }
    }
    Some((u32::from(first), 1))
}

fn single_code_point_value(units: &[u16]) -> Option<u32> {
    let (value, size) = code_point_at(units, 0)?;
    (size == units.len()).then_some(value)
}

fn encode_code_point(value: u32) -> Vec<u16> {
    if value <= 0xFFFF {
        vec![value as u16]
    } else {
        let value = value - 0x10000;
        vec![
            0xD800 + ((value >> 10) as u16),
            0xDC00 + ((value & 0x3FF) as u16),
        ]
    }
}

fn is_identifier_start_code_point(code_point: u32, target: ScriptTarget) -> bool {
    char::from_u32(code_point).is_some_and(|ch| is_identifier_start(ch, target))
}

fn is_identifier_part_code_point(code_point: u32, target: ScriptTarget) -> bool {
    char::from_u32(code_point).is_some_and(|ch| is_identifier_part(ch, target))
}

fn is_digit(ch: i32) -> bool {
    ch >= i32::from(b'0') && ch <= i32::from(b'9')
}

fn is_octal_digit(ch: i32) -> bool {
    ch >= i32::from(b'0') && ch <= i32::from(b'7')
}

fn is_hex_digit(ch: i32) -> bool {
    is_digit(ch)
        || (ch >= i32::from(b'a') && ch <= i32::from(b'f'))
        || (ch >= i32::from(b'A') && ch <= i32::from(b'F'))
}

fn is_ascii_letter(ch: i32) -> bool {
    (ch >= i32::from(b'a') && ch <= i32::from(b'z'))
        || (ch >= i32::from(b'A') && ch <= i32::from(b'Z'))
}

fn is_word_character(ch: i32) -> bool {
    is_ascii_letter(ch) || is_digit(ch) || ch == i32::from(b'_')
}

fn unicode_property_values(property: &str) -> &'static [&'static str] {
    match property {
        "General_Category" => GENERAL_CATEGORY_VALUES,
        "Script" | "Script_Extensions" => SCRIPT_VALUES,
        _ => &[],
    }
}

fn parse_decimal(value: &str) -> u64 {
    value.parse().unwrap_or(u64::MAX)
}

fn parse_decimal_number(value: &str) -> f64 {
    value.parse().unwrap_or(f64::INFINITY)
}

fn code_unit_result(ch: i32) -> Vec<u16> {
    (ch >= 0).then_some(ch as u16).into_iter().collect()
}

fn code_unit_string(ch: i32) -> String {
    String::from_utf16_lossy(&code_unit_result(ch))
}

fn lowercase_unit(unit: u16) -> Vec<u16> {
    match char::from_u32(u32::from(unit)) {
        Some(scalar) => scalar
            .to_lowercase()
            .flat_map(|lowered| {
                let mut buffer = [0u16; 2];
                lowered.encode_utf16(&mut buffer).to_vec()
            })
            .collect(),
        None => vec![unit],
    }
}

fn levenshtein_with_max(s1: &[u16], s2: &[u16], max: f64) -> Option<f64> {
    let mut previous: Vec<f64> = (0..=s2.len()).map(|index| index as f64).collect();
    let mut current = vec![0.0; s2.len() + 1];
    let big = max + 0.01;
    for index1 in 1..=s1.len() {
        let min_index2 = (if index1 as f64 > max {
            index1 as f64 - max
        } else {
            1.0
        })
        .ceil() as usize;
        let max_index2 = (max + index1 as f64).min(s2.len() as f64).floor() as usize;
        current[0] = index1 as f64;
        let mut column_minimum = index1 as f64;
        for entry in current.iter_mut().take(min_index2).skip(1) {
            *entry = big;
        }
        for index2 in min_index2..=max_index2 {
            let substitution = if lowercase_unit(s1[index1 - 1]) == lowercase_unit(s2[index2 - 1]) {
                previous[index2 - 1] + 0.1
            } else {
                previous[index2 - 1] + 2.0
            };
            let distance = if s1[index1 - 1] == s2[index2 - 1] {
                previous[index2 - 1]
            } else {
                (previous[index2] + 1.0)
                    .min(current[index2 - 1] + 1.0)
                    .min(substitution)
            };
            current[index2] = distance;
            column_minimum = column_minimum.min(distance);
        }
        for entry in current.iter_mut().take(s2.len() + 1).skip(max_index2 + 1) {
            *entry = big;
        }
        if column_minimum > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let result = previous[s2.len()];
    (result <= max).then_some(result)
}

fn get_spelling_suggestion<'a>(name: &str, candidates: &'a [String]) -> Option<&'a str> {
    get_spelling_suggestion_iter(name, candidates.iter().map(String::as_str))
}

fn get_spelling_suggestion_strs<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    get_spelling_suggestion_iter(name, candidates)
}

fn get_spelling_suggestion_iter<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let name_units: Vec<u16> = name.encode_utf16().collect();
    let maximum_length_difference = 2.0_f64.max((name_units.len() as f64 * 0.34).floor());
    let mut best_distance = (name_units.len() as f64 * 0.4).floor() + 1.0;
    let mut best_candidate = None;
    for candidate in candidates {
        let candidate_units: Vec<u16> = candidate.encode_utf16().collect();
        if (candidate_units.len() as f64 - name_units.len() as f64).abs()
            > maximum_length_difference
            || candidate == name
            || candidate_units.len() < 3 && candidate.to_lowercase() != name.to_lowercase()
        {
            continue;
        }
        if let Some(distance) =
            levenshtein_with_max(&name_units, &candidate_units, best_distance - 0.1)
        {
            best_distance = distance;
            best_candidate = Some(candidate);
        }
    }
    best_candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsrs2_diags::DiagnosticCategory;

    fn diagnostics_for(text: &str, target: ScriptTarget) -> Vec<RegexDiagnostic> {
        validate_regular_expression_literal(text, target)
    }

    fn codes(text: &str, target: ScriptTarget) -> Vec<u32> {
        diagnostics_for(text, target)
            .into_iter()
            .map(|diagnostic| diagnostic.message.code)
            .collect()
    }

    #[test]
    fn validates_flags_and_target_gates() {
        let duplicate = diagnostics_for("/a/gg", ScriptTarget::ES_NEXT);
        assert_eq!(duplicate.len(), 1);
        assert_eq!(
            duplicate[0].message,
            &diagnostics::Duplicate_regular_expression_flag
        );
        assert_eq!(
            (duplicate[0].start_utf16, duplicate[0].length_utf16),
            (4, 1)
        );

        assert_eq!(
            codes("/a/z", ScriptTarget::ES_NEXT),
            vec![diagnostics::Unknown_regular_expression_flag.code]
        );
        assert_eq!(
            codes("/a/uv", ScriptTarget::ES_NEXT),
            vec![
                diagnostics::The_Unicode_u_flag_and_the_Unicode_Sets_v_flag_cannot_be_set_simultaneously
                    .code
            ]
        );
        for (literal, minimum, target_name) in [
            ("/a/u", ScriptTarget::ES2015, "es6"),
            ("/a/y", ScriptTarget::ES2015, "es6"),
            ("/a/s", ScriptTarget::ES2018, "es2018"),
            ("/a/d", ScriptTarget::ES2022, "es2022"),
            ("/a/v", ScriptTarget::ES2024, "es2024"),
        ] {
            let target = ScriptTarget::from_bits(minimum.bits() - 1);
            let actual = diagnostics_for(literal, target);
            assert_eq!(actual.len(), 1, "{literal}");
            assert_eq!(
                actual[0].message,
                &diagnostics::This_regular_expression_flag_is_only_available_when_targeting_0_or_later
            );
            assert_eq!(actual[0].args, vec![target_name]);
            assert!(diagnostics_for(literal, minimum).is_empty(), "{literal}");
        }
    }

    #[test]
    fn validates_extended_unicode_escapes_in_utf16_units() {
        let actual = diagnostics_for("/\\u{-DDDD}/gu", ScriptTarget::ES_NEXT);
        assert_eq!(
            actual
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16
                ))
                .collect::<Vec<_>>(),
            vec![
                (diagnostics::Hexadecimal_digit_expected.code, 4, 0),
                (diagnostics::Unterminated_Unicode_escape_sequence.code, 4, 0),
                (
                    diagnostics::Unexpected_0_Did_you_mean_to_escape_it_with_backslash.code,
                    9,
                    1
                ),
            ]
        );

        let supplementary = diagnostics_for("/😀{/u", ScriptTarget::ES_NEXT);
        assert_eq!(supplementary.len(), 1);
        assert_eq!(supplementary[0].start_utf16, 3);
        assert_eq!(
            supplementary[0].message,
            &diagnostics::Unexpected_0_Did_you_mean_to_escape_it_with_backslash
        );

        let overflowing = diagnostics_for("/\\u{FFFFFFFFFFFFFFFF}/u", ScriptTarget::ES_NEXT);
        assert_eq!(
            overflowing[0].message,
            &diagnostics::An_extended_Unicode_escape_value_must_be_between_0x0_and_0x10FFFF_inclusive
        );
        assert_eq!(
            (overflowing[0].start_utf16, overflowing[0].length_utf16),
            (4, 16)
        );
    }

    #[test]
    fn validates_groups_backreferences_and_subpattern_modifiers() {
        assert!(diagnostics_for("/(?<name>a)\\k<name>/u", ScriptTarget::ES_NEXT).is_empty());
        assert_eq!(
            codes("/(?<name>a)\\k<nme>/u", ScriptTarget::ES_NEXT),
            vec![
                diagnostics::There_is_no_capturing_group_named_0_in_this_regular_expression.code,
                diagnostics::Did_you_mean_0.code,
            ]
        );
        assert_eq!(
            codes("/\\1/u", ScriptTarget::ES_NEXT),
            vec![
                diagnostics::This_backreference_refers_to_a_group_that_does_not_exist_There_are_no_capturing_groups_in_this_regular_expression.code
            ]
        );
        assert_eq!(
            codes("/(?u:a)/u", ScriptTarget::ES_NEXT),
            vec![
                diagnostics::This_regular_expression_flag_cannot_be_toggled_within_a_subpattern
                    .code
            ]
        );
        assert_eq!(
            codes("/(?-:a)/u", ScriptTarget::ES_NEXT),
            vec![diagnostics::Subpattern_flags_must_be_present_when_there_is_a_minus_sign.code]
        );
        assert_eq!(
            diagnostics_for("/\\2(a)/u", ScriptTarget::ES_NEXT)
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16,
                ))
                .collect::<Vec<_>>(),
            vec![(
                diagnostics::This_backreference_refers_to_a_group_that_does_not_exist_There_are_only_0_capturing_groups_in_this_regular_expression.code,
                2,
                1,
            )]
        );
        assert_eq!(
            diagnostics_for("/fo(o/", ScriptTarget::ES_NEXT)
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16,
                ))
                .collect::<Vec<_>>(),
            vec![(diagnostics::_0_expected.code, 5, 0)]
        );
    }

    #[test]
    fn validates_classes_sets_and_unicode_properties() {
        assert_eq!(
            codes("/[z-a]/u", ScriptTarget::ES_NEXT),
            vec![diagnostics::Range_out_of_order_in_character_class.code]
        );
        assert_eq!(
            diagnostics_for("/[&&a]/v", ScriptTarget::ES_NEXT)
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16,
                ))
                .collect::<Vec<_>>(),
            vec![(diagnostics::Expected_a_class_set_operand.code, 2, 0)]
        );
        assert_eq!(
            diagnostics_for("/[!!]/v", ScriptTarget::ES_NEXT)
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16,
                ))
                .collect::<Vec<_>>(),
            vec![(
                diagnostics::A_character_class_must_not_contain_a_reserved_double_punctuator_Did_you_mean_to_escape_it_with_backslash.code,
                2,
                2,
            )]
        );

        let property = diagnostics_for("/\\p{General_Categor=Letter}/u", ScriptTarget::ES_NEXT);
        assert_eq!(
            property
                .iter()
                .map(|diagnostic| (
                    diagnostic.message.code,
                    diagnostic.message.category,
                    diagnostic.start_utf16,
                    diagnostic.length_utf16,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    diagnostics::Unknown_Unicode_property_name.code,
                    DiagnosticCategory::Error,
                    4,
                    15,
                ),
                (
                    diagnostics::Did_you_mean_0.code,
                    DiagnosticCategory::Message,
                    4,
                    15,
                ),
            ]
        );
        assert_eq!(property[1].args, vec!["General_Category"]);

        assert_eq!(
            codes("/\\p{Basic_Emoji}/u", ScriptTarget::ES_NEXT),
            vec![
                diagnostics::Any_Unicode_property_that_would_possibly_match_more_than_a_single_character_is_only_available_when_the_Unicode_Sets_v_flag_is_set.code
            ]
        );
        assert!(diagnostics_for("/\\p{Script=Latin}/u", ScriptTarget::ES_NEXT).is_empty());
        let property_value = diagnostics_for("/\\p{Script=Latn_}/u", ScriptTarget::ES_NEXT);
        assert_eq!(
            property_value
                .iter()
                .map(|diagnostic| diagnostic.message.code)
                .collect::<Vec<_>>(),
            vec![
                diagnostics::Unknown_Unicode_property_value.code,
                diagnostics::Did_you_mean_0.code,
            ]
        );
        assert_eq!(property_value[1].args, vec!["Latn"]);
    }
}

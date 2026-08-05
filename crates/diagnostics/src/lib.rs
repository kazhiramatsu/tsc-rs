#![forbid(unsafe_code)]

#[allow(non_upper_case_globals)]
pub mod gen;
pub mod line_map;
/// TypeScript 6.0.3-compatible, deterministic diagnostic rendering.
pub mod render;

use std::cmp::Ordering;

pub use line_map::{
    compute_line_map, compute_line_starts, get_line_and_character_of_position, LineAndCharacter,
    LineMap,
};
pub use render::{
    format_diagnostics_with_context, format_diagnostics_with_context_raw,
    format_sorted_diagnostics_with_context, format_sorted_diagnostics_with_context_raw,
    sort_and_dedupe_diagnostic_indices_with_context, FormatDiagnosticsError, FormatDiagnosticsHost,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCategory {
    Warning,
    Error,
    Suggestion,
    Message,
}

impl DiagnosticCategory {
    pub fn name(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Suggestion => "suggestion",
            Self::Message => "message",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticMessage {
    pub code: u32,
    pub category: DiagnosticCategory,
    pub text: &'static str,
    pub reports_unnecessary: bool,
    pub reports_deprecated: bool,
    pub elided_in_compatibility_pyramid: bool,
}

pub fn by_code(code: u32) -> Option<&'static DiagnosticMessage> {
    gen::ALL_BY_CODE
        .binary_search_by_key(&code, |(candidate, _)| *candidate)
        .ok()
        .map(|index| gen::ALL_BY_CODE[index].1)
}

pub fn format_message(template: &str, args: &[String]) -> String {
    if args.is_empty() {
        return template.to_owned();
    }

    let mut output = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch != '{' {
            output.push(ch);
            continue;
        }

        let mut end = start + ch.len_utf8();
        let mut number = String::new();
        while let Some((next_index, next_ch)) = chars.peek().copied() {
            if next_ch.is_ascii_digit() {
                number.push(next_ch);
                end = next_index + next_ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }

        if !number.is_empty() && chars.peek().is_some_and(|(_, next_ch)| *next_ch == '}') {
            chars.next();
            let index: usize = number.parse().expect("ASCII digits parse as usize");
            output.push_str(
                args.get(index)
                    .expect("diagnostic format argument is defined"),
            );
        } else {
            output.push_str(&template[start..end]);
        }
    }

    output
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageChain {
    pub code: u32,
    pub category: DiagnosticCategory,
    pub text: String,
    /// Whether tsc's `next` property exists. `undefined` and an empty
    /// array sort differently and are both observable in raw outcomes.
    pub next_present: bool,
    pub next: Vec<MessageChain>,
}

impl MessageChain {
    pub fn new(message: &'static DiagnosticMessage, args: &[String]) -> Self {
        Self {
            code: message.code,
            category: message.category,
            text: format_message(message.text, args),
            next_present: false,
            next: Vec::new(),
        }
    }

    pub fn with_next(mut self, next: Vec<MessageChain>) -> Self {
        self.next_present = true;
        self.next = next;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelatedInfo {
    pub file_name: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub message: MessageChain,
}

/// tsc CanonicalDiagnostic (getCanonicalDiagnostic 13977-13982): the
/// "plain form" a Did-you-mean diagnostic stands in for. Sort and
/// dedupe compare through it (getDiagnosticCode/getDiagnosticMessage
/// 17948-17954), so a 2552 with canonicalHead (2304, plain text)
/// occupies the plain 2304's slot and wins the keep-first dedupe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalHead {
    pub code: u32,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub file_name: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub message: MessageChain,
    /// Whether tsc's `relatedInformation` property exists, including
    /// the observable present-but-empty `[]` case. Non-empty `related`
    /// is treated as present even when legacy producers leave this
    /// marker false.
    pub related_information_present: bool,
    pub related: Vec<RelatedInfo>,
    pub canonical_head: Option<CanonicalHead>,
    /// Optional diagnostic properties propagated by
    /// createFileDiagnostic/createCompilerDiagnostic.
    pub reports_unnecessary: Option<bool>,
    pub reports_deprecated: Option<bool>,
    pub source: Option<String>,
    /// tsc Diagnostic.skippedOn (errorSkippedOn 47575): the program
    /// layer drops the diagnostic when the named option is set
    /// (filterSemanticDiagnostics 125664). "noEmit" is the only key
    /// any tsc emitter passes, so the field is a bool, not the key.
    pub skipped_on_no_emit: bool,
}

impl Diagnostic {
    pub fn new(
        file_name: Option<String>,
        start: Option<u32>,
        length: Option<u32>,
        message: MessageChain,
    ) -> Self {
        let metadata = by_code(message.code);
        Self {
            file_name,
            start,
            length,
            message,
            related_information_present: false,
            related: Vec::new(),
            canonical_head: None,
            reports_unnecessary: metadata
                .is_some_and(|message| message.reports_unnecessary)
                .then_some(true),
            reports_deprecated: metadata
                .is_some_and(|message| message.reports_deprecated)
                .then_some(true),
            source: None,
            skipped_on_no_emit: false,
        }
    }

    pub fn with_reports_unnecessary(mut self, value: Option<bool>) -> Self {
        self.reports_unnecessary = value;
        self
    }

    pub fn with_reports_deprecated(mut self, value: Option<bool>) -> Self {
        self.reports_deprecated = value;
        self
    }

    pub fn with_source(mut self, value: impl Into<String>) -> Self {
        self.source = Some(value.into());
        self
    }

    pub fn code(&self) -> u32 {
        self.message.code
    }

    pub fn category(&self) -> DiagnosticCategory {
        self.message.category
    }

    pub fn message_text(&self) -> &str {
        &self.message.text
    }

    /// tsc getDiagnosticCode (17948-17950): canonicalHead code wins.
    fn comparison_code(&self) -> u32 {
        self.canonical_head
            .as_ref()
            .map_or_else(|| self.code(), |head| head.code)
    }

    /// tsc getDiagnosticMessage (17951-17954): canonicalHead text wins.
    fn comparison_text(&self) -> &str {
        self.canonical_head
            .as_ref()
            .map_or_else(|| self.message_text(), |head| head.text.as_str())
    }
}

pub type DiagnosticList = Vec<Diagnostic>;

pub fn compare_diagnostics(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    compare_diagnostics_skip_related(left, right).then_with(|| {
        compare_related_information(
            left.related_information_present || !left.related.is_empty(),
            &left.related,
            right.related_information_present || !right.related.is_empty(),
            &right.related,
        )
    })
}

pub fn sort_and_dedupe_diagnostics(diagnostics: &mut DiagnosticList) {
    diagnostics.sort_by(compare_diagnostics);
    diagnostics.dedup_by(|right, left| diagnostics_equal(left, right));
}

fn compare_diagnostics_skip_related(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    compare_optional_strings_case_sensitive(left.file_name.as_deref(), right.file_name.as_deref())
        .then_with(|| left.start.cmp(&right.start))
        .then_with(|| left.length.cmp(&right.length))
        .then_with(|| left.comparison_code().cmp(&right.comparison_code()))
        .then_with(|| compare_diagnostic_message_text(left, right))
}

/// JavaScript relational string comparison is lexicographic over UTF-16
/// code units. Rust's `str::cmp` instead compares UTF-8 bytes, which differs
/// when an astral character is compared with a BMP character above its high
/// surrogate.
fn compare_strings_case_sensitive(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn compare_optional_strings_case_sensitive(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => compare_strings_case_sensitive(left, right),
    }
}

/// tsc compareMessageText (17863-17888): head text through the
/// canonical head, chains from the RAW message, then the
/// canonical-bearing-sorts-first tiebreaker.
fn compare_diagnostic_message_text(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    compare_strings_case_sensitive(left.comparison_text(), right.comparison_text())
        .then_with(|| {
            compare_message_chain(
                left.message.next_present,
                &left.message.next,
                right.message.next_present,
                &right.message.next,
            )
        })
        .then_with(|| {
            match (
                left.canonical_head.is_some(),
                right.canonical_head.is_some(),
            ) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => Ordering::Equal,
            }
        })
}

fn compare_related_information(
    left_present: bool,
    left: &[RelatedInfo],
    right_present: bool,
    right: &[RelatedInfo],
) -> Ordering {
    match (left_present, right_present) {
        (false, false) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (true, true) => right.len().cmp(&left.len()).then_with(|| {
            left.iter()
                .zip(right.iter())
                .map(|(left, right)| compare_related_info(left, right))
                .find(|ordering| *ordering != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        }),
    }
}

fn compare_related_info(left: &RelatedInfo, right: &RelatedInfo) -> Ordering {
    compare_optional_strings_case_sensitive(left.file_name.as_deref(), right.file_name.as_deref())
        .then_with(|| left.start.cmp(&right.start))
        .then_with(|| left.length.cmp(&right.length))
        .then_with(|| left.message.code.cmp(&right.message.code))
        .then_with(|| compare_message_text(&left.message, &right.message))
}

fn compare_message_text(left: &MessageChain, right: &MessageChain) -> Ordering {
    compare_strings_case_sensitive(&left.text, &right.text).then_with(|| {
        compare_message_chain(
            left.next_present,
            &left.next,
            right.next_present,
            &right.next,
        )
    })
}

fn compare_message_chain(
    left_present: bool,
    left: &[MessageChain],
    right_present: bool,
    right: &[MessageChain],
) -> Ordering {
    match (left_present, right_present) {
        (false, false) => Ordering::Equal,
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (true, true) => compare_message_chain_size(left, right)
            .then_with(|| compare_message_chain_content(left, right)),
    }
}

fn compare_message_chain_size(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    right.len().cmp(&left.len()).then_with(|| {
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| {
                compare_message_chain_size_optional(
                    left.next_present,
                    &left.next,
                    right.next_present,
                    &right.next,
                )
            })
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    })
}

fn compare_message_chain_size_optional(
    left_present: bool,
    left: &[MessageChain],
    right_present: bool,
    right: &[MessageChain],
) -> Ordering {
    match (left_present, right_present) {
        (false, false) => Ordering::Equal,
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (true, true) => compare_message_chain_size(left, right),
    }
}

fn compare_message_chain_content(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            compare_strings_case_sensitive(&left.text, &right.text)
                .then_with(|| compare_message_chain_content(&left.next, &right.next))
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// tsc diagnosticsEqualityComparer (17941-17947): file/span plus code
/// and HEAD TEXT compared through the canonical head — chains and
/// related information are ignored, which is what lets a canonical
/// 2552 swallow its plain 2304 twin.
fn diagnostics_equal(left: &Diagnostic, right: &Diagnostic) -> bool {
    left.file_name == right.file_name
        && left.start == right.start
        && left.length == right.length
        && left.comparison_code() == right.comparison_code()
        && left.comparison_text() == right.comparison_text()
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;

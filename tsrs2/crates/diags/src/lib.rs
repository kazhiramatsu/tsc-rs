#![forbid(unsafe_code)]

#[allow(non_upper_case_globals)]
pub mod gen;
pub mod line_map;

use std::cmp::Ordering;

pub use line_map::{
    compute_line_map, compute_line_starts, get_line_and_character_of_position, LineAndCharacter,
    LineMap,
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
    pub next: Vec<MessageChain>,
}

impl MessageChain {
    pub fn new(message: &'static DiagnosticMessage, args: &[String]) -> Self {
        Self {
            code: message.code,
            category: message.category,
            text: format_message(message.text, args),
            next: Vec::new(),
        }
    }

    pub fn with_next(mut self, next: Vec<MessageChain>) -> Self {
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
    pub related: Vec<RelatedInfo>,
    pub canonical_head: Option<CanonicalHead>,
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
        Self {
            file_name,
            start,
            length,
            message,
            related: Vec::new(),
            canonical_head: None,
            skipped_on_no_emit: false,
        }
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
    compare_diagnostics_skip_related(left, right)
        .then_with(|| compare_related_information(&left.related, &right.related))
}

pub fn sort_and_dedupe_diagnostics(diagnostics: &mut DiagnosticList) {
    diagnostics.sort_by(compare_diagnostics);
    diagnostics.dedup_by(|right, left| diagnostics_equal(left, right));
}

fn compare_diagnostics_skip_related(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    left.file_name
        .cmp(&right.file_name)
        .then_with(|| left.start.cmp(&right.start))
        .then_with(|| left.length.cmp(&right.length))
        .then_with(|| left.comparison_code().cmp(&right.comparison_code()))
        .then_with(|| compare_diagnostic_message_text(left, right))
}

/// tsc compareMessageText (17863-17888): head text through the
/// canonical head, chains from the RAW message, then the
/// canonical-bearing-sorts-first tiebreaker.
fn compare_diagnostic_message_text(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    left.comparison_text()
        .cmp(right.comparison_text())
        .then_with(|| compare_message_chain(&left.message.next, &right.message.next))
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

fn compare_related_information(left: &[RelatedInfo], right: &[RelatedInfo]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => Ordering::Equal,
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        (false, false) => right.len().cmp(&left.len()).then_with(|| {
            left.iter()
                .zip(right.iter())
                .map(|(left, right)| compare_related_info(left, right))
                .find(|ordering| *ordering != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        }),
    }
}

fn compare_related_info(left: &RelatedInfo, right: &RelatedInfo) -> Ordering {
    left.file_name
        .cmp(&right.file_name)
        .then_with(|| left.start.cmp(&right.start))
        .then_with(|| left.length.cmp(&right.length))
        .then_with(|| left.message.code.cmp(&right.message.code))
        .then_with(|| compare_message_text(&left.message, &right.message))
}

fn compare_message_text(left: &MessageChain, right: &MessageChain) -> Ordering {
    left.text
        .cmp(&right.text)
        .then_with(|| compare_message_chain(&left.next, &right.next))
}

fn compare_message_chain(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => compare_message_chain_size(left, right)
            .then_with(|| compare_message_chain_content(left, right)),
    }
}

fn compare_message_chain_size(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    right.len().cmp(&left.len()).then_with(|| {
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| compare_message_chain_size(&left.next, &right.next))
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    })
}

fn compare_message_chain_content(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            left.text
                .cmp(&right.text)
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
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn chain(code: u32, text: &str) -> MessageChain {
        MessageChain {
            code,
            category: DiagnosticCategory::Error,
            text: text.to_owned(),
            next: Vec::new(),
        }
    }

    fn diagnostic(
        file_name: Option<&str>,
        start: Option<u32>,
        code: u32,
        text: &str,
    ) -> Diagnostic {
        Diagnostic::new(
            file_name.map(str::to_owned),
            start,
            Some(1),
            chain(code, text),
        )
    }

    #[test]
    fn looks_up_generated_message_by_code() {
        let message = by_code(1005).expect("diagnostic 1005 exists");
        assert_eq!(message.text, "'{0}' expected.");
        assert_eq!(message.category, DiagnosticCategory::Error);
    }

    #[test]
    fn formats_placeholder_arguments() {
        assert_eq!(
            format_message("'{0}' expected.", &args(&[";"])),
            "';' expected."
        );
        assert_eq!(
            format_message("{1} before {0}", &args(&["b", "a"])),
            "a before b"
        );
    }

    #[test]
    fn sorts_and_deduplicates_adjacent_diagnostics() {
        let duplicate = diagnostic(Some("b.ts"), Some(1), 1005, "';' expected.");
        let mut diagnostics = vec![
            diagnostic(Some("a.ts"), Some(4), 1003, "Identifier expected."),
            duplicate.clone(),
            diagnostic(None, None, 1002, "Unterminated string literal."),
            duplicate,
        ];

        sort_and_dedupe_diagnostics(&mut diagnostics);

        assert_eq!(diagnostics.len(), 3);
        assert_eq!(diagnostics[0].file_name, None);
        assert_eq!(diagnostics[1].file_name.as_deref(), Some("a.ts"));
        assert_eq!(diagnostics[2].file_name.as_deref(), Some("b.ts"));
    }
}

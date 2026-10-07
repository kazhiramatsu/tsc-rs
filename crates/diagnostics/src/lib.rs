#![forbid(unsafe_code)]

#[allow(non_upper_case_globals)]
pub mod gen;
pub mod js_string;
pub mod line_map;
/// The pretty diagnostic writer and error summary, ported from tsgo.
pub mod render;
pub mod text;

use std::cmp::Ordering;

pub use js_string::{CodeUnits, JsStr, JsString, JsStringByteLength};

pub use line_map::{
    compute_line_map, compute_line_starts, get_line_and_character_of_position, LineMap,
};
pub use render::{
    format_diagnostic_with_color_and_context, format_diagnostics_with_color_and_context,
    sort_and_dedupe_diagnostic_indices_with_context, write_error_summary_text,
    FormatDiagnosticsError, FormatDiagnosticsHost, PrettyDiagnosticSources,
};
pub use text::{
    collapse_byte_changes, collapse_utf16_changes, ByteTextChangeRange, ByteTextSpan,
    DocumentVersion, LineAndCharacter, PositionIndex, PositionIndexKind, PositionUnit,
    TextEditError, TextEditOutcome, TextSnapshot, Utf16TextChangeRange, Utf16TextSpan,
    VersionedTextStore,
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
    /// tsgo `Message.Key()`: the message name plus `_code`, recorded in the
    /// build info for every cached diagnostic.
    pub key: &'static str,
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

/// A scalar-only formatting API: its template and arguments are Rust UTF-8.
pub fn format_message(template: &str, args: &[String]) -> String {
    format_message_value(template, args)
        .as_str()
        .expect("UTF-8 template and arguments produce Unicode scalars")
        .to_owned()
}

/// A diagnostic argument retains its JavaScript value until the output sink.
pub trait DiagnosticArgument {
    fn diagnostic_value(&self) -> JsStr<'_>;
}

impl DiagnosticArgument for str {
    fn diagnostic_value(&self) -> JsStr<'_> {
        JsStr::from_str(self)
    }
}
impl DiagnosticArgument for String {
    fn diagnostic_value(&self) -> JsStr<'_> {
        JsStr::from_str(self)
    }
}
impl DiagnosticArgument for JsString {
    fn diagnostic_value(&self) -> JsStr<'_> {
        self.as_js()
    }
}
impl DiagnosticArgument for JsStr<'_> {
    fn diagnostic_value(&self) -> JsStr<'_> {
        *self
    }
}
impl<T: DiagnosticArgument + ?Sized> DiagnosticArgument for &T {
    fn diagnostic_value(&self) -> JsStr<'_> {
        (*self).diagnostic_value()
    }
}

fn format_message_value<A: DiagnosticArgument>(template: &str, args: &[A]) -> JsString {
    if args.is_empty() {
        return template.into();
    }

    let mut output = JsString::with_capacity(template.len());
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
            output.push_js(
                args.get(index)
                    .expect("diagnostic format argument is defined")
                    .diagnostic_value(),
            );
        } else {
            output.push_str(&template[start..end]);
        }
    }

    output
}

#[derive(Clone, Debug)]
pub struct MessageChain {
    pub code: u32,
    pub category: DiagnosticCategory,
    pub text: JsString,
    /// tsgo `Diagnostic.MessageKey()`: the catalog message this chain entry
    /// was formatted from, when it was formatted from one. The build info
    /// records the key and the arguments instead of the text.
    pub key: Option<&'static str>,
    /// tsgo `Diagnostic.MessageArgs()`: the arguments as given, in order,
    /// including arguments the text does not use.
    pub args: Vec<JsString>,
    /// Whether tsc's `next` property exists. `undefined` and an empty
    /// array sort differently and are both observable in raw outcomes.
    pub next_present: bool,
    pub next: Vec<MessageChain>,
    /// The related information a nested entry carries (tsgo
    /// NewDiagnosticChain: a chain built over a diagnostic takes that
    /// diagnostic's related information, so the relater's related
    /// information sits on every level of its chain). The head's lives on
    /// the [`Diagnostic`]; equality ignores this, as tsgo's
    /// equalMessageChain does.
    pub related: Vec<RelatedInfo>,
}

/// The key and the arguments are a record of how the text was produced;
/// equality is the observable message (code, category, text, chain).
impl PartialEq for MessageChain {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
            && self.category == other.category
            && self.text == other.text
            && self.next_present == other.next_present
            && self.next == other.next
    }
}

impl Eq for MessageChain {}

impl MessageChain {
    pub fn new(message: &'static DiagnosticMessage, args: &[String]) -> Self {
        Self {
            code: message.code,
            category: message.category,
            text: format_message_value(message.text, args),
            key: Some(message.key),
            args: args.iter().map(JsString::from).collect(),
            next_present: false,
            next: Vec::new(),
            related: Vec::new(),
        }
    }

    pub fn new_js(message: &'static DiagnosticMessage, args: &[JsString]) -> Self {
        Self {
            code: message.code,
            category: message.category,
            text: format_message_value(message.text, args),
            key: Some(message.key),
            args: args.to_vec(),
            next_present: false,
            next: Vec::new(),
            related: Vec::new(),
        }
    }

    /// Canonical borrowed arguments, including mixtures of scalar text and
    /// JavaScript names. Formatting copies directly into the message owner.
    pub fn new_js_parts(message: &'static DiagnosticMessage, args: &[JsStr<'_>]) -> Self {
        Self {
            code: message.code,
            category: message.category,
            text: format_message_value(message.text, args),
            key: Some(message.key),
            args: args.iter().map(|arg| JsString::from(*arg)).collect(),
            next_present: false,
            next: Vec::new(),
            related: Vec::new(),
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
    pub file_name: Option<JsString>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub message: MessageChain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// The SourceFile's `FileName()`, which also orders diagnostics
    /// (tsgo's getDiagnosticPath, ast/diagnostic.go:390-395).
    pub file_name: Option<JsString>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub message: MessageChain,
    /// Whether tsc's `relatedInformation` property exists, including
    /// the observable present-but-empty `[]` case. Non-empty `related`
    /// is treated as present even when legacy producers leave this
    /// marker false.
    pub related_information_present: bool,
    pub related: Vec<RelatedInfo>,
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
        Self::new_js(file_name.map(JsString::from), start, length, message)
    }

    /// Retain a JavaScript file name independently from its eventual host or
    /// output encoding. No native-path projection participates in identity.
    pub fn new_js(
        file_name: Option<JsString>,
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

    pub fn message_text(&self) -> &JsString {
        &self.message.text
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
    // tsgo compactAndMergeRelatedInfos (compiler/program.go:1657-1688): a
    // run of diagnostics that differ only by related information becomes
    // one diagnostic whose related information is sorted and deduplicated.
    let mut kept: DiagnosticList = Vec::with_capacity(diagnostics.len());
    let mut merged = Vec::new();
    for mut diagnostic in std::mem::take(diagnostics) {
        if let Some(index) = kept.len().checked_sub(1) {
            let last = &mut kept[index];
            if diagnostics_equal(last, &diagnostic) {
                if last.message == diagnostic.message && last.related != diagnostic.related {
                    last.related.append(&mut diagnostic.related);
                    last.related_information_present = true;
                    if merged.last() != Some(&index) {
                        merged.push(index);
                    }
                }
                continue;
            }
        }
        kept.push(diagnostic);
    }
    for index in merged {
        let related = &mut kept[index].related;
        related.sort_by(compare_related_info);
        related.dedup_by(|right, left| compare_related_info(left, right) == Ordering::Equal);
    }
    *diagnostics = kept;
}

/// tsgo's CompareDiagnostics (ast/diagnostic.go:482-520) orders by the
/// file name (`getDiagnosticPath` is `File().FileName()`), where tsc 6.0
/// used `SourceFile.path`, which was empty for a parsed config file. The
/// code, the category and the source precede the message text. tsgo has
/// no canonical diagnostic: a "Did you mean" diagnostic sorts and
/// deduplicates by its own code and text, not by the plain form tsc 6.0
/// attached to it (getCanonicalDiagnostic).
fn compare_diagnostics_skip_related(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    compare_optional_strings_case_sensitive(
        left.file_name.as_ref().map(JsString::as_js),
        right.file_name.as_ref().map(JsString::as_js),
    )
    .then_with(|| left.start.cmp(&right.start))
    .then_with(|| left.length.cmp(&right.length))
    .then_with(|| left.code().cmp(&right.code()))
    .then_with(|| (left.category() as u8).cmp(&(right.category() as u8)))
    .then_with(|| left.source.cmp(&right.source))
    .then_with(|| compare_message_text(&left.message, &right.message))
}

/// JavaScript relational string comparison is lexicographic over UTF-16
/// code units. Rust's `str::cmp` instead compares UTF-8 bytes, which differs
/// when an astral character is compared with a BMP character above its high
/// surrogate.
fn compare_strings_case_sensitive<'a, 'b>(
    left: impl Into<JsStr<'a>>,
    right: impl Into<JsStr<'b>>,
) -> Ordering {
    left.into().cmp_utf16(right.into())
}

fn compare_optional_strings_case_sensitive(
    left: Option<JsStr<'_>>,
    right: Option<JsStr<'_>>,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => compare_strings_case_sensitive(left, right),
    }
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
    compare_optional_strings_case_sensitive(
        left.file_name.as_ref().map(JsString::as_js),
        right.file_name.as_ref().map(JsString::as_js),
    )
    .then_with(|| left.start.cmp(&right.start))
    .then_with(|| left.length.cmp(&right.length))
    .then_with(|| left.message.code.cmp(&right.message.code))
    .then_with(|| compare_message_text(&left.message, &right.message))
}

fn compare_message_text(left: &MessageChain, right: &MessageChain) -> Ordering {
    left.text.cmp_utf16(right.text.as_js()).then_with(|| {
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
            left.text
                .cmp_utf16(right.text.as_js())
                .then_with(|| compare_message_chain_content(&left.next, &right.next))
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// tsgo's EqualDiagnosticsNoRelatedInfo (ast/diagnostic.go:405-417):
/// file, span, code, category, source, message text and the whole
/// message chain. Two diagnostics at one location whose chains differ
/// are both kept: `checkInheritedPropertiesAreIdentical` reports one
/// TS2320 per property that is not identical. tsc 6.0 compared the head
/// text only (diagnosticsEqualityComparer) and kept the first.
fn diagnostics_equal(left: &Diagnostic, right: &Diagnostic) -> bool {
    left.file_name == right.file_name
        && left.start == right.start
        && left.length == right.length
        && left.code() == right.code()
        && left.category() == right.category()
        && left.source == right.source
        && message_chains_equal(&left.message, &right.message)
}

/// tsgo's equalMessageChain (ast/diagnostic.go:429-436): code, text and
/// the chain below, recursively.
fn message_chains_equal(left: &MessageChain, right: &MessageChain) -> bool {
    left.code == right.code
        && left.text == right.text
        && left.next.len() == right.next.len()
        && left
            .next
            .iter()
            .zip(right.next.iter())
            .all(|(left, right)| message_chains_equal(left, right))
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;

#![forbid(unsafe_code)]

#[allow(non_upper_case_globals)]
pub mod gen;
pub mod js_string;
pub mod line_map;
/// The pretty diagnostic writer and error summary, ported from tsgo.
pub mod render;
pub mod spanmap;
pub mod text;

use std::cmp::Ordering;

pub use js_string::{CodeUnits, JsStr, JsString, JsStringByteLength};

pub use line_map::{
    compute_line_map, compute_line_starts, get_line_and_character_of_position, LineMap,
};
pub use render::{
    format_diagnostic_with_color_and_context, format_diagnostics_with_color_and_context,
    rendered_file_name, sort_and_dedupe_diagnostic_indices_with_context, split_rendered_file_name,
    write_error_summary_text, FormatDiagnosticsError, FormatDiagnosticsHost,
    PrettyDiagnosticSources,
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

/// Translated message templates by message key: one of the languages
/// tsgo's messages are translated into (`diagnostics.Localize` reads its
/// embedded catalogs the same way).
pub trait MessageCatalog: Sync {
    /// The translated template of the message with this key, if the
    /// catalog has one.
    fn template(&self, key: &str) -> Option<&str>;
}

impl DiagnosticMessage {
    /// The message's template in a catalog's language (its English text
    /// when the catalog has no translation).
    pub fn template_in<'c>(&'static self, catalog: Option<&'c dyn MessageCatalog>) -> &'c str {
        catalog
            .and_then(|catalog| catalog.template(self.key))
            .unwrap_or(self.text)
    }

    /// The message formatted with `args` in a catalog's language.
    pub fn format_in<A: DiagnosticArgument>(
        &'static self,
        catalog: Option<&dyn MessageCatalog>,
        args: &[A],
    ) -> JsString {
        format_message_value(self.template_in(catalog), args)
    }
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
    /// How an incremental build recomputes this entry when it reuses the
    /// diagnostic; equality ignores it.
    pub repopulate: Option<Repopulate>,
    /// A nested entry tsgo builds with `ast.NewCompilerDiagnostic` (the
    /// program's chains: the include reasons, the module format details,
    /// the option hints) has no file and no location; a checker's nested
    /// entry has its diagnostic's (tsgo `NewDiagnosticChain`). Equality
    /// ignores this.
    pub without_location: bool,
}

/// tsgo `RepopulateDiagnosticInfo`: a chain entry that depends on the
/// program's package.json state. An incremental build that reuses the
/// diagnostic recomputes the entry from its program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Repopulate {
    /// tsgo `CreateModeMismatchDetails` of the diagnostic's file.
    ModeMismatch,
    /// tsgo `CreateModuleNotFoundChain` for a module request of the
    /// diagnostic's file.
    ModuleNotFound {
        module_reference: JsString,
        /// tsgo `core.ResolutionMode` of the request: 0, 1 (CommonJS) or
        /// 99 (ESNext).
        mode: u32,
        /// The package name, when it is not the request itself.
        package_name: Option<JsString>,
    },
}

impl MessageChain {
    /// This entry's text in a catalog's language: the translated template
    /// of its message formatted with its arguments, or its text.
    pub fn text_in(&self, catalog: Option<&dyn MessageCatalog>) -> JsString {
        match (catalog, self.key) {
            (Some(catalog), Some(key)) => match catalog.template(key) {
                Some(template) => format_message_value(template, &self.args),
                None => self.text.clone(),
            },
            _ => self.text.clone(),
        }
    }
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
            repopulate: None,
            without_location: false,
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
            repopulate: None,
            without_location: false,
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
            repopulate: None,
            without_location: false,
        }
    }

    /// tsgo `NewExternalDiagnostic`'s message: text reported by another
    /// tool (a content mapper) under its own code, not a catalog message.
    pub fn external(code: u32, category: DiagnosticCategory, text: impl Into<JsString>) -> Self {
        Self {
            code,
            category,
            text: text.into(),
            key: None,
            args: Vec::new(),
            next_present: false,
            next: Vec::new(),
            related: Vec::new(),
            repopulate: None,
            without_location: false,
        }
    }

    pub fn with_next(mut self, next: Vec<MessageChain>) -> Self {
        self.next_present = true;
        self.next = next;
        self
    }

    pub fn with_repopulate(mut self, repopulate: Repopulate) -> Self {
        self.repopulate = Some(repopulate);
        self
    }

    /// This entry and the entries nested under it as tsgo's
    /// `ast.NewCompilerDiagnostic` entries: without a file or a location.
    pub fn without_location(mut self) -> Self {
        self.without_location = true;
        self.next = self
            .next
            .into_iter()
            .map(MessageChain::without_location)
            .collect();
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

/// tsgo `CompareDiagnostics` (ast/diagnostic.go:482-520), related
/// information included: more related diagnostics first, then each in turn.
pub fn compare_diagnostics(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    compare_diagnostics_skip_related(left, right)
        .then_with(|| compare_related_information(&left.related, &right.related))
}

/// tsgo `DiagnosticsCollection` (ast/diagnostic.go:269-387): `Add` keeps one
/// of the diagnostics that are equal (`EqualDiagnostics`, related information
/// included) and `GetDiagnostics` returns them sorted by `CompareDiagnostics`.
/// Equal diagnostics compare equal, so after the sort they are neighbours.
pub fn sort_and_dedupe_equal_diagnostics(diagnostics: &mut DiagnosticList) {
    diagnostics.sort_by(compare_diagnostics);
    diagnostics.dedup_by(|right, left| {
        diagnostics_equal(left, right)
            && left.related.len() == right.related.len()
            && left
                .related
                .iter()
                .zip(&right.related)
                .all(|(left, right)| compare_related_info(left, right) == Ordering::Equal)
    });
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

/// tsgo's CompareDiagnostics (ast/diagnostic.go:482-520) without the
/// related information: the file name (`getDiagnosticPath` is
/// `File().FileName()`, where tsc 6.0 used `SourceFile.path`, which was empty
/// for a parsed config file), the span, the code, the category and the
/// source, then the message's identity (its catalog key, or its text when it
/// has none), its arguments and the chain below it: longer first, then by
/// the arguments of each entry. tsgo has no canonical diagnostic: a "Did you
/// mean" diagnostic sorts and deduplicates by its own message, not by the
/// plain form tsc 6.0 attached to it (getCanonicalDiagnostic).
fn compare_diagnostics_skip_related(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    compare_optional_strings(
        left.file_name.as_ref().map(JsString::as_js),
        right.file_name.as_ref().map(JsString::as_js),
    )
    .then_with(|| left.start.cmp(&right.start))
    .then_with(|| left.length.cmp(&right.length))
    .then_with(|| left.code().cmp(&right.code()))
    .then_with(|| (left.category() as u8).cmp(&(right.category() as u8)))
    .then_with(|| left.source.cmp(&right.source))
    .then_with(|| compare_message(&left.message, &right.message))
}

/// tsgo `getDiagnosticMessageIdentity` (ast/diagnostic.go:419-427): the
/// catalog key a message was formatted from, else its text.
fn message_identity(message: &MessageChain) -> &[u8] {
    match message.key {
        Some(key) => key.as_bytes(),
        None => message.text.as_js().as_bytes(),
    }
}

/// Go's `slices.Compare` over the arguments, each compared as Go compares
/// strings (by their UTF-8 bytes).
fn compare_arguments(left: &[JsString], right: &[JsString]) -> Ordering {
    left.iter()
        .map(|argument| argument.as_js().as_bytes())
        .cmp(right.iter().map(|argument| argument.as_js().as_bytes()))
}

/// The identity, the arguments and the chain of a message (the part of
/// CompareDiagnostics after the source).
fn compare_message(left: &MessageChain, right: &MessageChain) -> Ordering {
    message_identity(left)
        .cmp(message_identity(right))
        .then_with(|| compare_arguments(&left.args, &right.args))
        .then_with(|| compare_message_chain_size(&left.next, &right.next))
        .then_with(|| compare_message_chain_content(&left.next, &right.next))
}

/// Go's `strings.Compare`, which tsgo's comparisons use: the strings' UTF-8
/// bytes (a [`JsStr`] is WTF-8, the same order for any valid text). An
/// absent name sorts first, as tsgo's empty path does.
fn compare_optional_strings(left: Option<JsStr<'_>>, right: Option<JsStr<'_>>) -> Ordering {
    left.map(JsStr::as_bytes)
        .unwrap_or_default()
        .cmp(right.map(JsStr::as_bytes).unwrap_or_default())
}

/// tsgo `compareRelatedInfo` (ast/diagnostic.go:466-480): more first, then
/// each pair by CompareDiagnostics (a related diagnostic has no source and no
/// related information of its own here).
fn compare_related_information(left: &[RelatedInfo], right: &[RelatedInfo]) -> Ordering {
    right.len().cmp(&left.len()).then_with(|| {
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| compare_related_info(left, right))
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    })
}

fn compare_related_info(left: &RelatedInfo, right: &RelatedInfo) -> Ordering {
    compare_optional_strings(
        left.file_name.as_ref().map(JsString::as_js),
        right.file_name.as_ref().map(JsString::as_js),
    )
    .then_with(|| left.start.cmp(&right.start))
    .then_with(|| left.length.cmp(&right.length))
    .then_with(|| left.message.code.cmp(&right.message.code))
    .then_with(|| (left.message.category as u8).cmp(&(right.message.category as u8)))
    .then_with(|| compare_message(&left.message, &right.message))
}

/// tsgo `compareMessageChainSize` (ast/diagnostic.go:438-450): the longer
/// chain first, then the entries' own chains in turn.
fn compare_message_chain_size(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    right.len().cmp(&left.len()).then_with(|| {
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| compare_message_chain_size(&left.next, &right.next))
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    })
}

/// tsgo `compareMessageChainContent` (ast/diagnostic.go:452-464): the
/// entries' arguments, then their own chains.
fn compare_message_chain_content(left: &[MessageChain], right: &[MessageChain]) -> Ordering {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            compare_arguments(&left.args, &right.args)
                .then_with(|| compare_message_chain_content(&left.next, &right.next))
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

/// tsgo's EqualDiagnosticsNoRelatedInfo (ast/diagnostic.go:405-417):
/// file, span, code, category, source, the message's identity and
/// arguments, and the whole message chain. Two diagnostics at one location
/// whose chains differ are both kept: `checkInheritedPropertiesAreIdentical`
/// reports one TS2320 per property that is not identical. tsc 6.0 compared
/// the head text only (diagnosticsEqualityComparer) and kept the first.
fn diagnostics_equal(left: &Diagnostic, right: &Diagnostic) -> bool {
    left.file_name == right.file_name
        && left.start == right.start
        && left.length == right.length
        && left.code() == right.code()
        && left.category() == right.category()
        && left.source == right.source
        && message_identity(&left.message) == message_identity(&right.message)
        && left.message.args == right.message.args
        && message_chains_equal(&left.message.next, &right.message.next)
}

/// tsgo's equalMessageChain (ast/diagnostic.go:429-436): each entry's code,
/// arguments and chain, recursively.
fn message_chains_equal(left: &[MessageChain], right: &[MessageChain]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(left, right)| {
            left.code == right.code
                && left.args == right.args
                && message_chains_equal(&left.next, &right.next)
        })
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;

//! tsgo `lsconv.DiagnosticToLSPPush` and `DiagnosticToLSPPull`: a
//! compiler diagnostic as the protocol's, its message flattened in the
//! session's locale, its UTF-16 span converted to protocol positions.

use tsc_diagnostics::spanmap::SpanMap;
use tsc_diagnostics::{
    flattened_message, Diagnostic, DiagnosticCategory, MessageCatalog, PositionIndex,
};
use tsc_lsproto::{
    self as lsproto, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag,
    IntegerOrString, Location, StringOrMarkupContent,
};

use super::{Converters, Script};

/// tsgo `diagnosticOptions`: what the client takes.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticOptions {
    pub related_information: bool,
    pub tag_value_set: Vec<DiagnosticTag>,
    pub visual_studio: bool,
    pub report_style_checks_as_warnings: bool,
}

impl DiagnosticOptions {
    /// tsgo `DiagnosticToLSPPull`'s options: the client's capabilities for
    /// pulled diagnostics and the user's `reportStyleChecksAsWarnings`.
    pub fn pull(
        capabilities: &lsproto::ResolvedClientCapabilities,
        report_style_checks_as_warnings: bool,
    ) -> Self {
        let diagnostic = &capabilities.text_document.diagnostic;
        Self {
            related_information: diagnostic.related_information,
            tag_value_set: diagnostic.tag_support.value_set.clone(),
            visual_studio: capabilities._vs_supports_visual_studio_extensions,
            report_style_checks_as_warnings,
        }
    }

    /// tsgo `DiagnosticToLSPPush`'s options from the client's capabilities.
    pub fn push(capabilities: &lsproto::ResolvedClientCapabilities) -> Self {
        let publish = &capabilities.text_document.publish_diagnostics;
        Self {
            related_information: publish.related_information,
            tag_value_set: publish.tag_support.value_set.clone(),
            visual_studio: capabilities._vs_supports_visual_studio_extensions,
            report_style_checks_as_warnings: false,
        }
    }
}

/// A file as a script: its name and its text (tsgo `ls.script`), and for a
/// content mapper's output its original file.
pub struct TextScript<'a> {
    pub file_name: String,
    pub text: &'a str,
    pub mapped: Option<MappedScript<'a>>,
}

/// A content mapper's output's original file (tsgo `OriginalFileName`,
/// `OriginalText` and `SpanMap`).
pub struct MappedScript<'a> {
    pub original_file_name: String,
    pub original_text: &'a str,
    pub span_map: Option<&'a SpanMap>,
}

impl<'a> TextScript<'a> {
    /// An ordinary file's script.
    pub fn new(file_name: impl Into<String>, text: &'a str) -> Self {
        Self {
            file_name: file_name.into(),
            text,
            mapped: None,
        }
    }
}

impl Script for TextScript<'_> {
    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn text(&self) -> &str {
        self.text
    }

    fn original_file_name(&self) -> &str {
        self.mapped
            .as_ref()
            .map_or(&self.file_name, |mapped| &mapped.original_file_name)
    }

    fn span_map(&self) -> Option<&SpanMap> {
        self.mapped.as_ref().and_then(|mapped| mapped.span_map)
    }

    fn original_text(&self) -> &str {
        self.mapped
            .as_ref()
            .map_or(self.text, |mapped| mapped.original_text)
    }
}

/// tsgo's style check diagnostics (`styleCheckDiagnostics`), which
/// `reportStyleChecksAsWarnings` reports as warnings.
const STYLE_CHECK_DIAGNOSTICS: [u32; 8] = [6133, 6138, 6192, 6196, 7027, 7028, 7029, 7030];

/// tsgo `diagnosticSeverity`.
fn diagnostic_severity(category: DiagnosticCategory) -> DiagnosticSeverity {
    match category {
        DiagnosticCategory::Suggestion => DiagnosticSeverity::HINT,
        DiagnosticCategory::Message => DiagnosticSeverity::INFORMATION,
        DiagnosticCategory::Warning => DiagnosticSeverity::WARNING,
        DiagnosticCategory::Error => DiagnosticSeverity::ERROR,
    }
}

/// tsgo `diagnosticToLSP`: `file` gives the text of a file a diagnostic or
/// its related information is in.
pub fn diagnostic_to_lsp<'a>(
    converters: &Converters,
    diagnostic: &Diagnostic,
    options: &DiagnosticOptions,
    catalog: Option<&dyn MessageCatalog>,
    file: &dyn Fn(&str) -> Option<TextScript<'a>>,
) -> lsproto::Diagnostic {
    let mut severity = diagnostic_severity(diagnostic.category());
    if options.report_style_checks_as_warnings
        && severity == DiagnosticSeverity::ERROR
        && STYLE_CHECK_DIAGNOSTICS.contains(&diagnostic.code())
    {
        severity = DiagnosticSeverity::WARNING;
    }
    let related_information = options.related_information.then(|| {
        diagnostic
            .related
            .iter()
            .map(|related| {
                let file_name = related
                    .file_name
                    .as_ref()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // tsgo: a supplemental output's original file's URI.
                let uri_file_name = file(&file_name)
                    .map(|script| script.original_file_name().to_owned())
                    .unwrap_or_else(|| file_name.clone());
                DiagnosticRelatedInformation {
                    location: Location {
                        uri: lsproto::DocumentUri(tsc_program::uri::file_name_to_document_uri(
                            &uri_file_name,
                        )),
                        range: span_range(
                            converters,
                            file,
                            &file_name,
                            related.start,
                            related.length,
                            None,
                        ),
                    },
                    message: related
                        .message
                        .text_in(catalog)
                        .to_string_lossy()
                        .into_owned(),
                }
            })
            .collect::<Vec<_>>()
    });
    let reports_unnecessary = diagnostic.reports_unnecessary == Some(true);
    let reports_deprecated = diagnostic.reports_deprecated == Some(true);
    let tags = (!options.tag_value_set.is_empty() && (reports_unnecessary || reports_deprecated))
        .then(|| {
            let mut tags = Vec::new();
            if reports_unnecessary && options.tag_value_set.contains(&DiagnosticTag::UNNECESSARY) {
                tags.push(DiagnosticTag::UNNECESSARY);
            }
            if reports_deprecated && options.tag_value_set.contains(&DiagnosticTag::DEPRECATED) {
                tags.push(DiagnosticTag::DEPRECATED);
            }
            tags
        });
    // A diagnostic without a file (a program's) has a zero range.
    let range = match &diagnostic.file_name {
        Some(file_name) => span_range(
            converters,
            file,
            &file_name.to_string_lossy(),
            diagnostic.start,
            diagnostic.length,
            diagnostic.source.as_deref(),
        ),
        None => lsproto::Range::default(),
    };
    let code = if options.visual_studio {
        IntegerOrString::String(format!("TS{}", diagnostic.code()))
    } else {
        IntegerOrString::Integer(diagnostic.code() as i32)
    };
    lsproto::Diagnostic {
        range,
        severity: Some(severity),
        code: Some(code),
        source: Some(
            diagnostic
                .source
                .clone()
                .filter(|source| !source.is_empty())
                .unwrap_or_else(|| "ts".to_owned()),
        ),
        message: StringOrMarkupContent::String(
            flattened_message(&diagnostic.message, "\n", catalog)
                .to_string_lossy()
                .into_owned(),
        ),
        tags: tags.filter(|tags| !tags.is_empty()),
        related_information: related_information.filter(|related| !related.is_empty()),
        ..lsproto::Diagnostic::default()
    }
}

/// A diagnostic's UTF-16 span in a file as a protocol range; a file whose
/// text is not known has a zero range. In a content mapper's output (tsgo
/// `diagnosticScriptAndRange`), a mapper's own diagnostic (with a `source`)
/// is at its original file's range already, and one in synthesized code
/// has a zero range.
fn span_range<'a>(
    converters: &Converters,
    file: &dyn Fn(&str) -> Option<TextScript<'a>>,
    file_name: &str,
    start: Option<u32>,
    length: Option<u32>,
    source: Option<&str>,
) -> lsproto::Range {
    let Some(script) = file(file_name) else {
        return lsproto::Range::default();
    };
    let start = start.unwrap_or(0);
    let end = start.saturating_add(length.unwrap_or(0));
    let bytes = |text: &str, start: u32, end: u32| {
        let positions = PositionIndex::new_static(text);
        let byte = |position: u32| {
            positions
                .utf16_to_byte(position)
                .unwrap_or(text.len() as u32) as usize
        };
        byte(start)..byte(end)
    };
    if script.span_map().is_some() && source.is_some_and(|source| !source.is_empty()) {
        let original = TextScript::new(script.original_file_name(), script.original_text());
        return converters.to_lsp_range(&original, bytes(original.text, start, end));
    }
    let (range, fidelity) =
        converters.to_lsp_range_with_fidelity(&script, bytes(script.text, start, end));
    if fidelity.is_none() {
        // Diagnostics carry a range: a zero range means the file when the
        // diagnostic is entirely in synthesized code.
        return lsproto::Range::default();
    }
    range
}

//! Port of tsgo's diagnostic writer (`internal/diagnosticwriter`): the
//! `--pretty` reporter (`FormatDiagnosticWithColorAndContext`, ANSI styles
//! and source snippets) and the error summary (`WriteErrorSummaryText`),
//! with the command-line reporter's sort/deduplicate boundary
//! (`sortAndDeduplicateDiagnostics`). Positions are UTF-16 offsets.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::{
    compare_diagnostics, diagnostics_equal, Diagnostic, JsStr, JsString, MessageChain, TextSnapshot,
};

const FILE_APPEARS_TO_BE_BINARY: u32 = 1490;
const GREY: &str = "\u{1b}[90m";
const RED: &str = "\u{1b}[91m";
const YELLOW: &str = "\u{1b}[93m";
const BLUE: &str = "\u{1b}[94m";
const CYAN: &str = "\u{1b}[96m";
const GUTTER_STYLE: &str = "\u{1b}[7m";
const GUTTER_SEPARATOR: &str = " ";
const RESET: &str = "\u{1b}[0m";
const ELLIPSIS: &str = "...";

#[derive(Clone, Copy, Debug)]
pub struct FormatDiagnosticsHost<'a> {
    current_directory: JsStr<'a>,
    file_texts: DiagnosticSourceTexts<'a>,
}

#[derive(Clone, Copy, Debug)]
enum DiagnosticSourceTexts<'a> {
    Owned(&'a BTreeMap<String, String>),
    Snapshots(&'a BTreeMap<String, std::sync::Arc<TextSnapshot>>),
    JsOwned(&'a BTreeMap<JsString, String>),
    JsSnapshots(&'a BTreeMap<JsString, std::sync::Arc<TextSnapshot>>),
}

impl<'a> FormatDiagnosticsHost<'a> {
    pub fn new(current_directory: &'a str, file_texts: &'a BTreeMap<String, String>) -> Self {
        Self {
            current_directory: current_directory.into(),
            file_texts: DiagnosticSourceTexts::Owned(file_texts),
        }
    }

    pub fn from_snapshots(
        current_directory: &'a str,
        file_texts: &'a BTreeMap<String, std::sync::Arc<TextSnapshot>>,
    ) -> Self {
        Self {
            current_directory: current_directory.into(),
            file_texts: DiagnosticSourceTexts::Snapshots(file_texts),
        }
    }

    pub fn new_js(
        current_directory: JsStr<'a>,
        file_texts: &'a BTreeMap<JsString, String>,
    ) -> Self {
        Self {
            current_directory,
            file_texts: DiagnosticSourceTexts::JsOwned(file_texts),
        }
    }

    pub fn from_js_snapshots(
        current_directory: JsStr<'a>,
        file_texts: &'a BTreeMap<JsString, std::sync::Arc<TextSnapshot>>,
    ) -> Self {
        Self {
            current_directory,
            file_texts: DiagnosticSourceTexts::JsSnapshots(file_texts),
        }
    }

    fn file_text(&self, file_name: JsStr<'_>) -> Option<&'a str> {
        match self.file_texts {
            DiagnosticSourceTexts::Owned(file_texts) => {
                lookup_source_text(file_texts, file_name, String::as_str)
            }
            DiagnosticSourceTexts::Snapshots(file_texts) => {
                lookup_source_text(file_texts, file_name, |snapshot| snapshot.text())
            }
            DiagnosticSourceTexts::JsOwned(file_texts) => {
                lookup_js_source_text(file_texts, file_name, String::as_str)
            }
            DiagnosticSourceTexts::JsSnapshots(file_texts) => {
                lookup_js_source_text(file_texts, file_name, |snapshot| snapshot.text())
            }
        }
    }
}

fn lookup_source_text<'a, T>(
    file_texts: &'a BTreeMap<String, T>,
    file_name: JsStr<'_>,
    text: impl Fn(&'a T) -> &'a str,
) -> Option<&'a str> {
    // A scalar-keyed host cannot own a non-scalar name. This is a comparison
    // against that host's key domain, never a lossy projection of the query.
    if let Some(source) = file_name.as_str().and_then(|name| file_texts.get(name)) {
        return Some(text(source));
    }
    let normalized = normalize_slashes(file_name);
    file_texts
        .iter()
        .find(|(candidate, _)| normalize_slashes(candidate.as_str()) == normalized)
        .map(|(_, source)| text(source))
}

fn lookup_js_source_text<'a, T>(
    file_texts: &'a BTreeMap<JsString, T>,
    file_name: JsStr<'_>,
    text: impl Fn(&'a T) -> &'a str,
) -> Option<&'a str> {
    if let Some(source) = file_texts.get(file_name.as_bytes()) {
        return Some(text(source));
    }
    let normalized = normalize_slashes(file_name);
    file_texts
        .iter()
        .find(|(candidate, _)| normalize_slashes(candidate.as_js()) == normalized)
        .map(|(_, source)| text(source))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatDiagnosticsError {
    message: String,
}

impl FormatDiagnosticsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for FormatDiagnosticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for FormatDiagnosticsError {}

/// Return the exact input occurrence retained by tsc's stable,
/// cwd-aware sort/deduplicate boundary.
pub fn sort_and_dedupe_diagnostic_indices_with_context(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Vec<usize> {
    // The checker stores public/virtual names, while tsc sorts by the
    // host-absolutized and reduced SourceFile.path. Keep that comparison
    // twin beside the original SourceFile.fileName display record.
    let mut diagnostics = diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let mut comparison = diagnostic.clone();
            comparison.file_name = comparison
                .file_name
                .as_ref()
                .map(JsString::as_js)
                .map(|name| absolute_virtual_path(name, host.current_directory));
            for related in &mut comparison.related {
                related.file_name = related
                    .file_name
                    .as_ref()
                    .map(JsString::as_js)
                    .map(|name| absolute_virtual_path(name, host.current_directory));
            }
            (comparison, index)
        })
        .collect::<Vec<_>>();
    diagnostics.sort_by(|(left, _), (right, _)| compare_diagnostics(left, right));
    diagnostics.dedup_by(|(right, _), (left, _)| diagnostics_equal(left, right));
    diagnostics.into_iter().map(|(_, index)| index).collect()
}

/// What the pretty writer reads about a diagnostic's file.
pub trait PrettyDiagnosticSources {
    /// The name a location prints (tsgo `WriteLocation`: the file name
    /// relative to the current directory).
    fn location_name(&self, file_name: JsStr<'_>) -> JsString;
    /// The name the error summary prints (`prettyPathForFileError`).
    fn summary_name(&self, file_name: JsStr<'_>) -> JsString;
    /// The file's text.
    fn text(&self, file_name: JsStr<'_>) -> Option<&str>;
}

impl PrettyDiagnosticSources for FormatDiagnosticsHost<'_> {
    fn location_name(&self, file_name: JsStr<'_>) -> JsString {
        relative_file_name(file_name, self.current_directory)
    }

    fn summary_name(&self, file_name: JsStr<'_>) -> JsString {
        relative_file_name(file_name, self.current_directory)
    }

    fn text(&self, file_name: JsStr<'_>) -> Option<&str> {
        self.file_text(file_name)
    }
}

/// tsgo `FormatDiagnosticsWithColorAndContext`: the diagnostics in the
/// given order, separated by `new_line`.
pub fn format_diagnostics_with_color_and_context(
    diagnostics: &[Diagnostic],
    sources: &dyn PrettyDiagnosticSources,
    new_line: &str,
) -> Result<JsString, FormatDiagnosticsError> {
    let mut output = JsString::new();
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index > 0 {
            output.push_str(new_line);
        }
        format_diagnostic_with_color_and_context(&mut output, diagnostic, sources, new_line)?;
    }
    Ok(output)
}

/// tsgo `FormatDiagnosticWithColorAndContext`: the location, the styled
/// category and code, the flattened message and the source snippet, then
/// each related location with its message and snippet.
pub fn format_diagnostic_with_color_and_context(
    output: &mut JsString,
    diagnostic: &Diagnostic,
    sources: &dyn PrettyDiagnosticSources,
    new_line: &str,
) -> Result<(), FormatDiagnosticsError> {
    let file_name = diagnostic.file_name.as_ref().map(JsString::as_js);
    if let Some(file_name) = file_name {
        let start = required_position(diagnostic.start, file_name, "start")?;
        write_location(output, file_name, start, sources)?;
        output.push_str(" - ");
    }
    let category_format = category_format(diagnostic.category());
    write_with_style_and_reset(output, diagnostic.category().name(), category_format);
    output.push_str(GREY);
    output.push_str(&format!(" TS{}: ", diagnostic.code()));
    output.push_str(RESET);
    write_flattened_message(output, &diagnostic.message, new_line);

    if let Some(file_name) = file_name {
        if diagnostic.code() != FILE_APPEARS_TO_BE_BINARY {
            let start = required_position(diagnostic.start, file_name, "start")?;
            let length = required_position(diagnostic.length, file_name, "length")?;
            output.push_str(new_line);
            write_code_snippet(
                output,
                file_name,
                start,
                length,
                category_format,
                "",
                sources,
                new_line,
            )?;
            output.push_str(new_line);
        }
    }

    for related in &diagnostic.related {
        if let Some(file_name) = related.file_name.as_ref().map(JsString::as_js) {
            let start = required_position(related.start, file_name, "related start")?;
            let length = required_position(related.length, file_name, "related length")?;
            output.push_str(new_line);
            output.push_str("  ");
            write_location(output, file_name, start, sources)?;
            output.push_str(" - ");
            write_flattened_message(output, &related.message, new_line);
            write_code_snippet(
                output, file_name, start, length, CYAN, "    ", sources, new_line,
            )?;
        }
        output.push_str(new_line);
    }
    Ok(())
}

/// tsgo `WriteErrorSummaryText`: `Found N errors…` after the diagnostics,
/// with a table of the files when several have errors. Empty when there is
/// no error.
pub fn write_error_summary_text(
    diagnostics: &[Diagnostic],
    sources: &dyn PrettyDiagnosticSources,
    new_line: &str,
) -> Result<JsString, FormatDiagnosticsError> {
    let mut total = 0usize;
    let mut global = 0usize;
    let mut files: Vec<(JsString, Vec<&Diagnostic>)> = Vec::new();
    for diagnostic in diagnostics {
        if diagnostic.category() != crate::DiagnosticCategory::Error {
            continue;
        }
        total += 1;
        match &diagnostic.file_name {
            None => global += 1,
            Some(file_name) => match files.iter_mut().find(|(name, _)| name == file_name) {
                Some((_, errors)) => errors.push(diagnostic),
                None => files.push((file_name.clone(), vec![diagnostic])),
            },
        }
    }
    let mut output = JsString::new();
    if total == 0 {
        return Ok(output);
    }
    // Go orders the file names by their UTF-8 bytes.
    files.sort_by(|(left, _), (right, _)| {
        left.to_string_lossy()
            .as_bytes()
            .cmp(right.to_string_lossy().as_bytes())
    });
    let first_file_name = match files.first() {
        Some((file_name, errors)) => pretty_path_for_file_error(file_name, errors, sources)?,
        None => JsString::new(),
    };
    let message = if total == 1 {
        if global > 0 || first_file_name.is_empty() {
            crate::gen::Found_1_error.text.to_owned()
        } else {
            crate::format_message(
                crate::gen::Found_1_error_in_0.text,
                &[first_file_name.to_string_lossy().into_owned()],
            )
        }
    } else {
        match files.len() {
            0 => crate::format_message(crate::gen::Found_0_errors.text, &[total.to_string()]),
            1 => crate::format_message(
                crate::gen::Found_0_errors_in_the_same_file_starting_at_1.text,
                &[
                    total.to_string(),
                    first_file_name.to_string_lossy().into_owned(),
                ],
            ),
            count => crate::format_message(
                crate::gen::Found_0_errors_in_1_files.text,
                &[total.to_string(), count.to_string()],
            ),
        }
    };
    output.push_str(new_line);
    output.push_str(&message);
    output.push_str(new_line);
    output.push_str(new_line);
    if files.len() > 1 {
        // writeTabularErrorsDisplay
        let max_errors = files
            .iter()
            .map(|(_, errors)| errors.len())
            .max()
            .unwrap_or(0);
        let header = crate::gen::Errors_Files.text;
        let left_heading_length = header.split(' ').next().map_or(0, str::len);
        let biggest_count_length = max_errors.to_string().len();
        let left_padding_goal = left_heading_length.max(biggest_count_length);
        let header_padding = biggest_count_length.saturating_sub(left_heading_length);
        output.push_str(&" ".repeat(header_padding));
        output.push_str(header);
        output.push_str(new_line);
        for (file_name, errors) in &files {
            output.push_str(&format!("{:>left_padding_goal$}  ", errors.len()));
            output.push_js(pretty_path_for_file_error(file_name, errors, sources)?.as_js());
            output.push_str(new_line);
        }
        output.push_str(new_line);
    }
    Ok(output)
}

/// tsgo `prettyPathForFileError`: the file name and, in grey, the line of
/// its first error.
fn pretty_path_for_file_error(
    file_name: &JsString,
    errors: &[&Diagnostic],
    sources: &dyn PrettyDiagnosticSources,
) -> Result<JsString, FormatDiagnosticsError> {
    let text = source_text(file_name.as_js(), sources)?;
    let start = errors.first().and_then(|error| error.start).unwrap_or(0);
    let (line, _) = Utf16File::new(text).line_and_character(start)?;
    let mut path = sources.summary_name(file_name.as_js());
    path.push_str(GREY);
    path.push_str(&format!(":{}", line + 1));
    path.push_str(RESET);
    Ok(path)
}

fn category_format(category: crate::DiagnosticCategory) -> &'static str {
    match category {
        crate::DiagnosticCategory::Error => RED,
        crate::DiagnosticCategory::Warning => YELLOW,
        crate::DiagnosticCategory::Suggestion => GREY,
        crate::DiagnosticCategory::Message => BLUE,
    }
}

fn write_with_style_and_reset(output: &mut JsString, text: &str, style: &str) {
    output.push_str(style);
    output.push_str(text);
    output.push_str(RESET);
}

fn source_text<'s>(
    file_name: JsStr<'_>,
    sources: &'s dyn PrettyDiagnosticSources,
) -> Result<&'s str, FormatDiagnosticsError> {
    sources.text(file_name).ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic source text is unavailable for {file_name:?}"
        ))
    })
}

/// tsgo `WriteLocation`: `file:line:column`, the name in cyan and the
/// numbers in yellow.
fn write_location(
    output: &mut JsString,
    file_name: JsStr<'_>,
    start: u32,
    sources: &dyn PrettyDiagnosticSources,
) -> Result<(), FormatDiagnosticsError> {
    let text = source_text(file_name, sources)?;
    let (line, character) = Utf16File::new(text).line_and_character(start)?;
    output.push_str(CYAN);
    output.push_js(sources.location_name(file_name).as_js());
    output.push_str(RESET);
    output.push(':');
    write_with_style_and_reset(output, &(line + 1).to_string(), YELLOW);
    output.push(':');
    write_with_style_and_reset(output, &(character + 1).to_string(), YELLOW);
    Ok(())
}

/// tsgo `WriteFlattenedDiagnosticMessage`: the head, then each chained
/// message on its own line, indented two spaces per level.
fn write_flattened_message(output: &mut JsString, chain: &MessageChain, new_line: &str) {
    fn children(output: &mut JsString, chain: &MessageChain, new_line: &str, level: usize) {
        for child in &chain.next {
            output.push_str(new_line);
            for _ in 0..level {
                output.push_str("  ");
            }
            output.push_js(child.text.as_js());
            children(output, child, new_line, level + 1);
        }
    }
    output.push_js(chain.text.as_js());
    children(output, chain, new_line, 1);
}

/// tsgo `writeCodeSnippet`: each line of the span under a numbered gutter
/// with its squiggles; a span over five lines shows the first two and the
/// last two around an ellipsis. Counts are UTF-16 units, and a zero-length
/// span squiggles the next character.
#[allow(clippy::too_many_arguments)]
fn write_code_snippet(
    output: &mut JsString,
    file_name: JsStr<'_>,
    start: u32,
    length: u32,
    squiggle_color: &str,
    indent: &str,
    sources: &dyn PrettyDiagnosticSources,
    new_line: &str,
) -> Result<(), FormatDiagnosticsError> {
    let file = Utf16File::new(source_text(file_name, sources)?);
    let end = start.checked_add(length).ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic span overflows UTF-16 offsets for {file_name:?}"
        ))
    })?;
    let (first_line, first_character) = file.line_and_character(start)?;
    let (last_line, mut last_character) = file.line_and_character(end)?;
    if length == 0 {
        last_character += 1;
    }
    let last_line_of_file = file.line_and_character(file.len())?.0;
    let has_more_than_five_lines = last_line - first_line >= 4;
    let mut gutter_width = decimal_width(last_line + 1);
    if has_more_than_five_lines {
        gutter_width = gutter_width.max(ELLIPSIS.len());
    }

    let mut line = first_line;
    while line <= last_line {
        output.push_str(new_line);
        if has_more_than_five_lines && first_line + 1 < line && line < last_line - 1 {
            output.push_str(indent);
            output.push_str(GUTTER_STYLE);
            push_padded(output, ELLIPSIS, gutter_width);
            output.push_str(RESET);
            output.push_str(GUTTER_SEPARATOR);
            output.push_str(new_line);
            line = last_line - 1;
        }

        let line_start = file.line_start(line)?;
        let line_end = if line < last_line_of_file {
            file.line_start(line + 1)?
        } else {
            file.len()
        };
        let mut content = file.units[line_start as usize..line_end as usize].to_vec();
        while content.last().copied().is_some_and(is_go_space) {
            content.pop();
        }
        for unit in &mut content {
            if *unit == u16::from(b'\t') {
                *unit = u16::from(b' ');
            }
        }

        output.push_str(indent);
        output.push_str(GUTTER_STYLE);
        push_padded(output, &(line + 1).to_string(), gutter_width);
        output.push_str(RESET);
        output.push_str(GUTTER_SEPARATOR);
        for &unit in &content {
            output.push_code_unit(unit);
        }
        output.push_str(new_line);

        output.push_str(indent);
        output.push_str(GUTTER_STYLE);
        push_padded(output, "", gutter_width);
        output.push_str(RESET);
        output.push_str(GUTTER_SEPARATOR);
        output.push_str(squiggle_color);
        let content_length = content.len() as u32;
        if line == first_line {
            let last_for_line = if line == last_line {
                last_character
            } else {
                content_length
            };
            output.push_str(&" ".repeat(first_character as usize));
            output.push_str(&"~".repeat(last_for_line.saturating_sub(first_character) as usize));
        } else if line == last_line {
            output.push_str(&"~".repeat(last_character as usize));
        } else {
            output.push_str(&"~".repeat(content_length as usize));
        }
        output.push_str(RESET);
        line += 1;
    }
    Ok(())
}

fn required_position(
    position: Option<u32>,
    file_name: JsStr<'_>,
    field: &str,
) -> Result<u32, FormatDiagnosticsError> {
    position.ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic for {file_name:?} is missing its {field}"
        ))
    })
}

/// Go's `unicode.IsSpace`, which `strings.TrimRightFunc` uses on each line.
fn is_go_space(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000d
            | 0x0020
            | 0x0085
            | 0x00a0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
    )
}

fn decimal_width(value: u32) -> usize {
    value.to_string().len()
}

fn push_padded(output: &mut JsString, value: &str, width: usize) {
    for _ in value.len()..width {
        output.push(' ');
    }
    output.push_str(value);
}

#[derive(Debug)]
struct Utf16File {
    units: Vec<u16>,
    line_starts: Vec<u32>,
}

impl Utf16File {
    fn new(text: &str) -> Self {
        let units = text.encode_utf16().collect::<Vec<_>>();
        let mut line_starts = vec![0];
        let mut index = 0usize;
        while index < units.len() {
            match units[index] {
                0x000d => {
                    index += 1;
                    if units.get(index) == Some(&0x000a) {
                        index += 1;
                    }
                    line_starts.push(index as u32);
                }
                0x000a | 0x2028 | 0x2029 => {
                    index += 1;
                    line_starts.push(index as u32);
                }
                _ => index += 1,
            }
        }
        Self { units, line_starts }
    }

    fn len(&self) -> u32 {
        self.units.len() as u32
    }

    fn line_start(&self, line: u32) -> Result<u32, FormatDiagnosticsError> {
        self.line_starts.get(line as usize).copied().ok_or_else(|| {
            FormatDiagnosticsError::new(format!("source line {line} is unavailable"))
        })
    }

    fn line_and_character(&self, position: u32) -> Result<(u32, u32), FormatDiagnosticsError> {
        if position > self.len() {
            return Err(FormatDiagnosticsError::new(format!(
                "UTF-16 position {position} exceeds source length {}",
                self.len()
            )));
        }
        let line = match self.line_starts.binary_search(&position) {
            Ok(line) => line,
            Err(insert_at) => insert_at.saturating_sub(1),
        };
        Ok((line as u32, position - self.line_starts[line]))
    }
}

fn normalize_slashes<'a>(path: impl Into<JsStr<'a>>) -> JsString {
    path.into()
        .code_units()
        .map(|unit| if unit == 0x5c { 0x2f } else { unit })
        .collect()
}

fn join_parts<'a>(parts: impl IntoIterator<Item = JsStr<'a>>) -> JsString {
    let mut result = JsString::new();
    for (index, part) in parts.into_iter().enumerate() {
        if index != 0 {
            result.push('/');
        }
        result.push_js(part);
    }
    result
}

fn relative_file_name<'a, 'b>(
    file_name: impl Into<JsStr<'a>>,
    current_directory: impl Into<JsStr<'b>>,
) -> JsString {
    // Reconstruct the cwd-resolved public name before making it relative.
    // All component comparisons retain the original JavaScript units.
    let current_directory = current_directory.into();
    let file_name = source_file_name(file_name.into(), current_directory);
    let current_directory = absolute_current_directory(current_directory);
    let from = reduced_path(current_directory.as_js());
    let to = reduced_path(file_name.as_js());
    if !ascii_case_equal(from.root.as_js(), to.root.as_js()) {
        return to.to_path();
    }
    let shared = from
        .parts
        .iter()
        .zip(&to.parts)
        .take_while(|(a, b)| a == b)
        .count();
    join_parts(
        (shared..from.parts.len())
            .map(|_| JsStr::from_str(".."))
            .chain(to.parts[shared..].iter().map(JsString::as_js)),
    )
}

fn ascii_case_equal(left: JsStr<'_>, right: JsStr<'_>) -> bool {
    let fold = |unit| {
        if (0x41..=0x5a).contains(&unit) {
            unit + 0x20
        } else {
            unit
        }
    };
    left.code_units().map(fold).eq(right.code_units().map(fold))
}

fn absolute_virtual_path<'a, 'b>(
    file_name: impl Into<JsStr<'a>>,
    current_directory: impl Into<JsStr<'b>>,
) -> JsString {
    // SourceFile.path is reduced independently from SourceFile.fileName.
    reduced_path(source_file_name(file_name.into(), current_directory.into()).as_js()).to_path()
}

fn source_file_name(file_name: JsStr<'_>, current_directory: JsStr<'_>) -> JsString {
    let file_name = normalize_slashes(file_name);
    if file_name.starts_with("/") || has_url_scheme(file_name.as_js()) {
        // Already-absolute public names retain their leading UNC root. A URL
        // is rooted at its scheme (tspath getEncodedRootLength, `scheme://`):
        // tsgo's bundled libraries are `bundled:///libs/<lib>`, which sorts
        // after every `/`-rooted file name.
        return file_name;
    }
    let mut path = absolute_current_directory(current_directory);
    path.push('/');
    path.push_js(file_name.as_js());
    resolve_posix_path(path.as_js())
}

/// Whether `path` starts with a URL scheme (`scheme://`), the root of a URL
/// in tspath's getEncodedRootLength: letters, digits, `+`, `-` and `.` before
/// the first `://`, at least one letter first.
fn has_url_scheme(path: JsStr<'_>) -> bool {
    let bytes = path.as_bytes();
    let Some(end) = bytes.windows(3).position(|window| window == b"://") else {
        return false;
    };
    end > 0
        && bytes[0].is_ascii_alphabetic()
        && bytes[..end]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// `normalizeFileName(path.posix.resolve(cwd))` from program-host.mjs.
/// POSIX reduction precedes backslash normalization, as in the scalar path.
fn absolute_current_directory(current_directory: JsStr<'_>) -> JsString {
    let raw_path = if current_directory.starts_with("/") {
        current_directory.to_owned()
    } else {
        // The process directory comes from the native host, not from a JS
        // identity key. Preserve the existing native display conversion here.
        let process_directory = std::env::current_dir()
            .map(|path| {
                let raw = path.to_string_lossy().into_owned();
                if cfg!(windows) {
                    let normalized = raw.replace('\\', "/");
                    match normalized.find('/') {
                        Some(index) => normalized[index..].to_owned(),
                        None => "/".to_owned(),
                    }
                } else {
                    raw
                }
            })
            .unwrap_or_default();
        let mut path = JsString::from(process_directory);
        path.push('/');
        path.push_js(current_directory);
        path
    };
    normalize_slashes(resolve_posix_path(raw_path.as_js()).as_js())
}

fn resolve_posix_path(path: JsStr<'_>) -> JsString {
    let absolute = path.starts_with("/");
    let mut parts = Vec::new();
    for component in path.split_ascii(b'/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if !parts.is_empty() {
                parts.pop();
            } else if !absolute {
                parts.push(component);
            }
        } else {
            parts.push(component);
        }
    }
    let joined = join_parts(parts);
    if absolute {
        let mut result = JsString::from("/");
        result.push_js(joined.as_js());
        result
    } else if joined.is_empty() {
        JsString::from(".")
    } else {
        joined
    }
}

#[derive(Debug)]
struct ReducedPath {
    root: JsString,
    parts: Vec<JsString>,
}

impl ReducedPath {
    fn to_path(&self) -> JsString {
        let mut result = self.root.clone();
        if !self.parts.is_empty() {
            if !result.is_empty() && !result.ends_with("/") {
                result.push('/');
            }
            result.push_js(join_parts(self.parts.iter().map(JsString::as_js)).as_js());
        }
        result
    }
}

fn reduced_path(path: JsStr<'_>) -> ReducedPath {
    let path = normalize_slashes(path);
    let path = path.as_js();
    let (root, rest) = if let Some(rest) = path.strip_prefix("//") {
        match rest.split_once("/") {
            Some((server, tail)) => {
                let mut root = JsString::from("//");
                root.push_js(server);
                root.push('/');
                (root, tail)
            }
            None => (path.to_owned(), JsStr::from_str("")),
        }
    } else if let Some(rest) = path.strip_prefix("/") {
        (JsString::from("/"), rest)
    } else if path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/') {
        let (root, rest) = path.split_at_byte(3).expect("ASCII drive prefix");
        (root.to_owned(), rest)
    } else {
        (JsString::new(), path)
    };
    let mut parts = Vec::new();
    for component in rest.split_ascii(b'/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            if !parts.is_empty() {
                parts.pop();
                continue;
            }
            if !root.is_empty() {
                continue;
            }
        }
        parts.push(component.to_owned());
    }
    ReducedPath { root, parts }
}

#[cfg(test)]
#[path = "../tests/unit/render/tests.rs"]
mod tests;

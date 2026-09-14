//! Deterministic, color-free port of TypeScript 6.0.3's
//! `formatDiagnosticsWithColorAndContext`.
//!
//! The upstream formatter is UTF-16 based and assumes its caller has
//! already applied `sortAndDeduplicateDiagnostics`.  The public entry
//! point below owns that precondition as well: callers may pass checker
//! diagnostics in any order and receive the exact CLI ordering.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::{
    compare_diagnostics, diagnostics_equal, Diagnostic, JsStr, JsString, MessageChain, RelatedInfo,
    TextSnapshot,
};

const FILE_APPEARS_TO_BE_BINARY: u32 = 1490;
const HALF_INDENT: &str = "  ";
const INDENT: &str = "    ";
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

/// Render diagnostics in the TypeScript 6.0.3
/// `formatDiagnosticsWithColorAndContext` shape, with ANSI styling
/// removed and every formatter-owned newline fixed to LF.
///
/// Unlike the upstream leaf formatter, this entry point performs the
/// CLI's `sortAndDeduplicateDiagnostics` step first.
pub fn format_diagnostics_with_context(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Result<JsString, FormatDiagnosticsError> {
    format_diagnostics_with_context_raw(diagnostics, host)
        .map(|output| normalize_newlines(output.as_js()))
}

/// Render after the CLI sort/deduplicate boundary without rewriting
/// CR, CRLF, U+2028, or U+2029 that originated in diagnostic data.
pub fn format_diagnostics_with_context_raw(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Result<JsString, FormatDiagnosticsError> {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let selected = indices
        .into_iter()
        .map(|index| diagnostics[index].clone())
        .collect::<Vec<_>>();
    format_sorted_diagnostics_with_context_raw(&selected, host)
}

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

/// Render an already sorted/deduplicated diagnostic sequence.
///
/// This is useful after an exact-scope projection: removing entries
/// from a sorted sequence preserves the oracle order and must not
/// cause a second, projection-dependent pairing decision.
pub fn format_sorted_diagnostics_with_context(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Result<JsString, FormatDiagnosticsError> {
    format_sorted_diagnostics_with_context_raw(diagnostics, host)
        .map(|output| normalize_newlines(output.as_js()))
}

/// Render an already sorted/deduplicated sequence while preserving
/// non-LF line separators contained in diagnostic data. ANSI SGR is
/// still removed at the same final-string boundary as the oracle.
pub fn format_sorted_diagnostics_with_context_raw(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Result<JsString, FormatDiagnosticsError> {
    let mut output = JsString::new();
    for diagnostic in diagnostics {
        if let Some(file_name) = diagnostic.file_name.as_ref().map(JsString::as_js) {
            let start = required_position(diagnostic.start, file_name, "start")?;
            output.push_js(format_location(file_name, start, host)?.as_js());
            output.push_str(" - ");
        }
        output.push_str(diagnostic.category().name());
        output.push_str(" TS");
        output.push_str(&diagnostic.code().to_string());
        output.push_str(": ");
        flatten_message_chain(&diagnostic.message, 0, &mut output);

        if let Some(file_name) = diagnostic.file_name.as_ref().map(JsString::as_js) {
            if diagnostic.code() != FILE_APPEARS_TO_BE_BINARY {
                let start = required_position(diagnostic.start, file_name, "start")?;
                let length = required_position(diagnostic.length, file_name, "length")?;
                output.push('\n');
                output.push_str(&format_code_span(file_name, start, length, "", host)?);
            }
        }

        if diagnostic.related_information_present || !diagnostic.related.is_empty() {
            output.push('\n');
            for related in &diagnostic.related {
                format_related_information(related, host, &mut output)?;
            }
        }
        output.push('\n');
    }
    // The oracle contract removes ANSI SGR from the formatter's final
    // string, not merely from formatter-owned color tokens. Preserve
    // that observable ordering for literal escape sequences embedded
    // in file names, source lines, or diagnostic messages as well.
    Ok(strip_ansi_sgr(output.as_js()))
}

fn format_related_information(
    related: &RelatedInfo,
    host: &FormatDiagnosticsHost<'_>,
    output: &mut JsString,
) -> Result<(), FormatDiagnosticsError> {
    if let Some(file_name) = related.file_name.as_ref().map(JsString::as_js) {
        let start = required_position(related.start, file_name, "related start")?;
        let length = required_position(related.length, file_name, "related length")?;
        output.push('\n');
        output.push_str(HALF_INDENT);
        output.push_js(format_location(file_name, start, host)?.as_js());
        output.push_str(&format_code_span(file_name, start, length, INDENT, host)?);
    }
    output.push('\n');
    output.push_str(INDENT);
    flatten_message_chain(&related.message, 0, output);
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

fn format_location(
    file_name: JsStr<'_>,
    start: u32,
    host: &FormatDiagnosticsHost<'_>,
) -> Result<JsString, FormatDiagnosticsError> {
    let text = host.file_text(file_name).ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic source text is unavailable for {file_name:?}"
        ))
    })?;
    let file = Utf16File::new(text);
    let (line, character) = file.line_and_character(start)?;
    let mut location = relative_file_name(file_name, host.current_directory);
    location.push_str(&format!(":{}:{}", line + 1, character + 1));
    Ok(location)
}

fn format_code_span(
    file_name: JsStr<'_>,
    start: u32,
    length: u32,
    indent: &str,
    host: &FormatDiagnosticsHost<'_>,
) -> Result<String, FormatDiagnosticsError> {
    let text = host.file_text(file_name).ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic source text is unavailable for {file_name:?}"
        ))
    })?;
    let file = Utf16File::new(text);
    let end = start.checked_add(length).ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic span overflows UTF-16 offsets for {file_name:?}"
        ))
    })?;
    let (first_line, first_character) = file.line_and_character(start)?;
    let (last_line, last_character) = file.line_and_character(end)?;
    let last_line_in_file = file.line_and_character(file.len())?.0;
    let has_more_than_five_lines = last_line.saturating_sub(first_line) >= 4;
    let mut gutter_width = decimal_width(last_line + 1);
    if has_more_than_five_lines {
        gutter_width = gutter_width.max(ELLIPSIS.len());
    }

    let mut context = String::new();
    let mut line = first_line;
    while line <= last_line {
        context.push('\n');
        if has_more_than_five_lines && first_line + 1 < line && line < last_line - 1 {
            context.push_str(indent);
            push_padded(&mut context, ELLIPSIS, gutter_width);
            context.push(' ');
            context.push('\n');
            line = last_line - 1;
        }

        let line_start = file.line_start(line)?;
        let line_end = if line < last_line_in_file {
            file.line_start(line + 1)?
        } else {
            file.len()
        };
        let mut line_content = file.units[line_start as usize..line_end as usize].to_vec();
        trim_end_js(&mut line_content);
        for unit in &mut line_content {
            if *unit == b'\t' as u16 {
                *unit = b' ' as u16;
            }
        }

        context.push_str(indent);
        push_padded(&mut context, &(line + 1).to_string(), gutter_width);
        context.push(' ');
        push_utf16(&mut context, &line_content);
        context.push('\n');
        context.push_str(indent);
        push_padded(&mut context, "", gutter_width);
        context.push(' ');

        if line == first_line {
            let end_character = if line == last_line {
                last_character as usize
            } else {
                line_content.len()
            };
            // JavaScript String#slice clamps both offsets. This is
            // observable for zero-width diagnostics in trailing
            // whitespace and exactly at a line break.
            let first_character = (first_character as usize).min(line_content.len());
            let end_character = end_character.min(line_content.len());
            push_non_whitespace_as_spaces(&mut context, &line_content[..first_character]);
            if first_character < end_character {
                push_tildes(&mut context, &line_content[first_character..end_character]);
            }
        } else if line == last_line {
            let last_character = (last_character as usize).min(line_content.len());
            push_tildes(&mut context, &line_content[..last_character]);
        } else {
            push_tildes(&mut context, &line_content);
        }
        line += 1;
    }
    Ok(context)
}

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut JsString) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str(HALF_INDENT);
        }
    }
    output.push_js(chain.text.as_js());
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn push_non_whitespace_as_spaces(output: &mut String, units: &[u16]) {
    for &unit in units {
        if is_js_whitespace(unit) {
            push_utf16(output, &[unit]);
        } else {
            output.push(' ');
        }
    }
}

fn push_tildes(output: &mut String, units: &[u16]) {
    // JavaScript's non-Unicode `/./g` consumes one UTF-16 code unit at
    // a time.  Astral characters therefore occupy two squiggles.
    for _ in units {
        output.push('~');
    }
}

fn trim_end_js(units: &mut Vec<u16>) {
    while units.last().copied().is_some_and(is_js_whitespace) {
        units.pop();
    }
}

fn is_js_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000d
            | 0x0020
            | 0x00a0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
            | 0xfeff
    )
}

fn push_utf16(output: &mut String, units: &[u16]) {
    output.push_str(&String::from_utf16_lossy(units));
}

fn decimal_width(value: u32) -> usize {
    value.to_string().len()
}

fn push_padded(output: &mut String, value: &str, width: usize) {
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
    if file_name.starts_with("/") {
        // Already-absolute public names retain their leading UNC root.
        return file_name;
    }
    let mut path = absolute_current_directory(current_directory);
    path.push('/');
    path.push_js(file_name.as_js());
    resolve_posix_path(path.as_js())
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

fn normalize_newlines(text: JsStr<'_>) -> JsString {
    let mut output = JsString::new();
    let mut units = text.code_units().peekable();
    while let Some(unit) = units.next() {
        if unit == 13 {
            if units.peek() == Some(&10) {
                units.next();
            }
            output.push_code_unit(10);
        } else {
            output.push_code_unit(unit);
        }
    }
    output
}

fn strip_ansi_sgr(text: JsStr<'_>) -> JsString {
    let units = text.to_utf16();
    let mut output = JsString::new();
    let mut index = 0;
    while index < units.len() {
        if units[index] == 0x1B && units.get(index + 1) == Some(&0x5B) {
            let mut end = index + 2;
            while units
                .get(end)
                .is_some_and(|unit| (0x30..=0x39).contains(unit) || *unit == 0x3B)
            {
                end += 1;
            }
            if units.get(end) == Some(&0x6D) {
                index = end + 1;
                continue;
            }
        }
        // Removing SGR can bring a lead and trail surrogate together. The
        // canonical owner joins them, just as JavaScript string replacement.
        output.push_code_unit(units[index]);
        index += 1;
    }
    output
}

#[cfg(test)]
#[path = "../tests/unit/render/tests.rs"]
mod tests;

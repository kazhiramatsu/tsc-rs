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

use crate::{compare_diagnostics, diagnostics_equal, Diagnostic, MessageChain, RelatedInfo};

const FILE_APPEARS_TO_BE_BINARY: u32 = 1490;
const HALF_INDENT: &str = "  ";
const INDENT: &str = "    ";
const ELLIPSIS: &str = "...";

#[derive(Clone, Copy, Debug)]
pub struct FormatDiagnosticsHost<'a> {
    current_directory: &'a str,
    file_texts: &'a BTreeMap<String, String>,
}

impl<'a> FormatDiagnosticsHost<'a> {
    pub fn new(current_directory: &'a str, file_texts: &'a BTreeMap<String, String>) -> Self {
        Self {
            current_directory,
            file_texts,
        }
    }

    fn file_text(&self, file_name: &str) -> Option<&'a str> {
        if let Some(text) = self.file_texts.get(file_name) {
            return Some(text);
        }
        let normalized = normalize_slashes(file_name);
        self.file_texts
            .iter()
            .find(|(candidate, _)| normalize_slashes(candidate) == normalized)
            .map(|(_, text)| text.as_str())
    }
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
) -> Result<String, FormatDiagnosticsError> {
    format_diagnostics_with_context_raw(diagnostics, host).map(|output| normalize_newlines(&output))
}

/// Render after the CLI sort/deduplicate boundary without rewriting
/// CR, CRLF, U+2028, or U+2029 that originated in diagnostic data.
pub fn format_diagnostics_with_context_raw(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Result<String, FormatDiagnosticsError> {
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
                .as_deref()
                .map(|name| absolute_virtual_path(name, host.current_directory));
            for related in &mut comparison.related {
                related.file_name = related
                    .file_name
                    .as_deref()
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
) -> Result<String, FormatDiagnosticsError> {
    format_sorted_diagnostics_with_context_raw(diagnostics, host)
        .map(|output| normalize_newlines(&output))
}

/// Render an already sorted/deduplicated sequence while preserving
/// non-LF line separators contained in diagnostic data. ANSI SGR is
/// still removed at the same final-string boundary as the oracle.
pub fn format_sorted_diagnostics_with_context_raw(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
) -> Result<String, FormatDiagnosticsError> {
    let mut output = String::new();
    for diagnostic in diagnostics {
        if let Some(file_name) = diagnostic.file_name.as_deref() {
            let start = required_position(diagnostic.start, file_name, "start")?;
            output.push_str(&format_location(file_name, start, host)?);
            output.push_str(" - ");
        }
        output.push_str(diagnostic.category().name());
        output.push_str(" TS");
        output.push_str(&diagnostic.code().to_string());
        output.push_str(": ");
        flatten_message_chain(&diagnostic.message, 0, &mut output);

        if let Some(file_name) = diagnostic.file_name.as_deref() {
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
    Ok(strip_ansi_sgr(&output))
}

fn format_related_information(
    related: &RelatedInfo,
    host: &FormatDiagnosticsHost<'_>,
    output: &mut String,
) -> Result<(), FormatDiagnosticsError> {
    if let Some(file_name) = related.file_name.as_deref() {
        let start = required_position(related.start, file_name, "related start")?;
        let length = required_position(related.length, file_name, "related length")?;
        output.push('\n');
        output.push_str(HALF_INDENT);
        output.push_str(&format_location(file_name, start, host)?);
        output.push_str(&format_code_span(file_name, start, length, INDENT, host)?);
    }
    output.push('\n');
    output.push_str(INDENT);
    flatten_message_chain(&related.message, 0, output);
    Ok(())
}

fn required_position(
    position: Option<u32>,
    file_name: &str,
    field: &str,
) -> Result<u32, FormatDiagnosticsError> {
    position.ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic for {file_name:?} is missing its {field}"
        ))
    })
}

fn format_location(
    file_name: &str,
    start: u32,
    host: &FormatDiagnosticsHost<'_>,
) -> Result<String, FormatDiagnosticsError> {
    let text = host.file_text(file_name).ok_or_else(|| {
        FormatDiagnosticsError::new(format!(
            "diagnostic source text is unavailable for {file_name:?}"
        ))
    })?;
    let file = Utf16File::new(text);
    let (line, character) = file.line_and_character(start)?;
    Ok(format!(
        "{}:{}:{}",
        relative_file_name(file_name, host.current_directory),
        line + 1,
        character + 1
    ))
}

fn format_code_span(
    file_name: &str,
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

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str(HALF_INDENT);
        }
    }
    output.push_str(&chain.text);
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

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn relative_file_name(file_name: &str, current_directory: &str) -> String {
    // Oracle records persist the public program name, while tsc formats
    // the SourceFile's cwd-resolved absolute name. Reconstruct that host
    // name first so `./a.ts` prints as `a.ts` and `x/../a.ts` is reduced
    // exactly like program-host.mjs before convertToRelativePath runs.
    let file_name = source_file_name(file_name, current_directory);
    let current_directory = absolute_current_directory(current_directory);
    let from = reduced_path(&current_directory);
    let to = reduced_path(&file_name);
    if !from.root.eq_ignore_ascii_case(&to.root) {
        return to.to_path();
    }

    let mut shared = 0usize;
    while shared < from.parts.len()
        && shared < to.parts.len()
        && from.parts[shared] == to.parts[shared]
    {
        shared += 1;
    }
    let mut relative = Vec::new();
    relative.extend((shared..from.parts.len()).map(|_| "..".to_owned()));
    relative.extend(to.parts[shared..].iter().cloned());
    relative.join("/")
}

fn absolute_virtual_path(file_name: &str, current_directory: &str) -> String {
    // SourceFile.fileName preserves an already-absolute public name,
    // but SourceFile.path (the tsc diagnostic sort key) is reduced by
    // `toPath`. Keep that comparison twin separate from display-name
    // reconstruction.
    reduced_path(&source_file_name(file_name, current_directory)).to_path()
}

fn source_file_name(file_name: &str, current_directory: &str) -> String {
    let file_name = normalize_slashes(file_name);
    if file_name.starts_with('/') {
        // absoluteProgramFileName returns an already-absolute public
        // name verbatim after slash normalization. In particular, a
        // leading `//` remains a TypeScript UNC root; only cwd itself
        // passes through path.posix.resolve.
        return file_name;
    }
    let current_directory = absolute_current_directory(current_directory);
    resolve_posix_path(&format!("{current_directory}/{file_name}"))
}

/// `normalizeFileName(path.posix.resolve(cwd))` from program-host.mjs.
///
/// POSIX resolution intentionally happens before backslashes are
/// normalized. Thus a raw `C:/x` cwd is a relative POSIX path and a
/// raw `\x` component is not treated as a separator until after `..`
/// processing.
fn absolute_current_directory(current_directory: &str) -> String {
    let raw_path = if current_directory.starts_with('/') {
        current_directory.to_owned()
    } else {
        let process_directory = std::env::current_dir()
            .map(|path| {
                let raw = path.to_string_lossy().into_owned();
                if cfg!(windows) {
                    let normalized = normalize_slashes(&raw);
                    match normalized.find('/') {
                        Some(root) => normalized[root..].to_owned(),
                        None => normalized,
                    }
                } else {
                    raw
                }
            })
            .unwrap_or_default();
        format!("{process_directory}/{current_directory}")
    };
    resolve_posix_path(&raw_path).replace('\\', "/")
}

fn resolve_posix_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." if !parts.is_empty() => {
                parts.pop();
            }
            ".." if !absolute => parts.push(component),
            ".." => {}
            _ => parts.push(component),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

#[derive(Debug)]
struct ReducedPath {
    root: String,
    parts: Vec<String>,
}

impl ReducedPath {
    fn to_path(&self) -> String {
        if self.parts.is_empty() {
            return self.root.clone();
        }
        let suffix = self.parts.join("/");
        if self.root.is_empty() || self.root.ends_with('/') {
            format!("{}{suffix}", self.root)
        } else {
            format!("{}/{suffix}", self.root)
        }
    }
}

fn reduced_path(path: &str) -> ReducedPath {
    let path = normalize_slashes(path);
    let (root, rest) = if let Some(rest) = path.strip_prefix("//") {
        match rest.find('/') {
            Some(server_end) => (
                format!("//{}/", &rest[..server_end]),
                &rest[server_end + 1..],
            ),
            None => (format!("//{rest}"), ""),
        }
    } else if let Some(rest) = path.strip_prefix('/') {
        ("/".to_owned(), rest)
    } else if path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/') {
        (path[..3].to_owned(), &path[3..])
    } else {
        (String::new(), path.as_str())
    };
    let mut parts = Vec::new();
    for component in rest.split('/') {
        match component {
            "" | "." => {}
            ".." if !parts.is_empty() => {
                parts.pop();
            }
            ".." if !root.is_empty() => {}
            _ => parts.push(component.to_owned()),
        }
    }
    ReducedPath { root, parts }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn strip_ansi_sgr(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            let mut end = index + 2;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b';')
            {
                end += 1;
            }
            if bytes.get(end) == Some(&b'm') {
                index = end + 1;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).expect("removing ASCII SGR sequences preserves UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticCategory, RelatedInfo};

    fn chain(code: u32, category: DiagnosticCategory, text: &str) -> MessageChain {
        MessageChain {
            code,
            category,
            text: text.to_owned(),
            next_present: false,
            next: Vec::new(),
        }
    }

    #[test]
    fn renders_tsc_context_shape_with_utf16_tabs_chains_and_related() {
        let mut files = BTreeMap::new();
        files.insert(
            "/workspace/src/main.ts".to_owned(),
            "const\tface = \"😀\";\r\nconst value = 1;\r\n".to_owned(),
        );
        files.insert(
            "/workspace/src/origin.ts".to_owned(),
            "export const origin = 1;\n".to_owned(),
        );

        let mut message = chain(2322, DiagnosticCategory::Error, "Head");
        message.next_present = true;
        message.next = vec![chain(2322, DiagnosticCategory::Error, "Child")];
        let mut diagnostic = Diagnostic::new(
            Some("/workspace/src/main.ts".to_owned()),
            Some(6),
            Some(6),
            message,
        );
        diagnostic.related.push(RelatedInfo {
            file_name: Some("/workspace/src/origin.ts".to_owned()),
            start: Some(13),
            length: Some(6),
            message: chain(2728, DiagnosticCategory::Message, "Origin"),
        });

        let host = FormatDiagnosticsHost::new("/workspace", &files);
        assert_eq!(
            format_diagnostics_with_context(&[diagnostic], &host).unwrap(),
            concat!(
                "src/main.ts:1:7 - error TS2322: Head\n",
                "  Child\n",
                "\n",
                "1 const face = \"😀\";\n",
                "        ~~~~~~\n",
                "\n",
                "  src/origin.ts:1:14\n",
                "    1 export const origin = 1;\n",
                "                   ~~~~~~\n",
                "    Origin\n",
            )
        );
    }

    #[test]
    fn owns_order_dedupe_multiline_fileless_and_suggestion_rendering() {
        let mut files = BTreeMap::new();
        files.insert("multi.ts".to_owned(), "a\nb\nc\nd\ne\nf\n".to_owned());
        let fileless = Diagnostic::new(
            None,
            None,
            None,
            chain(999, DiagnosticCategory::Message, "global"),
        );
        let suggestion = Diagnostic::new(
            Some("multi.ts".to_owned()),
            Some(0),
            Some(10),
            chain(80001, DiagnosticCategory::Suggestion, "hint"),
        );
        let host = FormatDiagnosticsHost::new("/", &files);
        let output =
            format_diagnostics_with_context(&[suggestion.clone(), fileless, suggestion], &host)
                .unwrap();
        assert_eq!(
            output,
            concat!(
                "message TS999: global\n",
                "multi.ts:1:1 - suggestion TS80001: hint\n",
                "\n",
                "  1 a\n",
                "    ~\n",
                "  2 b\n",
                "    ~\n",
                "... \n",
                "  5 e\n",
                "    ~\n",
                "  6 f\n",
                "    \n",
            )
        );
    }

    #[test]
    fn present_empty_related_information_emits_the_tsc_blank_line() {
        let files = BTreeMap::new();
        let mut present_empty = Diagnostic::new(
            None,
            None,
            None,
            chain(1, DiagnosticCategory::Error, "first"),
        );
        present_empty.related_information_present = true;
        let absent = Diagnostic::new(
            None,
            None,
            None,
            chain(2, DiagnosticCategory::Error, "second"),
        );
        let host = FormatDiagnosticsHost::new("/", &files);

        assert_eq!(
            format_sorted_diagnostics_with_context(&[present_empty, absent], &host).unwrap(),
            "error TS1: first\n\nerror TS2: second\n"
        );
    }

    #[test]
    fn raw_formatter_preserves_message_newlines() {
        let files = BTreeMap::new();
        let diagnostic = Diagnostic::new(
            None,
            None,
            None,
            chain(1, DiagnosticCategory::Error, "head\rbody\r\ntail"),
        );
        let host = FormatDiagnosticsHost::new("/", &files);

        assert_eq!(
            format_sorted_diagnostics_with_context_raw(std::slice::from_ref(&diagnostic), &host,)
                .unwrap(),
            "error TS1: head\rbody\r\ntail\n"
        );
        assert_eq!(
            format_sorted_diagnostics_with_context(&[diagnostic], &host).unwrap(),
            "error TS1: head\nbody\ntail\n"
        );
    }

    #[test]
    fn cwd_aware_selection_returns_the_retained_input_occurrence() {
        let files = BTreeMap::new();
        let host = FormatDiagnosticsHost::new("/work", &files);
        let first = Diagnostic::new(
            Some("src/../a.ts".to_owned()),
            Some(0),
            Some(1),
            chain(1, DiagnosticCategory::Error, "same"),
        );
        let second = Diagnostic::new(
            Some("a.ts".to_owned()),
            Some(0),
            Some(1),
            chain(1, DiagnosticCategory::Error, "same"),
        );

        assert_eq!(
            sort_and_dedupe_diagnostic_indices_with_context(&[first, second], &host,),
            [0]
        );
    }

    #[test]
    fn sorts_by_virtual_absolute_path_and_clamps_trimmed_line_spans() {
        assert_eq!(
            relative_file_name("//server/share/a.ts", "/work"),
            "//server/share/a.ts"
        );
        assert_eq!(
            absolute_virtual_path("/z/../a.ts", "/work"),
            "/a.ts",
            "the sort twin is SourceFile.path, not the raw SourceFile.fileName"
        );
        assert_eq!(
            relative_file_name("/z/../a.ts", "/work"),
            "../a.ts",
            "display conversion reduces the raw absolute SourceFile.fileName"
        );
        let mut files = BTreeMap::new();
        files.insert("../z.ts".to_owned(), "x   \n".to_owned());
        files.insert("./nested/../dot.ts".to_owned(), "d\n".to_owned());
        files.insert("a.ts".to_owned(), "y\n".to_owned());
        let z = Diagnostic::new(
            Some("../z.ts".to_owned()),
            Some(4),
            Some(0),
            chain(2, DiagnosticCategory::Error, "z"),
        );
        let a = Diagnostic::new(
            Some("a.ts".to_owned()),
            Some(0),
            Some(1),
            chain(1, DiagnosticCategory::Error, "a"),
        );
        let dot = Diagnostic::new(
            Some("./nested/../dot.ts".to_owned()),
            Some(0),
            Some(1),
            chain(3, DiagnosticCategory::Error, "dot"),
        );
        let host = FormatDiagnosticsHost::new("/work", &files);
        assert_eq!(
            format_diagnostics_with_context(&[z, dot, a], &host).unwrap(),
            concat!(
                "a.ts:1:1 - error TS1: a\n",
                "\n",
                "1 y\n",
                "  ~\n",
                "dot.ts:1:1 - error TS3: dot\n",
                "\n",
                "1 d\n",
                "  ~\n",
                "../z.ts:1:5 - error TS2: z\n",
                "\n",
                "1 x\n",
                "   \n",
            )
        );
    }

    #[test]
    fn strips_input_sgr_after_rendering_like_the_oracle_adapter() {
        let mut files = BTreeMap::new();
        files.insert("./\u{1b}[31ma.ts".to_owned(), "x\u{1b}[32my\n".to_owned());
        let diagnostic = Diagnostic::new(
            Some("./\u{1b}[31ma.ts".to_owned()),
            Some(0),
            Some(7),
            chain(4, DiagnosticCategory::Error, "bad \u{1b}[33mcolor"),
        );
        let host = FormatDiagnosticsHost::new("/work", &files);

        assert_eq!(
            format_diagnostics_with_context(&[diagnostic], &host).unwrap(),
            concat!(
                "a.ts:1:1 - error TS4: bad color\n",
                "\n",
                "1 xy\n",
                "  ~~~~~~~\n",
            )
        );
    }
}

//! The native runner's error baseline (`GetErrorBaseline` in
//! `tsc/internal/testutil/tsbaseline/error_baseline.go`) rendered from tsc-rs
//! diagnostics, so a configuration can be compared with its `.errors.txt`
//! byte for byte, as the upstream test does. A `@pretty` baseline opens with
//! the pretty diagnostics (`FormatDiagnosticsWithColorAndContext`) and ends
//! with the error summary (`WriteErrorSummaryText`).
//!
//! The runner works on UTF-8 byte offsets and counts squiggles in characters;
//! tsc-rs positions are UTF-16 offsets and are converted through the file
//! text. Diagnostics keep tsc-rs's order (`sortAndDeduplicateDiagnostics`).
//! The native order (`ast.CompareDiagnostics`) agrees with it except between
//! diagnostics with equal file, span and code, which it orders by message key
//! and arguments rather than text.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use tsc_diagnostics::{
    format_diagnostics_with_color_and_context, write_error_summary_text, Diagnostic, JsStr,
    JsString, MessageChain, PositionIndex, PrettyDiagnosticSources, RelatedInfo,
};

const NEW_LINE: &str = "\r\n";

/// One file the runner passes to the baseline: `tsConfigFiles`,
/// `toBeCompiled`, then `otherFiles`, each named by its normalized absolute
/// unit path.
pub(super) struct InputFile<'a> {
    pub name: &'a str,
    pub content: &'a str,
}

/// The baseline text, or `None` when there are no diagnostics (the runner
/// then writes no file). `library` holds the `/.lib/` test-library files the
/// program loaded: they locate rows and related information (the native
/// runner prints `react18/react18.d.ts:478:9`) but list no section.
pub(super) fn render(
    diagnostics: &[Diagnostic],
    files: &[InputFile<'_>],
    library: &[InputFile<'_>],
    pretty: bool,
) -> Option<String> {
    if diagnostics.is_empty() {
        return None;
    }
    let indexes: Indexes<'_> = files
        .iter()
        .chain(library)
        .map(|file| (file.name, PositionIndex::new_static(file.content)))
        .collect();
    let sources = RunnerSources {
        texts: files
            .iter()
            .chain(library)
            .map(|file| (file.name, file.content))
            .collect(),
        indexes: &indexes,
    };

    let summary = if pretty {
        let top = format_diagnostics_with_color_and_context(diagnostics, &sources, NEW_LINE)
            .map(|text| text.to_string_lossy().into_owned())
            .unwrap_or_else(|error| format!("<pretty diagnostics failed: {error}>"));
        mask_summary_library_locations(&remove_test_path_prefixes(&top))
    } else {
        plain_summary(diagnostics, &indexes)
    };
    let mut baseline = format!(
        "{summary}{NEW_LINE}{NEW_LINE}{}",
        file_sections(diagnostics, files, &indexes)
    );
    if pretty {
        let error_summary = write_error_summary_text(diagnostics, &sources, NEW_LINE)
            .map(|text| text.to_string_lossy().into_owned())
            .unwrap_or_else(|error| format!("<error summary failed: {error}>"));
        baseline.push_str(&remove_test_path_prefixes(&error_summary));
    }
    Some(baseline)
}

/// `WriteFormatDiagnostics`: each diagnostic on its own line.
fn plain_summary(diagnostics: &[Diagnostic], indexes: &Indexes<'_>) -> String {
    let mut summary = String::new();
    for diagnostic in diagnostics {
        if let Some(file_name) = &diagnostic.file_name {
            let file_name = file_name.to_string_lossy();
            // A library file has no fixture text; its location is masked.
            let (line, column) = position(indexes, &file_name, diagnostic.start).unwrap_or((1, 1));
            let _ = write!(
                summary,
                "{}({line},{column}): ",
                display_name(&file_name, indexes)
            );
        }
        let _ = write!(
            summary,
            "{} TS{}: ",
            diagnostic.category().name(),
            diagnostic.code()
        );
        flatten(&diagnostic.message, &mut summary);
        summary.push_str(NEW_LINE);
    }
    mask_summary_library_locations(&remove_test_path_prefixes(&summary))
}

/// The baseline text after the summary of `render`.
fn file_sections(
    diagnostics: &[Diagnostic],
    files: &[InputFile<'_>],
    indexes: &Indexes<'_>,
) -> String {
    let mut lines = Lines::default();
    let error_text = |lines: &mut Lines, diagnostic: &Diagnostic| {
        let mut message = String::new();
        flatten(&diagnostic.message, &mut message);
        for line in remove_test_path_prefixes(&message).split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if !line.is_empty() {
                lines.push(&format!(
                    "!!! {} TS{}: {line}",
                    diagnostic.category().name(),
                    diagnostic.code()
                ));
            }
        }
        for related in &diagnostic.related {
            lines.push(&related_line(related, indexes));
        }
    };
    for diagnostic in diagnostics.iter().filter(|d| d.file_name.is_none()) {
        error_text(&mut lines, diagnostic);
    }
    for file in files {
        let unit = remove_test_path_prefixes(file.name);
        let errors: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.file_name.as_ref().is_some_and(|name| {
                    remove_test_path_prefixes(&name.to_string_lossy()).eq_ignore_ascii_case(&unit)
                })
            })
            .collect();
        lines.push(&format!("==== {unit} ({} errors) ====", errors.len()));
        let index = &indexes[file.name];
        let spans: Vec<(isize, isize)> = errors
            .iter()
            .map(|diagnostic| {
                let start = diagnostic.start.unwrap_or(0);
                let end = start + diagnostic.length.unwrap_or(0);
                let byte = |offset: u32| {
                    index
                        .utf16_to_byte(offset)
                        .map_or(file.content.len() as isize, |byte| byte as isize)
                };
                (byte(start), byte(end))
            })
            .collect();
        let line_starts = ecma_line_starts(file.content);
        let pieces = split_lines(file.content);
        let last = pieces.len() - 1;
        for (line_index, line) in pieces.iter().enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let this_start = line_starts[line_index] as isize;
            let next_start = if line_index == last {
                file.content.len() as isize
            } else {
                line_starts[line_index + 1] as isize
            };
            lines.push(&format!("    {line}"));
            for (diagnostic, &(start, end)) in errors.iter().zip(&spans) {
                if end < this_start || (start >= next_start && line_index != last) {
                    continue;
                }
                let length = (end - start) - (this_start - start).max(0);
                let squiggle_start = ((start - this_start).max(0) as usize).min(line.len());
                let squiggle_end = squiggle_start
                    .max((squiggle_start as isize + length).min(line.len() as isize) as usize);
                let lead: String = String::from_utf8_lossy(&line.as_bytes()[..squiggle_start])
                    .chars()
                    .map(|c| {
                        if matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ') {
                            c
                        } else {
                            ' '
                        }
                    })
                    .collect();
                let squiggles = character_count(&line.as_bytes()[squiggle_start..squiggle_end]);
                lines.push(&format!("    {lead}{}", "~".repeat(squiggles)));
                if line_index == last || next_start > end {
                    error_text(&mut lines, diagnostic);
                }
            }
        }
    }
    lines.text
}

/// The fixture and library files the pretty writer reads: names as the
/// runner prints them (test path prefixes are removed from the whole text
/// afterwards) and texts.
struct RunnerSources<'a> {
    texts: BTreeMap<&'a str, &'a str>,
    indexes: &'a Indexes<'a>,
}

impl PrettyDiagnosticSources for RunnerSources<'_> {
    fn location_name(&self, file_name: JsStr<'_>) -> JsString {
        JsString::from(display_name(&file_name.to_string_lossy(), self.indexes))
    }

    fn summary_name(&self, file_name: JsStr<'_>) -> JsString {
        JsString::from(display_name(&file_name.to_string_lossy(), self.indexes))
    }

    fn text(&self, file_name: JsStr<'_>) -> Option<&str> {
        self.texts
            .get(file_name.to_string_lossy().as_ref())
            .copied()
    }
}

/// The lines after the summary: the first has no line break before it.
#[derive(Default)]
struct Lines {
    text: String,
    started: bool,
}

impl Lines {
    fn push(&mut self, line: &str) {
        if self.started {
            self.text.push_str(NEW_LINE);
        }
        self.started = true;
        self.text.push_str(line);
    }
}

/// `FlattenDiagnosticMessage`: the head, then each chained message on its
/// own line, indented two spaces per level.
fn flatten(chain: &MessageChain, output: &mut String) {
    fn children(chain: &MessageChain, output: &mut String, level: usize) {
        for child in &chain.next {
            output.push_str(NEW_LINE);
            output.push_str(&"  ".repeat(level));
            output.push_str(&child.text.to_string_lossy());
            children(child, output, level + 1);
        }
    }
    output.push_str(&chain.text.to_string_lossy());
    children(chain, output, 1);
}

/// Each fixture file's position index, by its normalized absolute name.
type Indexes<'a> = BTreeMap<&'a str, PositionIndex>;

/// The 1-based line and UTF-16 column of `start` in a fixture file; `None`
/// for a library file, whose location the runner masks.
fn position(indexes: &Indexes<'_>, file_name: &str, start: Option<u32>) -> Option<(u32, u32)> {
    indexes.get(file_name).and_then(|index| {
        index
            .line_and_character_utf16(start.unwrap_or(0))
            .map(|position| (position.line + 1, position.character + 1))
    })
}

fn related_line(related: &RelatedInfo, indexes: &Indexes<'_>) -> String {
    let location = match &related.file_name {
        Some(file_name) => {
            let file_name = file_name.to_string_lossy();
            let (line, column) = position(indexes, &file_name, related.start).unwrap_or((1, 1));
            let location = remove_test_path_prefixes(&format!(
                " {}:{line}:{column}",
                display_name(&file_name, indexes)
            ));
            if is_default_library_file(&file_name) {
                mask_related_library_location(&location)
            } else {
                location
            }
        }
        None => String::new(),
    };
    let mut message = String::new();
    flatten(&related.message, &mut message);
    format!(
        "!!! related TS{}{location}: {message}",
        related.message.code
    )
}

/// The name the runner prints: fixture files by their path, library files
/// by their bundled name (`bundled:///libs/<file>`, shortened later).
fn display_name(file_name: &str, indexes: &Indexes<'_>) -> String {
    if !indexes.contains_key(file_name) && is_default_library_file(file_name) {
        return base_name(file_name).to_owned();
    }
    file_name.to_owned()
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `isDefaultLibraryFile`.
fn is_default_library_file(path: &str) -> bool {
    let name = base_name(path);
    name.starts_with("lib.") && name.ends_with(".d.ts")
}

/// `removeTestPathPrefixes` without the trailing separator.
pub(super) fn remove_test_path_prefixes(text: &str) -> String {
    const PREFIXES: [(&str, &str); 7] = [
        ("/.ts/", ""),
        ("/.lib/", ""),
        ("/.src/", ""),
        ("bundled:///libs/", ""),
        ("file:///./ts/", "file:///"),
        ("file:///./lib/", "file:///"),
        ("file:///./src/", "file:///"),
    ];
    // strings.NewReplacer: at each position the first matching pattern in
    // argument order wins, and replaced text is not scanned again.
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while !rest.is_empty() {
        for (pattern, replacement) in PREFIXES {
            if let Some(after) = rest.strip_prefix(pattern) {
                output.push_str(replacement);
                rest = after;
                continue 'scan;
            }
        }
        let character = rest.chars().next().expect("non-empty rest");
        output.push(character);
        rest = &rest[character.len_utf8()..];
    }
    output
}

/// `(?im)^(lib.*\.d\.ts)\(\d+,\d+\)` → `$1(--,--)` on every summary line.
fn mask_summary_library_locations(summary: &str) -> String {
    summary
        .split('\n')
        .map(|line| {
            let Some(head) = line.get(..3) else {
                return line.to_owned();
            };
            if !head.eq_ignore_ascii_case("lib") {
                return line.to_owned();
            }
            // Greedy `.*`: the last `.d.ts(<digits>,<digits>)` on the line.
            let lower = line.to_ascii_lowercase();
            let mut candidate = lower.rfind(".d.ts(");
            while let Some(at) = candidate {
                let open = at + ".d.ts".len();
                if let Some(close) = digits_pair_end(&line[open + 1..], ',', ')') {
                    return format!("{}(--,--){}", &line[..open], &line[open + 1 + close..]);
                }
                candidate = lower[..at].rfind(".d.ts(");
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `(?i)(lib.*\.d\.ts):\d+:\d+` → `$1:--:--` on a related location.
fn mask_related_library_location(location: &str) -> String {
    let lower = location.to_ascii_lowercase();
    let Some(lib) = lower.find("lib") else {
        return location.to_owned();
    };
    let mut candidate = lower.rfind(".d.ts:");
    while let Some(at) = candidate {
        if at < lib {
            break;
        }
        let colon = at + ".d.ts".len();
        if let Some(end) = digits_pair_end(&location[colon + 1..], ':', '\0') {
            return format!(
                "{}:--:--{}",
                &location[..colon],
                &location[colon + 1 + end..]
            );
        }
        candidate = lower[..at].rfind(".d.ts:");
    }
    location.to_owned()
}

/// The length of `<digits><separator><digits>[terminator]` at the start of
/// `text` (a `'\0'` terminator means none), or `None`.
fn digits_pair_end(text: &str, separator: char, terminator: char) -> Option<usize> {
    let first = text.bytes().take_while(u8::is_ascii_digit).count();
    if first == 0 || !text[first..].starts_with(separator) {
        return None;
    }
    let second_start = first + separator.len_utf8();
    let second = text[second_start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if second == 0 {
        return None;
    }
    let end = second_start + second;
    if terminator == '\0' {
        return Some(end);
    }
    text[end..]
        .starts_with(terminator)
        .then_some(end + terminator.len_utf8())
}

/// `lineDelimiter.Split(content, -1)` with `lineDelimiter = \r?\n`.
fn split_lines(content: &str) -> Vec<&str> {
    let mut pieces: Vec<&str> = content.split('\n').collect();
    let last = pieces.len() - 1;
    for piece in &mut pieces[..last] {
        *piece = piece.strip_suffix('\r').unwrap_or(piece);
    }
    pieces
}

/// `core.ComputeECMALineStarts`: byte offsets after CR, LF, CRLF, U+2028 and
/// U+2029.
fn ecma_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let bytes = content.as_bytes();
    let mut chars = content.char_indices().peekable();
    while let Some((at, character)) = chars.next() {
        match character {
            '\r' => {
                if bytes.get(at + 1) == Some(&b'\n') {
                    chars.next();
                    starts.push(at + 2);
                } else {
                    starts.push(at + 1);
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => starts.push(at + character.len_utf8()),
            _ => {}
        }
    }
    starts
}

/// Characters in a byte range of UTF-8 text (`utf8.RuneCountInString`).
fn character_count(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte & 0xc0 != 0x80).count()
}

#[cfg(test)]
#[path = "../../tests/unit/ts71_errors_baseline/tests.rs"]
mod tests;

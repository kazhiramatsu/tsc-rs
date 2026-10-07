//! The `.sourcemap.txt` baseline: tsgo's source map record
//! (`GetSourceMapRecord`, testutil/harnessutil/harnessutil.go:915-951, the
//! recorder of harnessutil/sourcemap_recorder.go and the baseline writer
//! `DoSourcemapRecordBaseline`, tsbaseline/sourcemap_record_baseline.go)
//! rendered from the emitted source maps, the generated files' text and the
//! source texts.
//!
//! The record decodes each map's `mappings` twice as tsgo does: the driver's
//! decoder feeds the spans and the writer's decoder checks them in lockstep
//! (the `!!^^` lines report a disagreement). Positions are bytes of the
//! UTF-8 texts, as in tsgo; the columns of the mappings are UTF-16 units.

use serde_json::Value;

use super::errors_baseline::{remove_test_path_prefixes, InputFile};

const CRLF: &str = "\r\n";
const EQUALS: &str = "===================================================================";
const DASHES: &str = "-------------------------------------------------------------------";

/// tsgo `SourceMapEmitResult`: one emitted source map with the files it
/// describes.
pub(super) struct SourceMapRecordInput<'a> {
    /// `GeneratedFile`, as the emitter named it (the record's `emittedFile:`).
    pub generated_file: &'a str,
    /// That file's emitted text; `None` when the harness did not collect it
    /// (tsgo fails on the nil file).
    pub generated_content: Option<&'a str>,
    /// `InputSourceFileNames`: the source files in the map's `sources` order,
    /// by the names `inputs` uses.
    pub input_source_files: &'a [String],
    /// The map's JSON.
    pub map_json: &'a str,
}

/// Renders the record of `records` (`GetSourceMapRecord`) with
/// `removeTestPathPrefixes` applied; `inputs` are the Program's source files.
/// An empty record is what tsgo writes no baseline for.
pub(super) fn render(
    records: &[SourceMapRecordInput<'_>],
    inputs: &[InputFile<'_>],
) -> Result<String, String> {
    let mut record = String::new();
    for input in records {
        let json: Value = serde_json::from_str(input.map_json)
            .map_err(|error| format!("{}: source map JSON: {error}", input.generated_file))?;
        let text = |key: &str| {
            json.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let list = |key: &str| -> Vec<String> {
            json.get(key)
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().unwrap_or_default().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let map = RawSourceMap {
            file: text("file"),
            source_root: text("sourceRoot"),
            sources: list("sources"),
            names: list("names"),
            sources_content: json
                .get("sourcesContent")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        };
        let mappings = text("mappings");
        let Some(generated_content) = input.generated_content else {
            return Err(format!(
                "{}: the generated file's text was not collected",
                input.generated_file
            ));
        };
        let mut writer = SpanWriter::new(
            &mut record,
            &map,
            &mappings,
            input.generated_file,
            generated_content,
        );
        let mut decoder = MappingsDecoder::new(&mappings);
        // The source file of the previous span; `None` is tsgo's nil file.
        let mut previous: Option<&str> = None;
        while let Some(mapping) = decoder.next() {
            if !mapping.is_source_mapping() {
                writer.record_source_map_span(mapping)?;
                continue;
            }
            let name = usize::try_from(mapping.source_index)
                .ok()
                .and_then(|index| input.input_source_files.get(index))
                .ok_or_else(|| {
                    format!(
                        "{}: source index {} outside the {} input files",
                        input.generated_file,
                        mapping.source_index,
                        input.input_source_files.len()
                    )
                })?;
            let current = inputs
                .iter()
                .find(|candidate| candidate.name == name.as_str());
            let current_name = current.map(|file| file.name);
            if current_name != previous {
                if let Some(file) = current {
                    writer.record_new_source_file_span(mapping, file.content)?;
                }
                previous = current_name;
            } else {
                writer.record_source_map_span(mapping)?;
            }
        }
        writer.close()?;
    }
    Ok(remove_test_path_prefixes(&record))
}

struct RawSourceMap {
    file: String,
    source_root: String,
    sources: Vec<String>,
    names: Vec<String>,
    sources_content: Vec<Option<String>>,
}

/// tsgo `sourcemap.Mapping`; `MISSING` is its `-1` sentinels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Mapping {
    generated_line: i64,
    generated_character: i64,
    source_index: i64,
    source_line: i64,
    source_character: i64,
    name_index: i64,
}

const MISSING: i64 = -1;

impl Mapping {
    fn is_source_mapping(&self) -> bool {
        self.source_index != MISSING
            && self.source_line != MISSING
            && self.source_character != MISSING
    }
}

/// tsgo `sourcemap.MappingsDecoder` (sourcemap/decoder.go).
struct MappingsDecoder<'a> {
    mappings: &'a [u8],
    done: bool,
    pos: usize,
    generated_line: i64,
    generated_character: i64,
    source_index: i64,
    source_line: i64,
    source_character: i64,
    name_index: i64,
    error: Option<String>,
}

impl<'a> MappingsDecoder<'a> {
    fn new(mappings: &'a str) -> Self {
        Self {
            mappings: mappings.as_bytes(),
            done: false,
            pos: 0,
            generated_line: 0,
            generated_character: 0,
            source_index: 0,
            source_line: 0,
            source_character: 0,
            name_index: 0,
            error: None,
        }
    }

    /// `State`: the current position as a full mapping.
    fn state(&self) -> Mapping {
        self.capture(true, true)
    }

    fn next(&mut self) -> Option<Mapping> {
        while !self.done && self.pos < self.mappings.len() {
            let ch = self.mappings[self.pos];
            if ch == b';' {
                self.generated_line += 1;
                self.generated_character = 0;
                self.pos += 1;
                continue;
            }
            if ch == b',' {
                self.pos += 1;
                continue;
            }
            let mut has_source = false;
            let mut has_name = false;
            self.generated_character += self.base64_vlq_decode();
            if self.error.is_some() {
                return self.stop();
            }
            if self.generated_character < 0 {
                return self.set_error_and_stop("Invalid generatedCharacter found");
            }
            if !self.is_segment_end() {
                has_source = true;
                self.source_index += self.base64_vlq_decode();
                if self.error.is_some() {
                    return self.stop();
                }
                if self.source_index < 0 {
                    return self.set_error_and_stop("Invalid sourceIndex found");
                }
                if self.is_segment_end() {
                    return self
                        .set_error_and_stop("Unsupported Format: No entries after sourceIndex");
                }
                self.source_line += self.base64_vlq_decode();
                if self.error.is_some() {
                    return self.stop();
                }
                if self.source_line < 0 {
                    return self.set_error_and_stop("Invalid sourceLine found");
                }
                if self.is_segment_end() {
                    return self
                        .set_error_and_stop("Unsupported Format: No entries after sourceLine");
                }
                self.source_character += self.base64_vlq_decode();
                if self.error.is_some() {
                    return self.stop();
                }
                if self.source_character < 0 {
                    return self.set_error_and_stop("Invalid sourceCharacter found");
                }
                if !self.is_segment_end() {
                    has_name = true;
                    self.name_index += self.base64_vlq_decode();
                    if self.error.is_some() {
                        return self.stop();
                    }
                    if self.name_index < 0 {
                        return self.set_error_and_stop("Invalid nameIndex found");
                    }
                    if !self.is_segment_end() {
                        return self.set_error_and_stop(
                            "Unsupported Error Format: Entries after nameIndex",
                        );
                    }
                }
            }
            return Some(self.capture(has_source, has_name));
        }
        self.stop()
    }

    fn capture(&self, has_source: bool, has_name: bool) -> Mapping {
        Mapping {
            generated_line: self.generated_line,
            generated_character: self.generated_character,
            source_index: if has_source {
                self.source_index
            } else {
                MISSING
            },
            source_line: if has_source {
                self.source_line
            } else {
                MISSING
            },
            source_character: if has_source {
                self.source_character
            } else {
                MISSING
            },
            name_index: if has_name { self.name_index } else { MISSING },
        }
    }

    fn stop(&mut self) -> Option<Mapping> {
        self.done = true;
        None
    }

    fn set_error_and_stop(&mut self, error: &str) -> Option<Mapping> {
        self.error = Some(error.to_owned());
        self.stop()
    }

    fn is_segment_end(&self) -> bool {
        self.pos == self.mappings.len()
            || self.mappings[self.pos] == b','
            || self.mappings[self.pos] == b';'
    }

    fn base64_vlq_decode(&mut self) -> i64 {
        let mut more_digits = true;
        let mut shift_count = 0;
        let mut value: i64 = 0;
        while more_digits {
            if self.pos >= self.mappings.len() {
                self.error = Some(
                    "Error in decoding base64VLQFormatDecode, past the mapping string".to_owned(),
                );
                return -1;
            }
            let current = base64_decode(self.mappings[self.pos]);
            if current == -1 {
                self.error = Some("Invalid character in VLQ".to_owned());
                return -1;
            }
            more_digits = (current & 32) != 0;
            value |= (current & 31) << shift_count;
            shift_count += 5;
            self.pos += 1;
        }
        if value & 1 == 0 {
            value >> 1
        } else {
            -(value >> 1)
        }
    }
}

fn base64_decode(ch: u8) -> i64 {
    match ch {
        b'A'..=b'Z' => i64::from(ch - b'A'),
        b'a'..=b'z' => i64::from(ch - b'a') + 26,
        b'0'..=b'9' => i64::from(ch - b'0') + 52,
        b'+' => 62,
        b'/' => 63,
        _ => -1,
    }
}

/// tsgo `core.ComputeECMALineStarts`: the byte offset of every line start
/// (`\r\n`, `\n`, `\r`, U+2028 and U+2029 end a line).
fn compute_line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut pos = 0;
    let mut line_start = 0;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte < 0x80 {
            pos += 1;
            match byte {
                b'\r' => {
                    if pos < bytes.len() && bytes[pos] == b'\n' {
                        pos += 1;
                    }
                    result.push(line_start);
                    line_start = pos;
                }
                b'\n' => {
                    result.push(line_start);
                    line_start = pos;
                }
                _ => {}
            }
        } else {
            let ch = text[pos..].chars().next().expect("a char boundary");
            pos += ch.len_utf8();
            if ch == '\u{2028}' || ch == '\u{2029}' {
                result.push(line_start);
                line_start = pos;
            }
        }
    }
    result.push(line_start);
    result
}

/// tsgo `scanner.ComputePositionOfLineAndUTF16Character` with `allowEdits`:
/// the byte position of a UTF-16 column, clamped to the text.
fn position_of_line_and_utf16_character(
    line_starts: &[usize],
    line: i64,
    character: i64,
    text: &str,
) -> usize {
    let line = if line < 0 {
        0
    } else {
        usize::try_from(line).map_or(line_starts.len() - 1, |line| {
            line.min(line_starts.len() - 1)
        })
    };
    let line_start = line_starts[line];
    if character > 0 {
        let line_end = if line + 1 < line_starts.len() {
            line_starts[line + 1]
        } else {
            text.len()
        };
        let mut utf16_count = 0;
        let mut pos = line_start;
        while pos < line_end {
            if utf16_count >= character {
                break;
            }
            let ch = text[pos..].chars().next().expect("a char boundary");
            utf16_count += ch.len_utf16() as i64;
            pos += ch.len_utf8();
        }
        return pos.min(text.len());
    }
    line_start.min(text.len())
}

/// The text of a line with its line break (`getTextOfLine`); the first line
/// loses its byte order mark.
fn text_of_line<'t>(line: i64, line_map: &[usize], code: &'t str) -> Result<&'t str, String> {
    let index = usize::try_from(line)
        .ok()
        .filter(|&index| index < line_map.len())
        .ok_or_else(|| format!("line {line} outside the {} lines", line_map.len()))?;
    let start = line_map[index];
    let end = if index + 1 < line_map.len() {
        line_map[index + 1]
    } else {
        code.len()
    };
    let text = &code[start..end];
    Ok(if index == 0 {
        text.strip_prefix('\u{feff}').unwrap_or(text)
    } else {
        text
    })
}

fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// tsgo `sourcemap.TryGetSourceMappingURL`: the `//# sourceMappingURL=`
/// comment among the last lines of the generated file.
fn try_get_source_mapping_url(text: &str, line_starts: &[usize]) -> String {
    for index in (0..line_starts.len()).rev() {
        let start = line_starts[index];
        let end = if index + 1 < line_starts.len() {
            line_starts[index + 1]
        } else {
            text.len()
        };
        let line = text[start..end]
            .trim_start_matches(char::is_whitespace)
            .trim_end_matches(is_line_break);
        if line.is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 4
            || !line.starts_with("//")
            || (bytes[2] != b'#' && bytes[2] != b'@')
            || bytes[3] != b' '
        {
            break;
        }
        if let Some(url) = line[4..].strip_prefix("sourceMappingURL=") {
            return url.trim_end_matches(char::is_whitespace).to_owned();
        }
    }
    String::new()
}

/// tsgo `sourceMapSpanWriter` with its `recordedSpanWriter`.
struct SpanWriter<'r, 'a> {
    record: &'r mut String,
    map: &'a RawSourceMap,
    mappings: &'a str,
    unit_name: &'a str,
    js_content: &'a str,
    js_line_map: Vec<usize>,
    ts_code: &'a str,
    ts_line_map: Vec<usize>,
    /// The spans of the generated line being recorded, with the decode
    /// errors of each.
    spans: Vec<(Mapping, Vec<String>)>,
    prev_written_source_pos: usize,
    next_js_line_to_write: i64,
    span_marker_continues: bool,
    decoder: MappingsDecoder<'a>,
}

impl<'r, 'a> SpanWriter<'r, 'a> {
    fn new(
        record: &'r mut String,
        map: &'a RawSourceMap,
        mappings: &'a str,
        unit_name: &'a str,
        js_content: &'a str,
    ) -> Self {
        let js_line_map = compute_line_starts(js_content);
        let map_url = try_get_source_mapping_url(js_content, &js_line_map);
        let mut writer = Self {
            record,
            map,
            mappings,
            unit_name,
            js_content,
            js_line_map,
            ts_code: "",
            ts_line_map: Vec::new(),
            spans: Vec::new(),
            prev_written_source_pos: 0,
            next_js_line_to_write: 0,
            span_marker_continues: false,
            decoder: MappingsDecoder::new(mappings),
        };
        writer.line(EQUALS);
        writer.line(&format!("JsFile: {}", map.file));
        writer.line(&format!("mapUrl: {map_url}"));
        writer.line(&format!("sourceRoot: {}", map.source_root));
        writer.line(&format!("sources: {}", map.sources.join(",")));
        if !map.sources_content.is_empty() {
            writer.line(&format!(
                "sourcesContent: {}",
                serde_json::to_string(&map.sources_content).unwrap_or_default()
            ));
        }
        writer.line(EQUALS);
        writer
    }

    fn line(&mut self, text: &str) {
        self.record.push_str(text);
        self.record.push_str(CRLF);
    }

    /// `getSourceMapSpanString`.
    fn span_string(&self, mapping: &Mapping, get_absent_name_index: bool) -> String {
        let mut text = format!(
            "Emitted({}, {})",
            mapping.generated_line + 1,
            mapping.generated_character + 1
        );
        if mapping.is_source_mapping() {
            text.push_str(&format!(
                " Source({}, {}) + SourceIndex({})",
                mapping.source_line + 1,
                mapping.source_character + 1,
                mapping.source_index
            ));
            match usize::try_from(mapping.name_index)
                .ok()
                .and_then(|index| self.map.names.get(index))
            {
                Some(name) => text.push_str(&format!(" name ({name})")),
                None if mapping.name_index != MISSING || get_absent_name_index => {
                    text.push_str(&format!(" nameIndex ({})", mapping.name_index));
                }
                None => {}
            }
        }
        text
    }

    /// `recordSourceMapSpan`: checks the span against the writer's own
    /// decoder and queues it on its generated line.
    fn record_source_map_span(&mut self, span: Mapping) -> Result<(), String> {
        let (decoded, decode_error) = match self.decoder.next() {
            Some(mapping) => (mapping, None),
            None => (
                self.decoder.state(),
                Some(
                    self.decoder
                        .error
                        .clone()
                        .unwrap_or_else(|| "No encoded entry found".to_owned()),
                ),
            ),
        };
        let mut decode_errors = Vec::new();
        if decode_error.is_some() || decoded != span {
            decode_errors.push(match decode_error {
                Some(error) => format!(
                    "!!^^ !!^^ There was decoding error in the sourcemap at this location: {error}"
                ),
                None => "!!^^ !!^^ The decoded span from sourcemap's mapping entry does not match what was encoded for this span:".to_owned(),
            });
            decode_errors.push(format!(
                "!!^^ !!^^ Decoded span from sourcemap's mappings entry: {} Span encoded by the emitter:{}",
                self.span_string(&decoded, true),
                self.span_string(&span, true)
            ));
        }
        if self
            .spans
            .first()
            .is_some_and(|(first, _)| first.generated_line != span.generated_line)
        {
            self.write_recorded_spans()?;
            self.spans.clear();
        }
        self.spans.push((span, decode_errors));
        Ok(())
    }

    /// `recordNewSourceFileSpan`: the first span of another source file.
    fn record_new_source_file_span(&mut self, span: Mapping, code: &'a str) -> Result<(), String> {
        let mut continues_line = false;
        // tsgo keeps Strada's comparison of the first span's character with
        // the new span's line.
        if self
            .spans
            .first()
            .is_some_and(|(first, _)| first.generated_character == span.generated_line)
        {
            self.write_recorded_spans()?;
            self.spans.clear();
            self.next_js_line_to_write -= 1;
            continues_line = true;
        }
        self.record_source_map_span(span)?;
        if self.spans.len() != 1 {
            return Err("expected a single span".to_owned());
        }
        let source = usize::try_from(span.source_index)
            .ok()
            .and_then(|index| self.map.sources.get(index))
            .ok_or_else(|| {
                format!(
                    "source index {} outside the map's {} sources",
                    span.source_index,
                    self.map.sources.len()
                )
            })?
            .clone();
        self.line(DASHES);
        if continues_line {
            self.line(&format!(
                "emittedFile:{} ({}, {})",
                self.unit_name,
                span.generated_line + 1,
                span.generated_character + 1
            ));
        } else {
            self.line(&format!("emittedFile:{}", self.unit_name));
        }
        self.line(&format!("sourceFile:{source}"));
        self.line(DASHES);
        self.ts_line_map = compute_line_starts(code);
        self.ts_code = code;
        self.prev_written_source_pos = 0;
        Ok(())
    }

    fn close(&mut self) -> Result<(), String> {
        self.write_recorded_spans()?;
        if self.decoder.pos != self.mappings.len() {
            self.line("!!!! **** There are more source map entries in the sourceMap's mapping than what was encoded");
            let remaining = format!(
                "!!!! **** Remaining decoded string: {}",
                &self.mappings[self.decoder.pos..]
            );
            self.line(&remaining);
        }
        let end = self.js_line_map.len() as i64;
        self.write_js_file_lines(end)
    }

    fn write_js_file_lines(&mut self, end_js_line: i64) -> Result<(), String> {
        while self.next_js_line_to_write < end_js_line {
            let text = text_of_line(
                self.next_js_line_to_write,
                &self.js_line_map,
                self.js_content,
            )?;
            self.record.push_str(">>>");
            self.record.push_str(text);
            self.next_js_line_to_write += 1;
        }
        Ok(())
    }

    /// `recordedSpanWriter.writeRecordedSpans`: the generated line, the
    /// markers, the source text of every span and the span details.
    fn write_recorded_spans(&mut self) -> Result<(), String> {
        if self.spans.is_empty() {
            return Ok(());
        }
        let spans = self.spans.clone();
        let current_js_line = spans[0].0.generated_line;
        self.write_js_file_lines(current_js_line + 1)?;
        let mut marker_ids = Vec::new();
        let mut prev_emitted_col = 0;
        for (index, (span, _)) in spans.iter().enumerate() {
            self.write_marker(
                &mut marker_ids,
                index,
                prev_emitted_col,
                span.generated_character,
                false,
            )?;
            prev_emitted_col = span.generated_character;
        }
        // tsgo keeps Strada's look at the next line's length here.
        let next_line_length =
            text_of_line(current_js_line + 1, &self.js_line_map, self.js_content)?.len() as i64;
        if prev_emitted_col < next_line_length - 1 {
            self.write_marker(
                &mut marker_ids,
                spans.len(),
                prev_emitted_col,
                next_line_length - 1,
                true,
            )?;
        }
        prev_emitted_col = 0;
        for (index, (span, decode_errors)) in spans.iter().enumerate() {
            self.write_source_text(span, decode_errors, &marker_ids[index], prev_emitted_col)?;
            prev_emitted_col = span.generated_character;
        }
        for (index, (span, _)) in spans.iter().enumerate() {
            let details = format!("{}{}", marker_ids[index], self.span_string(span, false));
            self.line(&details);
        }
        self.line("---");
        Ok(())
    }

    fn marker_id(&self, index: usize) -> Result<String, String> {
        if self.span_marker_continues {
            if index != 0 {
                return Err("expected markerIndex to be 0".to_owned());
            }
            return Ok("1->".to_owned());
        }
        let mut id = (index + 1).to_string();
        if id.len() < 2 {
            id.push(' ');
        }
        id.push('>');
        Ok(id)
    }

    fn indent(&mut self, length: i64, prefix: &str) {
        self.record.push_str(prefix);
        for _ in 0..length.max(0) {
            self.record.push(' ');
        }
    }

    /// `writeSourceMapMarkerEx`.
    fn write_marker(
        &mut self,
        marker_ids: &mut Vec<String>,
        index: usize,
        prev_emitted_col: i64,
        end_column: i64,
        end_continues: bool,
    ) -> Result<(), String> {
        let id = self.marker_id(index)?;
        self.indent(prev_emitted_col, &id);
        marker_ids.push(id);
        let mut column = prev_emitted_col;
        while column < end_column {
            self.record.push('^');
            column += 1;
        }
        if end_continues {
            self.record.push_str("->");
        }
        self.record.push_str(CRLF);
        self.span_marker_continues = end_continues;
        Ok(())
    }

    /// `writeSourceMapSourceText`: the source text from the previous span's
    /// position to this one's.
    fn write_source_text(
        &mut self,
        span: &Mapping,
        decode_errors: &[String],
        marker_id: &str,
        prev_emitted_col: i64,
    ) -> Result<(), String> {
        if self.ts_line_map.is_empty() {
            return Err("a source span before any source file".to_owned());
        }
        let ts_code = self.ts_code;
        let source_pos = position_of_line_and_utf16_character(
            &self.ts_line_map,
            span.source_line,
            span.source_character,
            ts_code,
        );
        let source_text = if self.prev_written_source_pos < source_pos {
            &ts_code[self.prev_written_source_pos..source_pos]
        } else {
            ""
        };
        for error in decode_errors {
            self.indent(prev_emitted_col, marker_id);
            self.line(error);
        }
        let line_map = compute_line_starts(source_text);
        for index in 0..line_map.len() {
            self.indent(prev_emitted_col, if index == 0 { marker_id } else { "  >" });
            let text = text_of_line(index as i64, &line_map, source_text)?;
            self.record.push_str(text);
            if index == line_map.len() - 1 {
                self.record.push_str(CRLF);
            }
        }
        self.prev_written_source_pos = source_pos;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mappings(text: &str) -> (Vec<Mapping>, Option<String>, usize) {
        let mut decoder = MappingsDecoder::new(text);
        let mut decoded = Vec::new();
        while let Some(mapping) = decoder.next() {
            decoded.push(mapping);
        }
        (decoded, decoder.error, decoder.pos)
    }

    #[test]
    fn decodes_segments_lines_and_names() {
        let (decoded, error, pos) = mappings("AAAA,SAASA;AACA");
        assert_eq!(error, None);
        assert_eq!(pos, 15);
        let span = |gl, gc, si, sl, sc, ni| Mapping {
            generated_line: gl,
            generated_character: gc,
            source_index: si,
            source_line: sl,
            source_character: sc,
            name_index: ni,
        };
        assert_eq!(
            decoded,
            vec![
                span(0, 0, 0, 0, 0, MISSING),
                span(0, 9, 0, 0, 9, 0),
                span(1, 0, 0, 1, 9, MISSING),
            ]
        );
        assert!(!span(0, 2, MISSING, MISSING, MISSING, MISSING).is_source_mapping());
    }

    #[test]
    fn decoder_reports_invalid_characters() {
        let (decoded, error, _) = mappings("AAAA,!");
        assert_eq!(decoded.len(), 1);
        assert_eq!(error.as_deref(), Some("Invalid character in VLQ"));
    }

    #[test]
    fn line_starts_follow_every_ecma_line_break() {
        assert_eq!(compute_line_starts(""), vec![0]);
        assert_eq!(compute_line_starts("a\r\nb\nc\u{2028}d"), vec![0, 3, 5, 9]);
        assert_eq!(compute_line_starts("x\n"), vec![0, 2]);
    }

    #[test]
    fn utf16_columns_map_to_byte_positions() {
        let text = "a\u{1d4b3}b";
        let starts = compute_line_starts(text);
        assert_eq!(position_of_line_and_utf16_character(&starts, 0, 1, text), 1);
        assert_eq!(position_of_line_and_utf16_character(&starts, 0, 2, text), 5);
        assert_eq!(position_of_line_and_utf16_character(&starts, 0, 3, text), 5);
        assert_eq!(position_of_line_and_utf16_character(&starts, 0, 9, text), 6);
        assert_eq!(position_of_line_and_utf16_character(&starts, 7, 0, text), 0);
    }

    #[test]
    fn source_mapping_url_is_read_from_the_last_lines() {
        let text = "var x;\n//# sourceMappingURL=a.js.map\n\n";
        assert_eq!(
            try_get_source_mapping_url(text, &compute_line_starts(text)),
            "a.js.map"
        );
        let text = "var x;\n//# sourceMappingURL=a.js.map\nvar y;";
        assert_eq!(
            try_get_source_mapping_url(text, &compute_line_starts(text)),
            ""
        );
    }

    #[test]
    fn renders_the_record_of_one_file() {
        let inputs = [InputFile {
            name: "/a.ts",
            content: "var x = 1;\n",
        }];
        let input_source_files = vec!["/a.ts".to_owned()];
        let record = render(
            &[SourceMapRecordInput {
                generated_file: "a.js",
                generated_content: Some("var x = 1;\n//# sourceMappingURL=a.js.map"),
                input_source_files: &input_source_files,
                map_json: r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["a.ts"],"names":[],"mappings":"AAAA,IAAI"}"#,
            }],
            &inputs,
        )
        .expect("rendered");
        let expected = [
            EQUALS,
            "JsFile: a.js",
            "mapUrl: a.js.map",
            "sourceRoot: ",
            "sources: a.ts",
            EQUALS,
            DASHES,
            "emittedFile:a.js",
            "sourceFile:a.ts",
            DASHES,
        ]
        .map(|line| format!("{line}\r\n"))
        .concat()
            + ">>>var x = 1;\n"
            + "1 >\r\n"
            + "2 >^^^^\r\n"
            + &format!("3 >    {}->\r\n", "^".repeat(24))
            + "1 >\r\n"
            + "2 >var \r\n"
            + "1 >Emitted(1, 1) Source(1, 1) + SourceIndex(0)\r\n"
            + "2 >Emitted(1, 5) Source(1, 5) + SourceIndex(0)\r\n"
            + "---\r\n"
            + ">>>//# sourceMappingURL=a.js.map";
        assert_eq!(record, expected);
    }

    #[test]
    fn marker_continues_on_the_next_recorded_line() {
        let inputs = [InputFile {
            name: "/a.ts",
            content: "a;\nb;\n",
        }];
        let input_source_files = vec!["/a.ts".to_owned()];
        // Two generated lines, one span each; the first line's continuation
        // marker makes the next line's first marker `1->`.
        let record = render(
            &[SourceMapRecordInput {
                generated_file: "a.js",
                generated_content: Some("a;\nb;\n//# sourceMappingURL=a.js.map"),
                input_source_files: &input_source_files,
                map_json: r#"{"version":3,"file":"a.js","sourceRoot":"","sources":["a.ts"],"names":[],"mappings":"AAAA;AACA"}"#,
            }],
            &inputs,
        )
        .expect("rendered");
        let body = record
            .split_once(&format!("{DASHES}\r\n>>>"))
            .expect("body")
            .1;
        assert_eq!(
            body,
            format!(
                "a;\n1 >\r\n2 >^^->\r\n1 >\r\n1 >Emitted(1, 1) Source(1, 1) + SourceIndex(0)\r\n---\r\n\
                 >>>b;\n1->\r\n2 >{}->\r\n1->a;\n  >\r\n\
                 1->Emitted(2, 1) Source(2, 1) + SourceIndex(0)\r\n---\r\n>>>//# sourceMappingURL=a.js.map",
                "^".repeat(28)
            )
        );
    }
}

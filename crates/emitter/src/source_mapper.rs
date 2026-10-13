//! tsgo `sourcemap` (decoder.go, source_mapper.go, util.go, lineinfo.go):
//! decoding a source map's `mappings`, the `sourceMappingURL` comment of a
//! generated file, and the document position mapper the language service
//! follows from a declaration file to its source. Positions are bytes of
//! the UTF-8 texts, as in tsgo; the columns of the mappings are UTF-16
//! units.

use std::collections::BTreeMap;

use tsc_diagnostics::JsStr;

/// tsgo `sourcemap.Mapping`; [`MISSING`] is its `-1` sentinels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mapping {
    pub generated_line: i64,
    pub generated_character: i64,
    pub source_index: i64,
    pub source_line: i64,
    pub source_character: i64,
    pub name_index: i64,
}

/// tsgo's `-1` for a mapping's absent source or name.
pub const MISSING: i64 = -1;

impl Mapping {
    /// tsgo `Mapping.IsSourceMapping`.
    pub fn is_source_mapping(&self) -> bool {
        self.source_index != MISSING
            && self.source_line != MISSING
            && self.source_character != MISSING
    }
}

/// tsgo `sourcemap.MappingsDecoder` (sourcemap/decoder.go).
pub struct MappingsDecoder<'a> {
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
    pub fn new(mappings: &'a str) -> Self {
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
    pub fn state(&self) -> Mapping {
        self.capture(true, true)
    }

    /// `Error`: why decoding stopped early.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The decoder's offset in the mappings.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// The next mapping, none at the end or after an error.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Mapping> {
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
pub fn compute_ecma_line_starts(text: &str) -> Vec<usize> {
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
pub fn position_of_line_and_utf16_character(
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

fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// tsgo `sourcemap.TryGetSourceMappingURL`: the `//# sourceMappingURL=`
/// comment among the last lines of the generated file.
pub fn try_get_source_mapping_url(text: &str, line_starts: &[usize]) -> String {
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

/// tsgo `sourcemap.Host`: the files a document position mapper reads.
pub trait SourceMapperHost {
    /// tsgo `CaseSensitivity`.
    fn use_case_sensitive_file_names(&self) -> bool;
    /// tsgo `ReadFile` (and the text of `GetECMALineInfo`).
    fn read_file(&self, file_name: &str) -> Option<String>;
}

/// tsgo `sourcemap.DocumentPosition`: a byte position of a file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentPosition {
    pub file_name: String,
    pub pos: usize,
}

/// tsgo `MappedPosition`, position-based (`-1` for a missing position).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MappedPosition {
    generated_position: i64,
    source_position: i64,
    source_index: i64,
}

impl MappedPosition {
    fn is_source_mapped_position(&self) -> bool {
        self.source_index != MISSING && self.source_position != MISSING
    }
}

/// tsgo `sourcemap.DocumentPositionMapper`: maps a generated file's
/// positions to its sources' and back.
#[derive(Debug)]
pub struct DocumentPositionMapper {
    use_case_sensitive_file_names: bool,
    source_file_absolute_paths: Vec<String>,
    source_mappings_by_path: BTreeMap<String, Vec<MappedPosition>>,
    generated_absolute_file_path: String,
    generated_mappings: Vec<MappedPosition>,
}

/// A file's text and ECMA line starts (tsgo `ECMALineInfo`), read once.
struct LineInfos<'h> {
    host: &'h dyn SourceMapperHost,
    files: BTreeMap<String, Option<(String, Vec<usize>)>>,
}

impl LineInfos<'_> {
    fn position(&mut self, file_name: &str, line: i64, character: i64) -> i64 {
        let host = self.host;
        let info = self.files.entry(file_name.to_owned()).or_insert_with(|| {
            host.read_file(file_name).map(|text| {
                let starts = compute_ecma_line_starts(&text);
                (text, starts)
            })
        });
        match info {
            Some((text, starts)) => {
                position_of_line_and_utf16_character(starts, line, character, text) as i64
            }
            None => MISSING,
        }
    }
}

impl DocumentPositionMapper {
    /// tsgo `GetSourcePosition`: the source position of the closest
    /// generated mapping at or after `pos` of the generated file.
    pub fn source_position(&self, pos: usize) -> Option<DocumentPosition> {
        if self.generated_mappings.is_empty() {
            return None;
        }
        let pos = pos as i64;
        let target = self
            .generated_mappings
            .partition_point(|mapping| mapping.generated_position < pos);
        let mapping = self.generated_mappings.get(target)?;
        if !mapping.is_source_mapped_position() {
            return None;
        }
        Some(DocumentPosition {
            file_name: self.source_file_absolute_paths[mapping.source_index as usize].clone(),
            pos: mapping.source_position as usize,
        })
    }

    /// tsgo `GetGeneratedPosition`: the generated position of the closest
    /// mapping of the source at or after `pos`.
    pub fn generated_position(&self, file_name: &str, pos: usize) -> Option<DocumentPosition> {
        let key = path_key(file_name, self.use_case_sensitive_file_names);
        let mappings = self.source_mappings_by_path.get(&key)?;
        let pos = pos as i64;
        let target = mappings.partition_point(|mapping| mapping.source_position < pos);
        let mapping = mappings.get(target)?;
        Some(DocumentPosition {
            file_name: self.generated_absolute_file_path.clone(),
            pos: mapping.generated_position as usize,
        })
    }
}

/// tsgo `GetDocumentPositionMapper`: the mapper of a generated file, from
/// its `sourceMappingURL` (a file or a base64 data URL) or `<file>.map`.
pub fn get_document_position_mapper(
    host: &dyn SourceMapperHost,
    generated_file_name: &str,
) -> Option<DocumentPositionMapper> {
    let mut map_file_name = host
        .read_file(generated_file_name)
        .map(|text| try_get_source_mapping_url(&text, &compute_ecma_line_starts(&text)))
        .unwrap_or_default();
    if !map_file_name.is_empty() {
        if let Some(base64) = try_parse_base64_url(&map_file_name) {
            if let Some(base64) = base64 {
                if let Some(decoded) = decode_base64(base64) {
                    return convert_document_to_source_mapper(host, &decoded, generated_file_name);
                }
            }
            // Not a data URL that can be parsed.
            map_file_name.clear();
        }
    }
    let mut possible_map_locations = Vec::new();
    if !map_file_name.is_empty() {
        possible_map_locations.push(map_file_name);
    }
    possible_map_locations.push(format!("{generated_file_name}.map"));
    let directory = tsc_program::get_directory_path(JsStr::from_str(generated_file_name))
        .to_string_lossy()
        .into_owned();
    for location in possible_map_locations {
        let Some(map_file_name) = try_resolve_source_map_path(&location, &directory) else {
            continue;
        };
        if let Some(contents) = host.read_file(&map_file_name) {
            return convert_document_to_source_mapper(host, &contents, &map_file_name);
        }
    }
    None
}

/// tsgo `convertDocumentToSourceMapper`: none for an invalid map or one
/// with inlined sources.
fn convert_document_to_source_mapper(
    host: &dyn SourceMapperHost,
    contents: &str,
    map_file_name: &str,
) -> Option<DocumentPositionMapper> {
    let parsed = try_parse_raw_source_map(contents)?;
    if parsed.sources.is_empty() || parsed.file.is_empty() || parsed.mappings.is_empty() {
        return None;
    }
    if parsed.has_inlined_sources {
        return None;
    }
    create_document_position_mapper(host, &parsed, map_file_name)
}

/// A version 3 source map's fields tsgo reads (`rawSourceMapJSON`).
struct RawSourceMap {
    file: String,
    source_root: Option<String>,
    /// The sources, `None` for a `null` entry.
    sources: Vec<Option<String>>,
    mappings: String,
    has_inlined_sources: bool,
}

/// tsgo `tryParseRawSourceMap`.
fn try_parse_raw_source_map(contents: &str) -> Option<RawSourceMap> {
    let value: serde_json::Value = serde_json::from_str(contents).ok()?;
    let object = value.as_object()?;
    if object.get("version")?.as_i64()? != 3 {
        return None;
    }
    let string = |name: &str| match object.get(name) {
        None | Some(serde_json::Value::Null) => Some(String::new()),
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(_) => None,
    };
    let source_root = match object.get("sourceRoot") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(_) => return None,
    };
    let sources = match object.get("sources") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::Array(sources)) => sources
            .iter()
            .map(|source| match source {
                serde_json::Value::Null => Some(None),
                serde_json::Value::String(text) => Some(Some(text.clone())),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        Some(_) => return None,
    };
    let has_inlined_sources = match object.get("sourcesContent") {
        Some(serde_json::Value::Array(contents)) => {
            contents.iter().any(|content| !content.is_null())
        }
        _ => false,
    };
    Some(RawSourceMap {
        file: string("file")?,
        source_root,
        sources,
        mappings: string("mappings")?,
        has_inlined_sources,
    })
}

/// tsgo `createDocumentPositionMapper`.
fn create_document_position_mapper(
    host: &dyn SourceMapperHost,
    source_map: &RawSourceMap,
    map_path: &str,
) -> Option<DocumentPositionMapper> {
    let map_directory = tsc_program::get_directory_path(JsStr::from_str(map_path))
        .to_string_lossy()
        .into_owned();
    // ECMA-426 prefixes an explicit empty sourceRoot with "/", but TypeScript
    // and established consumers treat it as absent.
    let source_url_prefix = match &source_map.source_root {
        Some(root) if !root.is_empty() => {
            if root.ends_with('/') {
                root.clone()
            } else {
                format!("{root}/")
            }
        }
        _ => String::new(),
    };
    let generated_absolute_file_path =
        try_resolve_source_map_path(&source_map.file, &map_directory)?;
    let mut unmapped_sources: Vec<bool> = source_map.sources.iter().map(Option::is_none).collect();
    let mut source_file_absolute_paths = vec![String::new(); source_map.sources.len()];
    for (index, source) in source_map.sources.iter().enumerate() {
        let Some(source) = source else {
            continue;
        };
        let with_prefix = format!("{source_url_prefix}{source}");
        let resolved = if with_prefix.is_empty() {
            Some(map_path.to_owned())
        } else {
            try_resolve_source_map_path(&with_prefix, &map_directory)
        };
        match resolved {
            Some(resolved) => source_file_absolute_paths[index] = resolved,
            None => unmapped_sources[index] = true,
        }
    }
    let use_case_sensitive_file_names = host.use_case_sensitive_file_names();
    let mut source_indices_by_path: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, source) in source_file_absolute_paths.iter().enumerate() {
        if unmapped_sources[index] {
            continue;
        }
        source_indices_by_path
            .entry(path_key(source, use_case_sensitive_file_names))
            .or_default()
            .push(index);
    }

    let mut line_infos = LineInfos {
        host,
        files: BTreeMap::new(),
    };
    let mut decoded = Vec::new();
    let mut decoder = MappingsDecoder::new(&source_map.mappings);
    while let Some(mapping) = decoder.next() {
        let generated_position = line_infos.position(
            &generated_absolute_file_path,
            mapping.generated_line,
            mapping.generated_character,
        );
        let mut source_position = MISSING;
        if mapping.is_source_mapping() {
            let index = mapping.source_index;
            if index >= 0
                && (index as usize) < source_file_absolute_paths.len()
                && !unmapped_sources[index as usize]
            {
                source_position = line_infos.position(
                    &source_file_absolute_paths[index as usize],
                    mapping.source_line,
                    mapping.source_character,
                );
            }
        }
        decoded.push(MappedPosition {
            generated_position,
            source_position,
            source_index: mapping.source_index,
        });
    }
    if decoder.error().is_some() {
        decoded.clear();
    }

    // getSourceMappings: each source's mappings by source position.
    let mut source_mappings: BTreeMap<i64, Vec<MappedPosition>> = BTreeMap::new();
    for mapping in &decoded {
        if mapping.is_source_mapped_position() {
            source_mappings
                .entry(mapping.source_index)
                .or_default()
                .push(*mapping);
        }
    }
    for list in source_mappings.values_mut() {
        list.sort_by_key(|mapping| mapping.source_position);
        list.dedup();
    }
    let mut source_mappings_by_path = BTreeMap::new();
    for (path, indices) in source_indices_by_path {
        let mut mappings = Vec::new();
        for index in indices {
            if let Some(list) = source_mappings.get(&(index as i64)) {
                mappings.extend(list.iter().copied());
            }
        }
        mappings.sort_by_key(|mapping| mapping.source_position);
        source_mappings_by_path.insert(path, mappings);
    }

    // getGeneratedMappings: by generated position.
    let mut generated_mappings = decoded;
    generated_mappings.sort_by_key(|mapping| mapping.generated_position);
    generated_mappings.dedup();

    Some(DocumentPositionMapper {
        use_case_sensitive_file_names,
        source_file_absolute_paths,
        source_mappings_by_path,
        generated_absolute_file_path,
        generated_mappings,
    })
}

/// tsgo `tryResolveSourceMapPath`: an absolute path, or one relative to
/// `directory`, normalized.
fn try_resolve_source_map_path(path: &str, directory: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let path = JsStr::from_str(path);
    let absolute = if tsc_program::path_root_parts(path).is_some() {
        path.to_owned()
    } else {
        tsc_program::combine_paths(JsStr::from_str(directory), path)
    };
    tsc_program::path_root_parts(absolute.as_js())?;
    Some(
        tsc_program::get_normalized_absolute_path(absolute.as_js(), JsStr::from_str(""))
            .to_string_lossy()
            .into_owned(),
    )
}

/// tsgo `CaseSensitivity.PathKey`.
fn path_key(file_name: &str, use_case_sensitive_file_names: bool) -> String {
    if use_case_sensitive_file_names {
        file_name.to_owned()
    } else {
        tsc_program::to_file_name_lower_case_js(JsStr::from_str(file_name))
            .to_string_lossy()
            .into_owned()
    }
}

/// tsgo `tryParseBase64Url`: none when the URL is not a data URL; the
/// base64 text when it is a JSON base64 one.
fn try_parse_base64_url(url: &str) -> Option<Option<&str>> {
    let url = url.strip_prefix("data:")?;
    let Some(url) = url.strip_prefix("application/json;") else {
        return Some(None);
    };
    let url = match url.strip_prefix("charset=") {
        Some(rest) => {
            if !rest
                .get(.."utf-8;".len())
                .is_some_and(|charset| charset.eq_ignore_ascii_case("utf-8;"))
            {
                return Some(None);
            }
            &rest["utf-8;".len()..]
        }
        None => url,
    };
    let Some(url) = url.strip_prefix("base64,") else {
        return Some(None);
    };
    if !url
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '='))
    {
        return Some(None);
    }
    Some(Some(url))
}

/// Go `base64.StdEncoding.DecodeString` (padded standard alphabet).
fn decode_base64(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut values = [0u32; 4];
        let mut padding = 0;
        for (index, &byte) in chunk.iter().enumerate() {
            values[index] = match byte {
                b'=' if index >= 2 => {
                    padding += 1;
                    0
                }
                _ if padding > 0 => return None,
                _ => u32::try_from(base64_decode(byte)).ok()?,
            };
        }
        let joined = values[0] << 18 | values[1] << 12 | values[2] << 6 | values[3];
        out.push((joined >> 16) as u8);
        if padding < 2 {
            out.push((joined >> 8) as u8);
        }
        if padding < 1 {
            out.push(joined as u8);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
#[path = "../tests/unit/source_mapper/tests.rs"]
mod tests;

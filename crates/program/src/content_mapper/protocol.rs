//! The content mapper protocol of tsgo hostimpl.go: the JSON-RPC methods,
//! their parameters and results, and the decoding of a transform response
//! into the compiler's positions.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tsc_diagnostics::spanmap::{
    DirectivePolicy, MappedDiagnosticDirective, Segment, SpanMap, FEATURES_ALL,
};

use super::errors::{DiagnosticDirectiveError, DiagnosticDirectiveErrorKind, ResponseFailure};
use super::is_supported_virtual_extension;

pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_OPEN_PROJECT: &str = "openProject";
pub const METHOD_CLOSE_PROJECT: &str = "closeProject";
pub const METHOD_TRANSFORM: &str = "transform";

/// tsgo `PositionEncoding`: how a mapper counts positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionEncoding {
    Utf8,
    Utf16,
}

impl PositionEncoding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "utf-8" => Some(Self::Utf8),
            "utf-16" => Some(Self::Utf16),
            _ => None,
        }
    }
}

/// tsgo `InitializeParams`.
#[derive(Serialize)]
pub struct InitializeParams<'a> {
    #[serde(skip_serializing_if = "str::is_empty")]
    pub locale: &'a str,
    #[serde(rename = "positionEncodings")]
    pub position_encodings: [&'static str; 2],
}

/// tsgo `InitializeResult`.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct InitializeResult {
    #[serde(rename = "positionEncoding")]
    pub position_encoding: String,
    #[serde(rename = "diagnosticSource")]
    pub diagnostic_source: String,
}

/// tsgo `OpenProjectParams`.
#[derive(Serialize)]
pub struct OpenProjectParams<'a> {
    #[serde(rename = "configFileName")]
    pub config_file_name: &'a str,
    #[serde(rename = "projectHandle")]
    pub project_handle: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<&'a RawValue>,
    #[serde(rename = "compilerOptions")]
    pub compiler_options: &'a RawValue,
}

/// tsgo `OpenProjectResult`.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct OpenProjectResult {
    #[serde(rename = "configIdentity")]
    pub config_identity: String,
    #[serde(rename = "watchedFiles")]
    pub watched_files: Vec<String>,
    #[serde(rename = "optionDiagnostics")]
    pub option_diagnostics: Vec<OptionDiagnosticResult>,
}

/// tsgo `OptionDiagnosticResult`: a path into the mapper entry's options
/// (property names and indices, as JSON values) and the message.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct OptionDiagnosticResult {
    pub path: Vec<Box<RawValue>>,
    #[serde(rename = "messageText")]
    pub message_text: String,
    pub code: i32,
}

/// tsgo `CloseProjectParams`.
#[derive(Serialize)]
pub struct CloseProjectParams<'a> {
    #[serde(rename = "projectHandle")]
    pub project_handle: &'a str,
}

/// tsgo `TransformParams`.
#[derive(Serialize)]
pub struct TransformParams<'a> {
    #[serde(rename = "fileName")]
    pub file_name: &'a str,
    pub content: &'a str,
    #[serde(rename = "projectHandle")]
    pub project_handle: &'a str,
}

/// tsgo `MappedOutput`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct MappedOutput {
    text: String,
    extension: String,
    mappings: Option<Box<RawValue>>,
    #[serde(rename = "diagnosticDirectives")]
    diagnostic_directives: Option<DiagnosticDirectives>,
}

/// tsgo `TransformResult`: a `MappedOutput` (its fields at the same level)
/// with the mapper's diagnostics and the supplemental outputs.
#[derive(Default, Deserialize)]
#[serde(default)]
struct TransformResult {
    text: String,
    extension: String,
    mappings: Option<Box<RawValue>>,
    #[serde(rename = "diagnosticDirectives")]
    diagnostic_directives: Option<DiagnosticDirectives>,
    #[serde(deserialize_with = "null_as_empty")]
    diagnostics: Vec<MapperDiagnosticResult>,
    #[serde(deserialize_with = "null_as_empty")]
    supplemental: Vec<MappedOutput>,
}

/// A list that Go's decoder also takes from `null` (a nil slice).
fn null_as_empty<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// tsgo `Diagnostic`: an error in original-source positions.
#[derive(Default, Deserialize)]
#[serde(default)]
struct MapperDiagnosticResult {
    #[serde(rename = "messageText")]
    message_text: String,
    start: i64,
    length: i64,
    code: i32,
}

/// tsgo `UnusedExpectDirectiveDiagnostic`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct UnusedExpectDirectiveDiagnostic {
    code: i32,
    #[serde(rename = "messageText")]
    message_text: String,
}

/// tsgo `DiagnosticDirectives`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct DiagnosticDirectives {
    #[serde(
        rename = "unusedExpectDirectiveDiagnostics",
        deserialize_with = "null_as_empty"
    )]
    unused_expect_directive_diagnostics: Vec<UnusedExpectDirectiveDiagnostic>,
    #[serde(deserialize_with = "null_as_empty")]
    directives: Vec<DirectiveTuple>,
}

/// tsgo `MappedDiagnosticDirective`'s tuple form: `[originalStart,
/// originalLength, virtualStart, virtualEnd, policy, unusedIndex?]`.
struct DirectiveTuple {
    original_start: i64,
    original_length: i64,
    virtual_start: i64,
    virtual_end: i64,
    policy: u8,
    unused_index: Option<i64>,
}

impl<'de> Deserialize<'de> for DirectiveTuple {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let tuple = Vec::<Box<RawValue>>::deserialize(deserializer)?;
        if tuple.len() != 5 && tuple.len() != 6 {
            return Err(D::Error::custom(format!(
                "diagnostic directive tuple must contain 5 or 6 elements, got {}",
                tuple.len()
            )));
        }
        let element = |index: usize| -> Result<i64, D::Error> {
            serde_json::from_str::<i64>(tuple[index].get()).map_err(|error| {
                D::Error::custom(format!(
                    "invalid diagnostic directive tuple element {index}: {error}"
                ))
            })
        };
        let policy = serde_json::from_str::<u8>(tuple[4].get()).map_err(|error| {
            D::Error::custom(format!(
                "invalid diagnostic directive tuple element 4: {error}"
            ))
        })?;
        Ok(Self {
            original_start: element(0)?,
            original_length: element(1)?,
            virtual_start: element(2)?,
            virtual_end: element(3)?,
            policy,
            unused_index: if tuple.len() == 6 {
                Some(element(5)?)
            } else {
                None
            },
        })
    }
}

/// tsgo `spanmap.Unmarshal`: segments from 5- or 6-number tuples; five
/// numbers take every feature.
pub fn decode_span_map(json: &str) -> Result<SpanMap, String> {
    let tuples: Vec<Vec<i32>> = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let mut segments = Vec::with_capacity(tuples.len());
    for (index, tuple) in tuples.iter().enumerate() {
        if tuple.len() != 5 && tuple.len() != 6 {
            return Err(format!(
                "span map segment {index}: expected 5 or 6 values, got {}",
                tuple.len()
            ));
        }
        segments.push(Segment {
            virtual_start: tuple[0],
            virtual_end: tuple[0].wrapping_add(tuple[1]),
            original_start: tuple[2],
            original_end: tuple[2].wrapping_add(tuple[3]),
            kind: tuple[4],
            features: tuple.get(5).copied().unwrap_or(FEATURES_ALL),
        });
    }
    Ok(SpanMap::new(segments))
}

/// tsgo `positionNormalizer`: a mapper's position in a text, in the
/// mapper's encoding, as the compiler's position (UTF-16 code units; tsgo's
/// are UTF-8 bytes). A position must not be negative, past the text or
/// inside a code point.
pub struct PositionNormalizer<'a> {
    text: &'a str,
    encoding: PositionEncoding,
    /// For each non-ASCII character: the UTF-8 offset after it and the
    /// UTF-16 offset after it.
    entries: Vec<(usize, usize)>,
    length: usize,
}

impl<'a> PositionNormalizer<'a> {
    pub fn new(text: &'a str, encoding: PositionEncoding) -> Self {
        let mut entries = Vec::new();
        let mut utf16 = 0;
        for (byte, character) in text.char_indices() {
            utf16 += character.len_utf16();
            if !character.is_ascii() {
                entries.push((byte + character.len_utf8(), utf16));
            }
        }
        let length = match encoding {
            PositionEncoding::Utf8 => text.len(),
            PositionEncoding::Utf16 => utf16,
        };
        Self {
            text,
            encoding,
            entries,
            length,
        }
    }

    pub fn normalize(&self, position: i64) -> Result<i32, String> {
        if position < 0 {
            return Err(format!("position {position} is negative"));
        }
        let offset = position as usize;
        if offset > self.length {
            return Err(format!(
                "position {position} exceeds {} length {}",
                self.encoding.as_str(),
                self.length
            ));
        }
        let splits = || format!("position {position} splits a Unicode code point");
        let utf16 = match self.encoding {
            PositionEncoding::Utf8 => {
                if !self.text.is_char_boundary(offset) {
                    return Err(splits());
                }
                self.utf8_to_utf16(offset)
            }
            PositionEncoding::Utf16 => {
                let byte = self.utf16_to_utf8(offset);
                // tsgo maps an offset inside a surrogate pair to a byte inside
                // its character.
                if byte > self.text.len()
                    || (byte < self.text.len() && !self.text.is_char_boundary(byte))
                {
                    return Err(splits());
                }
                offset
            }
        };
        i32::try_from(utf16).map_err(|_| format!("position {position} is too large"))
    }

    /// tsgo `PositionMap.UTF8ToUTF16` at a character boundary.
    fn utf8_to_utf16(&self, byte: usize) -> usize {
        let index = self.entries.partition_point(|&(after, _)| after <= byte);
        match index.checked_sub(1) {
            None => byte,
            Some(previous) => {
                let (after, utf16_after) = self.entries[previous];
                utf16_after + (byte - after)
            }
        }
    }

    /// tsgo `PositionMap.UTF16ToUTF8`, also for an offset inside a
    /// surrogate pair.
    fn utf16_to_utf8(&self, utf16: usize) -> usize {
        let index = self
            .entries
            .partition_point(|&(_, utf16_after)| utf16_after <= utf16);
        match index.checked_sub(1) {
            None => utf16,
            Some(previous) => {
                let (after, utf16_after) = self.entries[previous];
                after + (utf16 - utf16_after)
            }
        }
    }
}

/// A mapper's diagnostic in the original text's compiler positions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapperDiagnostic {
    pub start: i32,
    pub end: i32,
    pub code: i32,
    pub message_text: String,
    pub source: String,
}

/// tsgo `MappedResult`: one virtual source file and its mapping to the
/// original input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappedResult {
    pub text: String,
    pub virtual_extension: String,
    pub mappings: SpanMap,
    pub diagnostic_directives: Vec<MappedDiagnosticDirective>,
}

/// tsgo `Result`: the canonical output with the mapper's diagnostics of the
/// original content and the supplemental outputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransformOutput {
    pub canonical: MappedResult,
    pub diagnostics: Vec<MapperDiagnostic>,
    pub supplemental: Vec<MappedResult>,
}

/// tsgo `decodeTransformResult`.
pub fn decode_transform_result(
    raw: &[u8],
    original_text: &str,
    encoding: PositionEncoding,
    diagnostic_source: &str,
) -> Result<TransformOutput, ResponseFailure> {
    let result: TransformResult = serde_json::from_slice(raw)
        .map_err(|error| ResponseFailure::Malformed(error.to_string()))?;
    let output = MappedOutput {
        text: result.text,
        extension: result.extension,
        mappings: result.mappings,
        diagnostic_directives: result.diagnostic_directives,
    };
    let (canonical, original_positions) =
        decode_mapped_output(output, original_text, encoding, diagnostic_source)?;
    let mut supplemental = Vec::with_capacity(result.supplemental.len());
    for (index, output) in result.supplemental.into_iter().enumerate() {
        let (mapped, _) = decode_mapped_output(output, original_text, encoding, diagnostic_source)
            .map_err(|failure| match failure {
                ResponseFailure::DiagnosticDirective(mut error) => {
                    error.supplemental_index = Some(index);
                    ResponseFailure::DiagnosticDirective(error)
                }
                failure => failure,
            })?;
        supplemental.push(mapped);
    }
    let mut diagnostics = Vec::with_capacity(result.diagnostics.len());
    for diagnostic in result.diagnostics {
        if diagnostic.start < 0
            || diagnostic.length < 0
            || diagnostic.start > i64::MAX - diagnostic.length
        {
            return Err(ResponseFailure::Malformed(format!(
                "invalid content mapper diagnostic range [{}, {})",
                diagnostic.start,
                diagnostic.start.wrapping_add(diagnostic.length)
            )));
        }
        let start = original_positions
            .normalize(diagnostic.start)
            .map_err(|error| {
                ResponseFailure::Malformed(format!(
                    "invalid content mapper diagnostic start: {error}"
                ))
            })?;
        let end = original_positions
            .normalize(diagnostic.start + diagnostic.length)
            .map_err(|error| {
                ResponseFailure::Malformed(format!(
                    "invalid content mapper diagnostic end: {error}"
                ))
            })?;
        diagnostics.push(MapperDiagnostic {
            start,
            end,
            code: diagnostic.code,
            message_text: diagnostic.message_text,
            source: diagnostic_source.to_owned(),
        });
    }
    Ok(TransformOutput {
        canonical,
        diagnostics,
        supplemental,
    })
}

/// tsgo `decodeMappedOutput`.
fn decode_mapped_output<'a>(
    output: MappedOutput,
    original_text: &'a str,
    encoding: PositionEncoding,
    diagnostic_source: &str,
) -> Result<(MappedResult, PositionNormalizer<'a>), ResponseFailure> {
    if !is_supported_virtual_extension(&output.extension) {
        return Err(ResponseFailure::InvalidVirtualExtension(output.extension));
    }
    let virtual_positions = PositionNormalizer::new(&output.text, encoding);
    let original_positions = PositionNormalizer::new(original_text, encoding);
    // A successful transform always carries a span map; without mappings the
    // output is fully synthesized (an empty map, not "not content-mapped").
    let mappings = match output.mappings.as_deref().map(RawValue::get) {
        Some(json) if !json.is_empty() => {
            let mappings = decode_span_map(json).map_err(ResponseFailure::Malformed)?;
            normalize_mappings(&mappings, &virtual_positions, &original_positions)
                .map_err(ResponseFailure::Malformed)?
        }
        _ => SpanMap::new(Vec::new()),
    };
    let diagnostic_directives = normalize_diagnostic_directives(
        output.diagnostic_directives.as_ref(),
        &virtual_positions,
        &original_positions,
        diagnostic_source,
    )?;
    Ok((
        MappedResult {
            text: output.text.clone(),
            virtual_extension: output.extension,
            mappings,
            diagnostic_directives,
        },
        original_positions,
    ))
}

/// tsgo `normalizeDiagnosticDirectives`.
fn normalize_diagnostic_directives(
    directives: Option<&DiagnosticDirectives>,
    virtual_positions: &PositionNormalizer<'_>,
    original_positions: &PositionNormalizer<'_>,
    diagnostic_source: &str,
) -> Result<Vec<MappedDiagnosticDirective>, ResponseFailure> {
    let Some(directives) = directives else {
        return Ok(Vec::new());
    };
    let mut result = Vec::with_capacity(directives.directives.len());
    for (index, directive) in directives.directives.iter().enumerate() {
        let error = |kind| {
            ResponseFailure::DiagnosticDirective(DiagnosticDirectiveError {
                kind,
                index,
                supplemental_index: None,
                policy: 0,
            })
        };
        let mut normalized = MappedDiagnosticDirective {
            source: diagnostic_source.to_owned(),
            ..MappedDiagnosticDirective::default()
        };
        match directive.policy {
            0 => normalized.policy = DirectivePolicy::Ignore,
            1 => {
                let unused = &directives.unused_expect_directive_diagnostics;
                let unused_index = match directive.unused_index {
                    Some(index) => index,
                    None if unused.len() != 1 => {
                        return Err(error(
                            DiagnosticDirectiveErrorKind::ExpectMissingUnusedDiagnostic,
                        ))
                    }
                    None => 0,
                };
                let Some(unused) = usize::try_from(unused_index)
                    .ok()
                    .and_then(|index| unused.get(index))
                else {
                    return Err(error(
                        DiagnosticDirectiveErrorKind::InvalidUnusedDiagnosticIndex,
                    ));
                };
                normalized.policy = DirectivePolicy::Expect;
                normalized.unused_code = unused.code;
                normalized.unused_message_text = unused.message_text.clone();
            }
            policy => {
                return Err(ResponseFailure::DiagnosticDirective(
                    DiagnosticDirectiveError {
                        kind: DiagnosticDirectiveErrorKind::InvalidPolicy,
                        index,
                        supplemental_index: None,
                        policy,
                    },
                ))
            }
        }
        if directive.virtual_start < 0 || directive.virtual_end < directive.virtual_start {
            return Err(error(DiagnosticDirectiveErrorKind::InvalidRange));
        }
        let virtual_start = virtual_positions
            .normalize(directive.virtual_start)
            .map_err(|_| error(DiagnosticDirectiveErrorKind::InvalidRange))?;
        let virtual_end = virtual_positions
            .normalize(directive.virtual_end)
            .map_err(|_| error(DiagnosticDirectiveErrorKind::InvalidRange))?;
        normalized.virtual_range = (virtual_start, virtual_end);
        let mut valid_original = directive.original_start >= 0
            && directive.original_length >= 0
            && directive.original_start <= i64::MAX - directive.original_length;
        if valid_original {
            match (
                original_positions.normalize(directive.original_start),
                original_positions.normalize(directive.original_start + directive.original_length),
            ) {
                (Ok(start), Ok(end)) => normalized.original_range = (start, end),
                _ => valid_original = false,
            }
        }
        if normalized.policy == DirectivePolicy::Expect && !valid_original {
            return Err(error(DiagnosticDirectiveErrorKind::InvalidRange));
        }
        result.push(normalized);
    }
    let mut sorted: Vec<(usize, (i32, i32))> = result
        .iter()
        .enumerate()
        .map(|(index, directive)| (index, directive.virtual_range))
        .collect();
    sorted.sort_by_key(|&(_, (pos, _))| pos);
    for pair in sorted.windows(2) {
        if pair[1].1 .0 < pair[0].1 .1 {
            return Err(ResponseFailure::DiagnosticDirective(
                DiagnosticDirectiveError {
                    kind: DiagnosticDirectiveErrorKind::Overlap,
                    index: pair[1].0,
                    supplemental_index: None,
                    policy: 0,
                },
            ));
        }
    }
    Ok(result)
}

/// tsgo `normalizeMappings`: every segment's positions in the compiler's.
fn normalize_mappings(
    mappings: &SpanMap,
    virtual_positions: &PositionNormalizer<'_>,
    original_positions: &PositionNormalizer<'_>,
) -> Result<SpanMap, String> {
    let mut segments = mappings.segments().to_vec();
    for (index, segment) in segments.iter_mut().enumerate() {
        let normalize = |normalizer: &PositionNormalizer<'_>, position: i32, what: &str| {
            normalizer
                .normalize(i64::from(position))
                .map_err(|error| format!("invalid content mapper mapping {index} {what}: {error}"))
        };
        segment.virtual_start =
            normalize(virtual_positions, segment.virtual_start, "virtual start")?;
        segment.virtual_end = normalize(virtual_positions, segment.virtual_end, "virtual end")?;
        segment.original_start =
            normalize(original_positions, segment.original_start, "original start")?;
        segment.original_end = normalize(original_positions, segment.original_end, "original end")?;
    }
    Ok(SpanMap::new(segments))
}

#[cfg(test)]
#[path = "../../tests/unit/content_mapper/protocol_tests.rs"]
mod protocol_tests;

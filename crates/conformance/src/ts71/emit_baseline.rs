//! The native runner's JavaScript emit baseline (`DoJSEmitBaseline` in
//! `tsc/internal/testutil/tsbaseline/js_emit_baseline.go`) and raw source-map
//! baseline (`DoSourcemapBaseline` in `sourcemap_baseline.go`) rendered from
//! tsc-rs's emitted files, so a configuration's output can be compared with
//! its `.js` and `.js.map` references byte for byte.
//!
//! The `[DtsFileErrors]` section (the diagnostics of compiling the emitted
//! declaration files again) and the `!!!! File … noCheck emit` comparison
//! are appended by the caller from [`dts_file_errors_section`] and
//! [`no_check_sections`]. The JSON parse-error rendering for emitted `.json`
//! files is not reproduced.

use super::errors_baseline::{remove_test_path_prefixes, InputFile};
use tsc_compiler::{EmitArtifact, EmitArtifactKind};

const NEW_LINE: &str = "\r\n";

/// One file the emitter wrote, as the native harness records it.
pub(super) struct EmittedFile {
    /// The output path the emitter wrote to.
    pub path: String,
    /// The written bytes as text, including the byte order mark when one
    /// was written.
    pub content: String,
}

/// The emitted files of one configuration, grouped as the native harness
/// groups them (`newCompilationResult` in `harnessutil.go`): JavaScript and
/// JSON files, declaration files, then source maps, each in emit order.
#[derive(Default)]
pub(super) struct Emission {
    pub js: Vec<EmittedFile>,
    pub dts: Vec<EmittedFile>,
    pub maps: Vec<EmittedFile>,
}

impl Emission {
    pub fn from_writes(writes: &[EmitArtifact]) -> Self {
        let mut emission = Self::default();
        for artifact in writes {
            let file = EmittedFile {
                path: artifact.path().to_string_lossy().into_owned(),
                content: String::from_utf8_lossy(&artifact.materialized_bytes()).into_owned(),
            };
            match artifact.kind() {
                EmitArtifactKind::JavaScript => emission.js.push(file),
                EmitArtifactKind::Declaration => emission.dts.push(file),
                EmitArtifactKind::JavaScriptMap | EmitArtifactKind::DeclarationMap => {
                    emission.maps.push(file);
                }
                EmitArtifactKind::BuildInfo => {}
            }
        }
        emission
    }
}

/// The `.js` baseline: the header, the sources (`otherFiles` then
/// `toBeCompiled`), the JavaScript files, the declaration files and then
/// `sections` (the declaration-recompile errors and the noCheck comparison).
/// `None` when all of them are empty (the runner then writes no file).
pub(super) fn render_js(
    header: &str,
    sources: &[InputFile<'_>],
    emission: &Emission,
    full_emit_paths: bool,
    sections: &str,
) -> Option<String> {
    let mut ts_code = format!("//// [{header}] ////{NEW_LINE}{NEW_LINE}");
    for (index, source) in sources.iter().enumerate() {
        ts_code.push_str("//// [");
        ts_code.push_str(base_name(source.name));
        ts_code.push(']');
        ts_code.push_str(NEW_LINE);
        ts_code.push_str(source.content);
        if index + 1 < sources.len() {
            ts_code.push_str(NEW_LINE);
        }
    }
    let mut js_code = String::new();
    for file in &emission.js {
        if !js_code.is_empty() && !js_code.ends_with('\n') {
            js_code.push_str(NEW_LINE);
        }
        js_code.push_str(&file_output(file, full_emit_paths));
    }
    if !emission.dts.is_empty() {
        js_code.push_str(NEW_LINE);
        js_code.push_str(NEW_LINE);
        for file in &emission.dts {
            js_code.push_str(&file_output(file, full_emit_paths));
        }
    }
    js_code.push_str(sections);
    if js_code.is_empty() {
        return None;
    }
    Some(format!("{ts_code}{NEW_LINE}{NEW_LINE}{js_code}"))
}

/// The `[DtsFileErrors]` section over the declaration compile's error
/// baseline (`DoJSEmitBaseline`, js_emit_baseline.go:87-97).
pub(super) fn dts_file_errors_section(errors: &str) -> String {
    format!("{NEW_LINE}{NEW_LINE}//// [DtsFileErrors]{NEW_LINE}{NEW_LINE}{NEW_LINE}{errors}")
}

/// The comparison of the noCheck emit with the original one, declaration
/// files first (`compareResultFileSets`, js_emit_baseline.go:99-131): a file
/// only the noCheck emit wrote is listed with its text. A file both wrote
/// with different texts gets tsgo's header, but not its line diff, which no
/// reference baseline shows.
pub(super) fn no_check_sections(
    original: &Emission,
    no_check: &Emission,
    full_emit_paths: bool,
) -> String {
    let mut text = String::new();
    for (no_check_files, original_files) in
        [(&no_check.dts, &original.dts), (&no_check.js, &original.js)]
    {
        for file in no_check_files {
            let name = remove_test_path_prefixes(&file.path);
            match original_files
                .iter()
                .find(|candidate| candidate.path == file.path)
            {
                None => {
                    text.push_str(&format!(
                        "{NEW_LINE}{NEW_LINE}!!!! File {name} missing from original emit, but present in noCheck emit{NEW_LINE}"
                    ));
                    text.push_str(&file_output(file, full_emit_paths));
                }
                Some(original) if original.content != file.content => {
                    text.push_str(&format!(
                        "{NEW_LINE}{NEW_LINE}!!!! File {name} differs from original emit in noCheck emit{NEW_LINE}"
                    ));
                    let shown = if full_emit_paths {
                        name.clone()
                    } else {
                        base_name(&file.path).to_owned()
                    };
                    text.push_str(&format!("//// [{shown}]{NEW_LINE}"));
                }
                Some(_) => {}
            }
        }
    }
    text
}

/// The map options `DoSourcemapBaseline` reads.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MapOptions {
    pub source_map: bool,
    pub declaration_map: bool,
    pub inline_source_map: bool,
    pub no_emit_on_error: bool,
}

/// The `.js.map` baseline: every source map the configuration wrote, each
/// followed by its visualization link. `None` when the runner writes no
/// file: inline source maps, no map option, a `noEmitOnError` run with
/// diagnostics, or no map.
pub(super) fn render_js_map(
    options: MapOptions,
    has_diagnostics: bool,
    emission: &Emission,
    inputs: &[InputFile<'_>],
    full_emit_paths: bool,
) -> Option<String> {
    if options.inline_source_map || !(options.source_map || options.declaration_map) {
        return None;
    }
    if (options.no_emit_on_error && has_diagnostics) || emission.maps.is_empty() {
        return None;
    }
    let mut text = String::new();
    for map in &emission.maps {
        if !text.is_empty() {
            text.push_str(NEW_LINE);
        }
        text.push_str(&file_output(map, full_emit_paths));
        text.push_str(&preview_link(map, emission, inputs));
    }
    Some(text)
}

/// `createSourceMapPreviewLink`: the output the map describes, the map and
/// the map's sources, base64-encoded for the visualization site; empty when
/// the output or a source cannot be found.
fn preview_link(map: &EmittedFile, emission: &Emission, inputs: &[InputFile<'_>]) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&map.content) else {
        return String::new();
    };
    let file = json
        .get("file")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let Some(output) = emission
        .js
        .iter()
        .chain(&emission.dts)
        .chain(&emission.maps)
        .find(|candidate| candidate.path.ends_with(file))
    else {
        return String::new();
    };
    let mut sources = Vec::new();
    for source in json
        .get("sources")
        .and_then(|value| value.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = source.as_str().unwrap_or_default();
        let Some(input) = inputs.iter().find(|input| input.name.ends_with(name)) else {
            return String::new();
        };
        sources.push(input.content);
    }
    let mut link = String::from("\n//// https://sokra.github.io/source-map-visualization#base64,");
    link.push_str(&base64(output.content.as_bytes()));
    link.push(',');
    link.push_str(&base64(map.content.as_bytes()));
    for source in sources {
        link.push(',');
        link.push_str(&base64(source.as_bytes()));
    }
    link.push('\n');
    link
}

fn file_output(file: &EmittedFile, full_emit_paths: bool) -> String {
    let name = if full_emit_paths {
        remove_test_path_prefixes(&file.path)
    } else {
        base_name(&file.path).to_owned()
    };
    format!("//// [{name}]{NEW_LINE}{}", file.content)
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Standard base64 with padding (`encoding/base64.StdEncoding`).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

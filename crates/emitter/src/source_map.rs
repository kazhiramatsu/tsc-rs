//! Source-map generator foundation (H2.6a / m-1,
//! docs/design/greenfield/slices/h2-6a-m-1.md).
//!
//! Dormant port of the tsc 6.0.3 `createSourceMapGenerator` state machine
//! and the sources-relativization path closure. No production caller
//! exists in this rung: the m-2 recording model feeds `add_mapping`, and
//! the m-3 runtime flip constructs the generator from the emit host and
//! wires `to_json_string`/`raw_sources` into the artifact path.
//!
//! Deliberately absent (dead code in the vendored `_tsc.js` build; see
//! docs/design/greenfield/slices/h2-6a.md §4.2):
//!
//! | absent member | earliest live owner |
//! | --- | --- |
//! | `appendSourceMap` (_tsc.js:92477-92523) | H2.6b if its packet proves input-map reachability, else BLD1/L3 |
//! | `decodeMappings` (_tsc.js:92605-92726) | reachable only from `appendSourceMap`; same owner |
//! | `addName` (_tsc.js:92432-92443) | reachable only from `appendSourceMap`; the printer never records names (`emitPos` passes `nameIndex: undefined`, _tsc.js:121318-121319) |

use std::collections::HashMap;
use std::fmt::Write as _;

/// tsc-port: createSourceMapGenerator @6.0.3
/// tsc-hash: a9b20ed17638b1cd1d4c96a283a9731eef97e8ccffaf09b96d1e999d66d18e34
/// tsc-span: _tsc.js:92365-92601
///
/// The closure state is carried as owned fields: the six `last*` and six
/// `pending*` scalars keep their upstream pairing, and the four `has*`
/// flags stay separate booleans (no `Option` collapse — `pending*` values
/// remain readable after a flag resets, exactly as upstream). The host
/// pair (`getCurrentDirectory`/`getCanonicalFileName`) is threaded at
/// construction as the current directory plus the canonical-comparer
/// identity: the emit host canonicalizer lowercases if and only if file
/// names are case-insensitive, so a boolean selects identity versus the
/// ported `to_file_name_lower_case` arm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMapGenerator {
    file: Box<str>,
    source_root: Box<str>,
    sources_directory_path: Box<str>,
    current_directory: Box<str>,
    use_case_sensitive_source_keys: bool,
    raw_sources: Vec<Box<str>>,
    sources: Vec<Box<str>>,
    source_index_by_relative: HashMap<Box<str>, u32>,
    sources_content: Option<Vec<Option<Box<str>>>>,
    names: Vec<Box<str>>,
    mappings: String,
    last: MappingFields,
    has_last: bool,
    pending: MappingFields,
    has_pending: bool,
    has_pending_source: bool,
    has_pending_name: bool,
}

/// The six-scalar mapping record shared by the `last*`/`pending*` pairs
/// (_tsc.js:92375-92387).
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
struct MappingFields {
    generated_line: u32,
    generated_character: u32,
    source_index: u32,
    source_line: u32,
    source_character: u32,
    name_index: u32,
}

/// The all-or-nothing source triple of `addMapping`: upstream declares
/// three independent optionals but consumes them only when all three are
/// present (_tsc.js:92465), so the Rust surface types the conjunction.
#[derive(Debug, Clone, Copy)]
pub struct SourceMappingFields {
    pub source_index: u32,
    pub source_line: u32,
    pub source_character: u32,
}

impl SourceMapGenerator {
    /// Constructor facts mirror `createSourceMapGenerator(host, file,
    /// sourceRoot, sourcesDirectoryPath, generatorOptions)`; the
    /// `generatorOptions.extendedDiagnostics` timer is diagnostics-only
    /// upstream and has no output surface.
    pub fn new(
        file: impl Into<Box<str>>,
        source_root: impl Into<Box<str>>,
        sources_directory_path: impl Into<Box<str>>,
        current_directory: impl Into<Box<str>>,
        use_case_sensitive_source_keys: bool,
    ) -> Self {
        Self {
            file: file.into(),
            source_root: source_root.into(),
            sources_directory_path: sources_directory_path.into(),
            current_directory: current_directory.into(),
            use_case_sensitive_source_keys,
            raw_sources: Vec::new(),
            sources: Vec::new(),
            source_index_by_relative: HashMap::new(),
            sources_content: None,
            names: Vec::new(),
            mappings: String::new(),
            last: MappingFields::default(),
            has_last: false,
            pending: MappingFields::default(),
            has_pending: false,
            has_pending_source: false,
            has_pending_name: false,
        }
    }

    /// tsc-port: addSource @6.0.3
    /// tsc-hash: 76070cdb8ed2cd15927fb715699301c61cf8739305bef8c0d711abadd57960dc
    /// tsc-span: _tsc.js:92401-92420
    pub fn add_source(&mut self, file_name: &str) -> u32 {
        let source = paths::get_relative_path_to_directory_or_url(
            &self.sources_directory_path,
            file_name,
            &self.current_directory,
            self.use_case_sensitive_source_keys,
            /* is_absolute_path_an_url */ true,
        );
        if let Some(&index) = self.source_index_by_relative.get(source.as_str()) {
            return index;
        }
        let index = u32::try_from(self.sources.len()).expect("source index exceeds u32");
        let source: Box<str> = source.into();
        self.sources.push(source.clone());
        self.raw_sources.push(file_name.into());
        self.source_index_by_relative.insert(source, index);
        index
    }

    /// tsc-port: setSourceContent @6.0.3
    /// tsc-hash: 39bd7580b1839ba1bb8fc860fef0a1100acea48a054b1da0fa05f11c69ae3c5e
    /// tsc-span: _tsc.js:92421-92431
    ///
    /// Dormant in H2.6a: the sole upstream caller runs under
    /// `printerOptions.inlineSources` (H2.6b). Upstream's `content !==
    /// null` guard is the `Some` arm; holes back-fill with `null`.
    pub fn set_source_content(&mut self, source_index: u32, content: Option<&str>) {
        let Some(content) = content else { return };
        let sources_content = self.sources_content.get_or_insert_with(Vec::new);
        let index = source_index as usize;
        while sources_content.len() < index {
            sources_content.push(None);
        }
        if sources_content.len() == index {
            sources_content.push(Some(content.into()));
        } else {
            sources_content[index] = Some(content.into());
        }
    }

    /// tsc-port: isNewGeneratedPosition @6.0.3
    /// tsc-hash: db047537ae9d0a447d4ac971b9e28867961b4566ce316302373ad1eeb769827c
    /// tsc-span: _tsc.js:92444-92446
    fn is_new_generated_position(&self, generated_line: u32, generated_character: u32) -> bool {
        !self.has_pending
            || self.pending.generated_line != generated_line
            || self.pending.generated_character != generated_character
    }

    /// tsc-port: isBacktrackingSourcePosition @6.0.3
    /// tsc-hash: 1a1d2be8aea5ef81bfa0ba92dfa2d44eea67b989c535c1962f7f9986701d251d
    /// tsc-span: _tsc.js:92447-92449
    fn is_backtracking_source_position(&self, source: Option<SourceMappingFields>) -> bool {
        let Some(source) = source else { return false };
        self.pending.source_index == source.source_index
            && (self.pending.source_line > source.source_line
                || (self.pending.source_line == source.source_line
                    && self.pending.source_character > source.source_character))
    }

    /// tsc-port: addMapping @6.0.3
    /// tsc-hash: e56e5e0ddbfcaa4f8e9c172f5f04c6a7dd4efe0f2282198cda156f97c863e2a2
    /// tsc-span: _tsc.js:92450-92476
    ///
    /// Upstream's five `Debug.assert`s: the non-negativity four are
    /// vacuous under the `u32` field types; the monotone-generated-line
    /// invariant stays a live `debug_assert!`.
    pub fn add_mapping(
        &mut self,
        generated_line: u32,
        generated_character: u32,
        source: Option<SourceMappingFields>,
        name_index: Option<u32>,
    ) {
        debug_assert!(
            generated_line >= self.pending.generated_line,
            "generatedLine cannot backtrack"
        );
        if self.is_new_generated_position(generated_line, generated_character)
            || self.is_backtracking_source_position(source)
        {
            self.commit_pending_mapping();
            self.pending.generated_line = generated_line;
            self.pending.generated_character = generated_character;
            self.has_pending_source = false;
            self.has_pending_name = false;
            self.has_pending = true;
        }
        if let Some(source) = source {
            self.pending.source_index = source.source_index;
            self.pending.source_line = source.source_line;
            self.pending.source_character = source.source_character;
            self.has_pending_source = true;
            if let Some(name_index) = name_index {
                self.pending.name_index = name_index;
                self.has_pending_name = true;
            }
        }
    }

    /// tsc-port: shouldCommitMapping @6.0.3
    /// tsc-hash: 34eb70896449069893d153aa72fd5e84b28fc6d75d41dfd163c6c5eb363ed31f
    /// tsc-span: _tsc.js:92524-92526
    fn should_commit_mapping(&self) -> bool {
        !self.has_last
            || self.last.generated_line != self.pending.generated_line
            || self.last.generated_character != self.pending.generated_character
            || self.last.source_index != self.pending.source_index
            || self.last.source_line != self.pending.source_line
            || self.last.source_character != self.pending.source_character
            || self.last.name_index != self.pending.name_index
    }

    /// tsc-port: commitPendingMapping @6.0.3
    /// tsc-hash: bae0328c726adfcca8acf6f2506dceff0132fe6bb459389646d9b2f9b9d9d048
    /// tsc-span: _tsc.js:92533-92566
    ///
    /// The upstream `mappingCharCodes` buffer and its 1024-entry flush
    /// (`appendMappingCharCode` _tsc.js:92527-92532, `flushMappingBuffer`
    /// _tsc.js:92567-92572, hashes 388cd37403fe2aa5ce22ab1b713dbd09fe97b8c6be91c4dbbe599e2bd9cabd61 /
    /// 93fa554e80929bc4d28b5ffd9b392ee6bd58abc818ea482bb59f628010674608)
    /// are a `String.fromCharCode` batching detail with no observable
    /// boundary: the flushed concatenation equals direct appends, so this
    /// port pushes bytes straight onto `mappings`. Every VLQ delta is
    /// computed SIGNED before encoding (packet §5: 145 of the 845 frozen
    /// segments carry negative source deltas).
    fn commit_pending_mapping(&mut self) {
        if !self.has_pending || !self.should_commit_mapping() {
            return;
        }
        if self.last.generated_line < self.pending.generated_line {
            loop {
                self.mappings.push(';');
                self.last.generated_line += 1;
                if self.last.generated_line >= self.pending.generated_line {
                    break;
                }
            }
            self.last.generated_character = 0;
        } else {
            debug_assert!(
                self.last.generated_line == self.pending.generated_line,
                "generatedLine cannot backtrack"
            );
            if self.has_last {
                self.mappings.push(',');
            }
        }
        self.append_base64_vlq(
            i64::from(self.pending.generated_character) - i64::from(self.last.generated_character),
        );
        self.last.generated_character = self.pending.generated_character;
        if self.has_pending_source {
            self.append_base64_vlq(
                i64::from(self.pending.source_index) - i64::from(self.last.source_index),
            );
            self.last.source_index = self.pending.source_index;
            self.append_base64_vlq(
                i64::from(self.pending.source_line) - i64::from(self.last.source_line),
            );
            self.last.source_line = self.pending.source_line;
            self.append_base64_vlq(
                i64::from(self.pending.source_character) - i64::from(self.last.source_character),
            );
            self.last.source_character = self.pending.source_character;
            if self.has_pending_name {
                self.append_base64_vlq(
                    i64::from(self.pending.name_index) - i64::from(self.last.name_index),
                );
                self.last.name_index = self.pending.name_index;
            }
        }
        self.has_last = true;
    }

    /// tsc-port: appendBase64VLQ @6.0.3
    /// tsc-hash: f374335534e2e04e59f192225dc992c2f8ca4fb187f947013a104120c9bb336b
    /// tsc-span: _tsc.js:92586-92600
    fn append_base64_vlq(&mut self, value: i64) {
        let mut encoded: u64 = if value < 0 {
            ((value.unsigned_abs()) << 1) + 1
        } else {
            (value as u64) << 1
        };
        loop {
            let mut digit = (encoded & 31) as u8;
            encoded >>= 5;
            if encoded > 0 {
                digit |= 32;
            }
            self.mappings.push(base64_format_encode(digit));
            if encoded == 0 {
                break;
            }
        }
    }

    /// tsc-port: toJSON @6.0.3
    /// tsc-hash: c2ab20d2c4b9ad2907a1b4fb38e563add488d1f61bb4ee61f347ac0050e44298
    /// tsc-span: _tsc.js:92573-92585
    ///
    /// Also carries `toString` (_tsc.js:92399, hash
    /// 2328cb3bfeb115cf5b1db2071c49fb26d80892e825276573ff4e9da7c098f3a8):
    /// `JSON.stringify(toJSON())` with no replacer or indent. The key
    /// order is the literal upstream order — `version`, `file`,
    /// `sourceRoot`, `sources`, `names`, `mappings`, and
    /// `sourcesContent` LAST, dropped entirely while unset
    /// (`JSON.stringify` elides `undefined` properties).
    pub fn to_json_string(&mut self) -> String {
        self.commit_pending_mapping();
        let mut out = String::new();
        out.push_str("{\"version\":3,\"file\":");
        push_json_string(&mut out, &self.file);
        out.push_str(",\"sourceRoot\":");
        push_json_string(&mut out, &self.source_root);
        out.push_str(",\"sources\":[");
        for (index, source) in self.sources.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            push_json_string(&mut out, source);
        }
        out.push_str("],\"names\":[");
        for (index, name) in self.names.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            push_json_string(&mut out, name);
        }
        out.push_str("],\"mappings\":");
        push_json_string(&mut out, &self.mappings);
        if let Some(sources_content) = &self.sources_content {
            out.push_str(",\"sourcesContent\":[");
            for (index, content) in sources_content.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                match content {
                    Some(content) => push_json_string(&mut out, content),
                    None => out.push_str("null"),
                }
            }
            out.push(']');
        }
        out.push('}');
        out
    }

    /// tsc-port: getSources @6.0.3
    /// tsc-hash: ddeb72ac8a70cccaf038d14e4c4011e4027157835e2b2804fe79c7dfca76f26e
    /// tsc-span: _tsc.js:92392-92392
    ///
    /// The RAW registered file names in registration order — the m-3
    /// `SourceMapObservation.input_source_files` producer.
    pub fn raw_sources(&self) -> &[Box<str>] {
        &self.raw_sources
    }
}

/// tsc-port: base64FormatEncode @6.0.3
/// tsc-hash: 0a8c54fb9975179519b9840fdcbebb8c2ba29e39c5efb7affecde15bb22250ef
/// tsc-span: _tsc.js:92727-92729
fn base64_format_encode(value: u8) -> char {
    match value {
        0..=25 => (b'A' + value) as char,
        26..=51 => (b'a' + (value - 26)) as char,
        52..=61 => (b'0' + (value - 52)) as char,
        62 => '+',
        63 => '/',
        _ => unreachable!("base64 digit out of range: {value}"),
    }
}

/// `JSON.stringify`-equivalent string escaping (packet §5, V8-verified):
/// `"` and `\` escaped, U+0008/0009/000A/000C/000D as the `\b\t\n\f\r`
/// short forms, every other C0 control as four-digit lowercase `\u00xx`
/// hex, and everything else — DEL, U+2028/U+2029, all non-ASCII — passes
/// through raw.
fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{0009}' => out.push_str("\\t"),
            '\u{000A}' => out.push_str("\\n"),
            '\u{000C}' => out.push_str("\\f"),
            '\u{000D}' => out.push_str("\\r"),
            ch if (ch as u32) < 0x20 => {
                write!(out, "\\u{:04x}", ch as u32).expect("string write is infallible");
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Constructor inputs for a print-lifetime recording (h2-6a-m-2 §4):
/// exactly `createSourceMapGenerator`'s host/file/sourceRoot/
/// sourcesDirectoryPath surface, carried as owned values so the printer
/// needs no host reach.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMapRecordingInputs {
    pub file: Box<str>,
    pub source_root: Box<str>,
    pub sources_directory_path: Box<str>,
    pub current_directory: Box<str>,
    pub use_case_sensitive_source_keys: bool,
    /// h2-6b-m-1: `printerOptions.inlineSources` — the registration
    /// point feeds `set_source_content` iff set (upstream
    /// `setSourceMapSource` 121352-121370). `false` at the H2.6a floor.
    pub inline_sources: bool,
}

/// Where the current print source stands with the generator: JSON
/// sources are never registered and never record (upstream
/// `setSourceMapSource` skips registration for `.json` and `emitPos`
/// checks `isJsonSourceMapSource`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegisteredSource {
    Json,
    Indexed(u32),
}

/// Print-lifetime source-map recording state (h2-6a-m-2 §4): the m-1
/// generator plus the current-source identity, the per-source
/// registration memo, and the `NO_NESTED_SOURCE_MAPS` suppression depth
/// (upstream's mutable `sourceMapsDisabled` extent, carried here because
/// token and comment records never see an `EmitContext`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMapRecording {
    generator: SourceMapGenerator,
    registered: HashMap<crate::TransformSourceId, RegisteredSource>,
    current: Option<RegisteredSource>,
    suppressed_depth: u32,
    inline_sources: bool,
}

impl SourceMapRecording {
    pub fn new(inputs: SourceMapRecordingInputs) -> Self {
        Self {
            generator: SourceMapGenerator::new(
                inputs.file,
                inputs.source_root,
                inputs.sources_directory_path,
                inputs.current_directory,
                inputs.use_case_sensitive_source_keys,
            ),
            registered: HashMap::new(),
            current: None,
            suppressed_depth: 0,
            inline_sources: inputs.inline_sources,
        }
    }

    fn register(
        &mut self,
        source: crate::TransformSourceId,
        file_name: &str,
        source_text: Option<&str>,
    ) -> RegisteredSource {
        if let Some(&registered) = self.registered.get(&source) {
            return registered;
        }
        // tsc-port: isJsonSourceMapSource @6.0.3
        // tsc-hash: 6dfe87426da2f4738186629fcf86526d3d4b1030570d6284437b0d8ec67dbee2
        // tsc-span: _tsc.js:121375-121377
        let registered = if file_name.to_ascii_lowercase().ends_with(".json") {
            RegisteredSource::Json
        } else {
            let index = self.generator.add_source(file_name);
            // tsc-port: setSourceMapSource inlineSources arm @6.0.3
            // tsc-hash: 9d5ddceb0e432a1400a46679dcb977ee72a5c90837177cc4cd2cf705e82065ac
            // tsc-span: _tsc.js:121352-121370
            //
            // h2-6b-m-1: `setSourceContent(sourceMapSourceIndex,
            // sourceMapSource.text)` runs iff `printerOptions.inlineSources`,
            // once per registration (the memo above is the once-guard).
            if self.inline_sources {
                if let Some(source_text) = source_text {
                    self.generator.set_source_content(index, Some(source_text));
                }
                // A `None` text reaches here only from the foreign-source
                // seam (`record_for_source`), which no tsc-CLI input can
                // drive (h2-6b.md §4.6: `SourceMapSource` is never
                // constructed in `_tsc.js`); the public-API era owns the
                // foreign-content feed.
            }
            RegisteredSource::Indexed(index)
        };
        self.registered.insert(source, registered);
        registered
    }

    /// tsc-port: setSourceMapSource @6.0.3
    /// tsc-hash: 9d5ddceb0e432a1400a46679dcb977ee72a5c90837177cc4cd2cf705e82065ac
    /// tsc-span: _tsc.js:121352-121370
    ///
    /// Registers the print source up front — the empty-file witness pins
    /// `sources:["input.ts"]` with zero mappings.
    pub(crate) fn set_current_source(
        &mut self,
        source: crate::TransformSourceId,
        file_name: &str,
        source_text: &str,
    ) {
        let registered = self.register(source, file_name, Some(source_text));
        self.current = Some(registered);
    }

    pub(crate) fn suppress(&mut self) {
        self.suppressed_depth += 1;
    }

    pub(crate) fn unsuppress(&mut self) {
        self.suppressed_depth = self
            .suppressed_depth
            .checked_sub(1)
            .expect("source-map suppression depth underflow");
    }

    /// tsc-port: emitPos @6.0.3
    /// tsc-hash: 0f8f04ad2314d22ba03071e0cdd6fd5df55a159098f978cbddb6aafc8925b33a
    /// tsc-span: _tsc.js:121307-121321
    ///
    /// The generated triple comes from the writer at the record moment;
    /// synthesized-position filtering happens at the call sites (the
    /// range machinery), suppression and the JSON guard here.
    pub(crate) fn record_current(
        &mut self,
        source_line: u32,
        source_character: u32,
        generated_line: u32,
        generated_character: u32,
    ) {
        if self.suppressed_depth > 0 {
            return;
        }
        let Some(RegisteredSource::Indexed(index)) = self.current else {
            return;
        };
        self.generator.add_mapping(
            generated_line,
            generated_character,
            Some(SourceMappingFields {
                source_index: index,
                source_line,
                source_character,
            }),
            None,
        );
    }

    /// tsc-port: emitSourcePos @6.0.3
    /// tsc-hash: c3f079664348b23bc5f254936a266eb104d7fad5cc793ff5f49159625e1f8b70
    /// tsc-span: _tsc.js:121322-121332
    ///
    /// The foreign-source lane: registers on demand (the upstream
    /// save/set/emit/restore collapses to an explicit per-record source
    /// because the printer passes the effective source at every site).
    pub(crate) fn record_for_source(
        &mut self,
        source: crate::TransformSourceId,
        file_name: &str,
        source_line: u32,
        source_character: u32,
        generated_line: u32,
        generated_character: u32,
    ) {
        if self.suppressed_depth > 0 {
            return;
        }
        let RegisteredSource::Indexed(index) = self.register(source, file_name, None) else {
            return;
        };
        self.generator.add_mapping(
            generated_line,
            generated_character,
            Some(SourceMappingFields {
                source_index: index,
                source_line,
                source_character,
            }),
            None,
        );
    }

    pub fn into_generator(self) -> SourceMapGenerator {
        self.generator
    }
}

/// The sources-relativization path closure (packet §4/§5). Private to
/// this module: the generator is the only consumer in this rung, and the
/// m-3 orchestration lane names its own ports for the string-level
/// helpers (`getBaseFileName`, `getDirectoryPath`, `normalizePath`).
///
/// `resolvePath`/`normalizePath` (_tsc.js:5487-5489 / 5568-5576, with
/// `simpleNormalizePath` 5577-5592, `relativePathSegmentRegExp` 5630 and
/// `getNormalizedAbsolutePath` 5493-5567) are deliberately NOT ported as
/// string functions: `get_path_components_relative_to` consumes only the
/// REDUCED COMPONENT LISTS of the combined inputs, and
/// `reduce ∘ components` is idempotent over `normalizePath`'s output and
/// equal to `reduce ∘ components` over the un-normalized combined string
/// (both drop empty/`.` segments identically, both clamp `..` at an
/// absolute root, and both preserve leading `..` for relative inputs) —
/// the recorded packet §4 equivalence argument.
pub(crate) mod paths {
    /// tsc-port: normalizeSlashes @6.0.3
    /// tsc-hash: d53c3e92f0b97072b15fe2ed30c413ab7f33522619f88528f818eef207535163
    /// tsc-span: _tsc.js:5452-5454
    pub(crate) fn normalize_slashes(path: &str) -> String {
        if path.contains('\\') {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        }
    }

    /// tsc-port: isVolumeCharacter @6.0.3
    /// tsc-hash: 26e9866fded2866d431ef33aec085f14e19c50914b530d013181c5bf80eac7de
    /// tsc-span: _tsc.js:5337-5339
    fn is_volume_character(byte: u8) -> bool {
        byte.is_ascii_lowercase() || byte.is_ascii_uppercase()
    }

    /// tsc-port: getFileUrlVolumeSeparatorEnd @6.0.3
    /// tsc-hash: 57f3c75761fc787cb4cb93d2c9e5cf3a95d5afb23005ab84c6418087f065a629
    /// tsc-span: _tsc.js:5340-5348
    fn get_file_url_volume_separator_end(url: &[u8], start: usize) -> Option<usize> {
        match url.get(start) {
            Some(b':') => Some(start + 1),
            Some(b'%') if url.get(start + 1) == Some(&b'3') => match url.get(start + 2) {
                Some(b'a') | Some(b'A') => Some(start + 3),
                _ => None,
            },
            _ => None,
        }
    }

    /// tsc-port: getEncodedRootLength @6.0.3
    /// tsc-hash: ad42b701dd98c53ad89476947bccf551e3ab3db9ce0c9fc5009e16a41b49b1f9
    /// tsc-span: _tsc.js:5349-5386
    ///
    /// URL roots are `~`-encoded negative exactly as upstream. Index
    /// arithmetic runs over UTF-8 bytes: every structural delimiter is
    /// ASCII, so byte scanning finds the same boundaries as upstream's
    /// UTF-16 scan and the returned lengths are valid `&str` slice
    /// boundaries; the numeric VALUE differs from JS only for non-ASCII
    /// authority text, which no comparison in this module observes.
    fn get_encoded_root_length(path: &str) -> i64 {
        let bytes = path.as_bytes();
        if bytes.is_empty() {
            return 0;
        }
        let ch0 = bytes[0];
        if ch0 == b'/' || ch0 == b'\\' {
            if bytes.get(1) != Some(&ch0) {
                return 1;
            }
            let separator = if ch0 == b'/' { b'/' } else { b'\\' };
            return match bytes[2..].iter().position(|&b| b == separator) {
                None => bytes.len() as i64,
                Some(offset) => (offset + 2 + 1) as i64,
            };
        }
        if is_volume_character(ch0) && bytes.get(1) == Some(&b':') {
            match bytes.get(2) {
                Some(b'/') | Some(b'\\') => return 3,
                _ => {}
            }
            if bytes.len() == 2 {
                return 2;
            }
        }
        if let Some(scheme_end) = find_subslice(bytes, b"://") {
            let authority_start = scheme_end + 3;
            if let Some(authority_offset) = bytes[authority_start..].iter().position(|&b| b == b'/')
            {
                let authority_end = authority_start + authority_offset;
                let scheme = &path[..scheme_end];
                let authority = &path[authority_start..authority_end];
                if scheme == "file"
                    && (authority.is_empty() || authority == "localhost")
                    && bytes
                        .get(authority_end + 1)
                        .copied()
                        .is_some_and(is_volume_character)
                {
                    if let Some(volume_separator_end) =
                        get_file_url_volume_separator_end(bytes, authority_end + 2)
                    {
                        if bytes.get(volume_separator_end) == Some(&b'/') {
                            return !((volume_separator_end + 1) as i64);
                        }
                        if volume_separator_end == bytes.len() {
                            return !(volume_separator_end as i64);
                        }
                    }
                }
                return !((authority_end + 1) as i64);
            }
            return !(bytes.len() as i64);
        }
        0
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    /// tsc-port: getRootLength @6.0.3
    /// tsc-hash: 667612ab2d64309ca725b3e9e2b8c2d5e30fc1294a373956781a1f8a066e5cbb
    /// tsc-span: _tsc.js:5387-5390
    pub(crate) fn get_root_length(path: &str) -> usize {
        let root_length = get_encoded_root_length(path);
        (if root_length < 0 {
            !root_length
        } else {
            root_length
        }) as usize
    }

    /// tsc-port: isRootedDiskPath @6.0.3
    /// tsc-hash: 8b2dd2a22675acbfa2df9f0725c62de7f4d94457736242b226b4785ab69834a9
    /// tsc-span: _tsc.js:5304-5306
    ///
    /// Strictly positive encoded length: a `~`-encoded URL root is NOT a
    /// disk path, which is what keeps the `file://` prefix arm below off
    /// already-URL components.
    fn is_rooted_disk_path(path: &str) -> bool {
        get_encoded_root_length(path) > 0
    }

    /// tsc-port: hasTrailingDirectorySeparator @6.0.3
    /// tsc-hash: 41e47fac4a2d5c0eb6359198faf485e13ba5dbccbe7c0b13025f6c338c7960ed
    /// tsc-span: _tsc.js:5334-5336
    fn has_trailing_directory_separator(path: &str) -> bool {
        matches!(path.as_bytes().last(), Some(b'/') | Some(b'\\'))
    }

    /// tsc-port: ensureTrailingDirectorySeparator @6.0.3
    /// tsc-hash: 5bd7b55e9a33e4a9fbea300f2f8ee2011c420d700dcde3d9df360328fb0beca8
    /// tsc-span: _tsc.js:5610-5615
    pub(crate) fn ensure_trailing_directory_separator(path: &str) -> String {
        if has_trailing_directory_separator(path) {
            path.to_owned()
        } else {
            format!("{path}/")
        }
    }

    /// tsc-port: pathComponents @6.0.3
    /// tsc-hash: ce00442504a457bdc73ac095fbeee31278f66cb566b14edea2fdb0bdb0c1e863
    /// tsc-span: _tsc.js:5437-5442
    fn path_components(path: &str, root_length: usize) -> Vec<String> {
        let root = path[..root_length].to_owned();
        let mut rest: Vec<String> = path[root_length..].split('/').map(str::to_owned).collect();
        if rest.last().is_some_and(String::is_empty) {
            rest.pop();
        }
        let mut components = Vec::with_capacity(rest.len() + 1);
        components.push(root);
        components.extend(rest);
        components
    }

    /// tsc-port: getPathComponents @6.0.3
    /// tsc-hash: e90db53f4d0f370afebd35c5fee098b79f29ce0984f16a0ba684c0c7be428f14
    /// tsc-span: _tsc.js:5443-5446
    fn get_path_components(path: &str, current_directory: &str) -> Vec<String> {
        let combined = combine_paths(current_directory, path);
        let root_length = get_root_length(&combined);
        path_components(&combined, root_length)
    }

    /// tsc-port: reducePathComponents @6.0.3
    /// tsc-hash: 10c3c1d91bfab05c623f8bcd9fcdfc78bcc8bea06c7af29ec0594cbed3cfe0af
    /// tsc-span: _tsc.js:5455-5473
    ///
    /// `..` pops a kept non-`..` segment at depth > 1, is dropped
    /// outright at depth 1 under a truthy (absolute) root, and
    /// accumulates only under an empty (relative) root.
    fn reduce_path_components(components: Vec<String>) -> Vec<String> {
        let Some((root, rest)) = components.split_first() else {
            return Vec::new();
        };
        let mut reduced: Vec<String> = vec![root.clone()];
        for component in rest {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                if reduced.len() > 1 {
                    if reduced.last().is_some_and(|last| last != "..") {
                        reduced.pop();
                        continue;
                    }
                } else if !reduced[0].is_empty() {
                    continue;
                }
            }
            reduced.push(component.clone());
        }
        reduced
    }

    /// tsc-port: combinePaths @6.0.3
    /// tsc-hash: b5ac359c4863f2966ac63973b41fde20c4548b83a0671304ce14cb6c54ba8e28
    /// tsc-span: _tsc.js:5474-5486
    pub(crate) fn combine_paths(path: &str, relative: &str) -> String {
        let mut combined = if path.is_empty() {
            String::new()
        } else {
            normalize_slashes(path)
        };
        if relative.is_empty() {
            return combined;
        }
        let relative = normalize_slashes(relative);
        if combined.is_empty() || get_root_length(&relative) != 0 {
            return relative;
        }
        combined = ensure_trailing_directory_separator(&combined);
        combined.push_str(&relative);
        combined
    }

    /// tsc-port: getPathFromPathComponents @6.0.3
    /// tsc-hash: 138e19b80fb9f4ae63e7388d0dd670c3884ccf08aaceffd326a9fa2fa0611a50
    /// tsc-span: _tsc.js:5447-5451
    fn get_path_from_path_components(components: &[String]) -> String {
        let Some((root, rest)) = components.split_first() else {
            return String::new();
        };
        let root = if root.is_empty() {
            String::new()
        } else {
            ensure_trailing_directory_separator(root)
        };
        format!("{root}{}", rest.join("/"))
    }

    /// tsc-port: toFileNameLowerCase @6.0.3
    /// tsc-hash: b948cecaffc4b4ec572bfc34330f324c32237e25bf1e1b1d0da8c20f21da96db
    /// tsc-span: _tsc.js:873-876
    ///
    /// The `fileNameLowerCaseRegExp` guard (_tsc.js:871-872, hash
    /// 462d54438d5a72fc61b2613b519b8660343d466d36399eb46c2e73104b1ca186)
    /// preserves `İ`/`ı`/`ß`, ASCII lowercase, digits, and separators;
    /// only the matched runs are lowercased. DORMANT-UNWITNESSED in this
    /// rung (packet §12b): every witness replay and the whole 6a band run
    /// case-sensitive; the first case-insensitive-host band packet owns
    /// its first observation.
    fn to_file_name_lower_case(file_name: &str) -> String {
        fn is_preserved(ch: char) -> bool {
            matches!(ch,
                '\u{0130}' | '\u{0131}' | '\u{00DF}'
                | 'a'..='z' | '0'..='9'
                | '\\' | '/' | ':' | '-' | '_' | '.' | ' ')
        }
        if file_name.chars().all(is_preserved) {
            return file_name.to_owned();
        }
        let mut out = String::with_capacity(file_name.len());
        let mut run = String::new();
        for ch in file_name.chars() {
            if is_preserved(ch) {
                if !run.is_empty() {
                    out.push_str(&run.to_lowercase());
                    run.clear();
                }
                out.push(ch);
            } else {
                run.push(ch);
            }
        }
        if !run.is_empty() {
            out.push_str(&run.to_lowercase());
        }
        out
    }

    /// tsc-port: equateStringsCaseInsensitive @6.0.3
    /// tsc-hash: 1798be4a0411df11d02a3c1ab582f840d2c3d2bae6a48dc4803dabc1155e485c
    /// tsc-span: _tsc.js:905-907
    ///
    /// `toUpperCase` equality, ported as ASCII-uppercase with the packet
    /// §12a reachability note: every reachable root component on the
    /// admitted profile is `/`, so the compare short-circuits on
    /// equality; UNC roots that could carry non-ASCII are owned by the
    /// first band that admits one.
    fn equate_strings_case_insensitive(a: &str, b: &str) -> bool {
        a == b || a.eq_ignore_ascii_case(b)
    }

    /// tsc-port: getPathComponentsRelativeTo @6.0.3
    /// tsc-hash: 3b032651df66f06c4bddbf9061d733f05167fb83bce0e781f56717328c09562f
    /// tsc-span: _tsc.js:5694-5713
    ///
    /// Upstream receives `resolvePath`'d strings and re-reduces them; per
    /// the module-level equivalence argument this port reduces the
    /// combined strings directly. EVERY compared component — including
    /// component 0 — is canonicalized first (_tsc.js:5699-5700), then
    /// component 0 compares case-insensitively and later components with
    /// the case-sensitive comparer that
    /// `getRelativePathToDirectoryOrUrl` always passes.
    fn get_path_components_relative_to(
        from_combined: &str,
        to_combined: &str,
        use_case_sensitive_source_keys: bool,
    ) -> Vec<String> {
        let canonical = |component: &str| -> String {
            if use_case_sensitive_source_keys {
                component.to_owned()
            } else {
                to_file_name_lower_case(component)
            }
        };
        let from_components = reduce_path_components(get_path_components(from_combined, ""));
        let to_components = reduce_path_components(get_path_components(to_combined, ""));
        let mut start = 0usize;
        while start < from_components.len() && start < to_components.len() {
            let from_component = canonical(&from_components[start]);
            let to_component = canonical(&to_components[start]);
            let matched = if start == 0 {
                equate_strings_case_insensitive(&from_component, &to_component)
            } else {
                from_component == to_component
            };
            if !matched {
                break;
            }
            start += 1;
        }
        if start == 0 {
            return to_components;
        }
        let components = &to_components[start..];
        let mut relative = Vec::new();
        relative.push(String::new());
        for _ in start..from_components.len() {
            relative.push("..".to_owned());
        }
        relative.extend(components.iter().cloned());
        relative
    }

    /// tsc-port: getNormalizedAbsolutePath @6.0.3 (components form)
    /// tsc-hash: b61f74b787ba34aece216809c77bbf6f46565bc1f0a0af082110aacbe0bf9b0c
    /// tsc-span: _tsc.js:5493-5567
    ///
    /// The string form of the packet's recorded components-level
    /// equivalence: `from_components(reduce(components(combine(cwd,
    /// path))))`. File paths carry no trailing separator on any
    /// reachable input; the trailing-separator preservation arm of the
    /// upstream span is therefore vacuous here and not modeled.
    pub(crate) fn get_normalized_absolute_path(path: &str, current_directory: &str) -> String {
        get_path_from_path_components(&reduce_path_components(get_path_components(
            path,
            current_directory,
        )))
    }

    /// tsc-port: getSourceFilePathInNewDirWorker @6.0.3
    /// tsc-hash: 5c4d813d295ba15467286c56f0e2fc4d8b13c00694b1464d5d25c6ad189cab75
    /// tsc-span: _tsc.js:16635-16643
    ///
    /// h2-6b-m-1: the mapRoot per-file nesting arm. `new_dir_path` stays
    /// AS GIVEN (a relative mapRoot remains relative — the caller's
    /// root-length check then resolves it against the common source
    /// directory, upstream order). `common_source_directory` must carry
    /// its trailing separator (the upstream host guarantees it; the
    /// caller ensures it).
    pub(crate) fn source_file_path_in_new_dir_worker(
        file_name: &str,
        new_dir_path: &str,
        current_directory: &str,
        common_source_directory: &str,
        use_case_sensitive_source_keys: bool,
    ) -> String {
        let source_file_path = get_normalized_absolute_path(file_name, current_directory);
        let canonical = |value: &str| {
            if use_case_sensitive_source_keys {
                value.to_owned()
            } else {
                to_file_name_lower_case(value)
            }
        };
        let in_common =
            canonical(&source_file_path).starts_with(&canonical(common_source_directory));
        let suffix = if in_common {
            source_file_path[common_source_directory.len()..].to_owned()
        } else {
            source_file_path
        };
        combine_paths(new_dir_path, &suffix)
    }

    /// tsc-port: getRelativePathToDirectoryOrUrl @6.0.3
    /// tsc-hash: 702a35388f8748ee6cb70c7b7826ad08b66064005619ca2677dd90a7d8596b10
    /// tsc-span: _tsc.js:5734-5747
    pub(crate) fn get_relative_path_to_directory_or_url(
        directory_path_or_url: &str,
        relative_or_absolute_path: &str,
        current_directory: &str,
        use_case_sensitive_source_keys: bool,
        is_absolute_path_an_url: bool,
    ) -> String {
        let from_combined = combine_paths(current_directory, directory_path_or_url);
        let to_combined = combine_paths(current_directory, relative_or_absolute_path);
        let mut components = get_path_components_relative_to(
            &from_combined,
            &to_combined,
            use_case_sensitive_source_keys,
        );
        if let Some(first) = components.first_mut() {
            if is_absolute_path_an_url && is_rooted_disk_path(first) {
                let prefix = if first.starts_with('/') {
                    "file://"
                } else {
                    "file:///"
                };
                *first = format!("{prefix}{first}");
            }
        }
        get_path_from_path_components(&components)
    }
}

#[cfg(test)]
#[path = "../tests/unit/source_map/tests.rs"]
mod tests;

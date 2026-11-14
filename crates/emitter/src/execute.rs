use crate::artifact::EmitCallbackText;
use crate::PreparedEmitSource;
use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_diagnostics::{JsStr, JsString};
use tsc_program::SourceFileId;
use tsc_syntax::SourceFile;
use tsc_types::tracing::{Args as TraceArgs, Phase};
use tsc_types::{CompilerOptions, ScriptTarget};

use crate::builtins::get_script_transformers_for_source;
use crate::declarations::{
    emit_declaration_unit, get_declaration_diagnostics, PlanDeclarationPaths,
};
use crate::{
    create_printer, transform_nodes, EmitArtifact, EmitContractViolation, EmitFailure, EmitHost,
    EmitOutcome, EmitPreflight, EmitResolver, EmitResolverError, EmitRoot, EmitSelection,
    EmitTextMetadata, EmitWriteDisposition, NewLineKind, OutputSink, PrintRequest, PrinterOptions,
    SourceFileTextMode, SourceMapObservation, SourceMapRecordingInputs, TransformArena,
    TransformError, TransformRoot,
};

const MODULE_NONE: i32 = 0;
const MODULE_COMMON_JS: i32 = 1;
const MODULE_AMD: i32 = 2;
const MODULE_UMD: i32 = 3;
const MODULE_SYSTEM: i32 = 4;
const MODULE_ES2015: i32 = 5;
const MODULE_ES2020: i32 = 6;
const MODULE_ES2022: i32 = 7;
const MODULE_ES_NEXT: i32 = 99;
const MODULE_NODE16: i32 = 100;
const MODULE_NODE18: i32 = 101;
const MODULE_NODE20: i32 = 102;
const MODULE_NODE_NEXT: i32 = 199;
const MODULE_PRESERVE: i32 = 200;

/// The four public diagnostic getter streams consumed by
/// `handleNoEmitOptions`, kept separate so output-preflight diagnostics can
/// join the options bucket without disturbing cross-bucket order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitDiagnosticGate {
    options: DiagnosticList,
    syntactic: DiagnosticList,
    global: DiagnosticList,
    semantic: DiagnosticList,
    declaration: Option<DiagnosticList>,
}

impl EmitDiagnosticGate {
    pub fn new(
        options: DiagnosticList,
        syntactic: DiagnosticList,
        global: DiagnosticList,
        semantic: DiagnosticList,
    ) -> Self {
        Self {
            options,
            syntactic,
            global,
            semantic,
            declaration: None,
        }
    }

    /// Supply the Program-owned cached getter result only after the four
    /// earlier streams are empty. Ordinary callers without a retained Program
    /// diagnostic cache keep the existing emitter-owned getter branch.
    pub fn with_declaration_diagnostics(mut self, diagnostics: DiagnosticList) -> Self {
        self.declaration = Some(diagnostics);
        self
    }

    /// Whether `begin_emit_files` would run the whole-Program declaration
    /// getter for `options`: noEmitOnError with declaration output, no
    /// cached result, and no earlier stream (options with `preflight`,
    /// syntactic, global, semantic) already blocking the emit. A coordinator
    /// with several checker sessions gathers that result itself.
    pub fn wants_declaration_diagnostics(
        &self,
        options: &CompilerOptions,
        preflight: &[Diagnostic],
    ) -> bool {
        self.declaration.is_none()
            && options.no_emit_on_error == Some(true)
            && (options.declaration == Some(true) || options.composite == Some(true))
            && self.collect_with_preflight(preflight).is_empty()
    }

    fn collect_with_preflight(&self, preflight: &[Diagnostic]) -> DiagnosticList {
        let mut options = self.options.clone();
        options.extend_from_slice(preflight);
        sort_and_dedupe_diagnostics(&mut options);
        let capacity =
            options.len() + self.syntactic.len() + self.global.len() + self.semantic.len();
        let mut diagnostics = Vec::with_capacity(capacity);
        diagnostics.extend(options);
        diagnostics.extend(self.syntactic.iter().cloned());
        diagnostics.extend(self.global.iter().cloned());
        diagnostics.extend(self.semantic.iter().cloned());
        diagnostics
    }
}

/// Reject a compiler option this emitter does not implement before output
/// planning, checker-to-emitter borrowing, or sink dispatch.
pub fn validate_emit_options(options: &CompilerOptions) -> Result<(), EmitFailure> {
    validate_options(options, EmitOperation::Files)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum EmitOperation {
    Files,
    DeclarationDiagnostics,
    ForcedDeclarations,
    ForcedJavaScript,
}

fn validate_options(
    options: &CompilerOptions,
    operation: EmitOperation,
) -> Result<(), EmitFailure> {
    let target = options.emit_script_target();
    // JSON is an internal parser mode, not an unknown future JS target.
    // Filename-independent source-kind facts are not yet represented.
    if target < ScriptTarget::ES5 || target == ScriptTarget::JSON {
        return unsupported("target");
    }
    if !matches!(
        options.emit_module_kind(),
        MODULE_NONE
            | MODULE_PRESERVE
            | MODULE_ES_NEXT
            | MODULE_COMMON_JS
            | MODULE_AMD
            | MODULE_UMD
            | MODULE_SYSTEM
            | MODULE_ES2015
            | MODULE_ES2020
            | MODULE_ES2022
            | MODULE_NODE16
            | MODULE_NODE18
            | MODULE_NODE20
            | MODULE_NODE_NEXT
    ) {
        return unsupported("module");
    }
    if !matches!(options.new_line, None | Some(0 | 1)) {
        return unsupported("newLine");
    }

    {
        let (active, name) = (
            operation == EmitOperation::Files && options.no_emit == Some(true),
            "noEmit",
        );
        if active {
            return unsupported(name);
        }
    }
    if !matches!(options.jsx, None | Some(1..=5)) {
        return unsupported("jsx");
    }
    Ok(())
}

/// Validate the options and the source families of an emit request before
/// the checker constructs an emit resolver.
pub fn validate_emit_request(host: &dyn EmitHost) -> Result<(), EmitFailure> {
    validate_request(host, EmitOperation::Files)
}

pub fn validate_declaration_diagnostics_request(host: &dyn EmitHost) -> Result<(), EmitFailure> {
    validate_request(host, EmitOperation::DeclarationDiagnostics)
}

pub fn validate_forced_declaration_request(host: &dyn EmitHost) -> Result<(), EmitFailure> {
    validate_request(host, EmitOperation::ForcedDeclarations)
}

fn validate_request(host: &dyn EmitHost, operation: EmitOperation) -> Result<(), EmitFailure> {
    let options = host.compiler_options();
    validate_options(options, operation)?;
    for source_id in host.source_file_ids() {
        let source = host.source_file(*source_id).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(*source_id),
        ))?;
        let eligible = match operation {
            EmitOperation::ForcedDeclarations => {
                crate::plan::source_file_may_emit_forced_declaration(source, host)
            }
            EmitOperation::ForcedJavaScript => {
                crate::plan::source_file_may_emit_forced_javascript(source)
            }
            EmitOperation::Files | EmitOperation::DeclarationDiagnostics => {
                crate::plan::source_file_may_be_emitted(source, host)
            }
        };
        if !eligible {
            continue;
        }
        let name = source.path();
        let is_typescript = crate::builtins::has_ascii_file_suffix(name, ".ts")
            || crate::builtins::has_ascii_file_suffix(name, ".mts")
            || crate::builtins::has_ascii_file_suffix(name, ".cts")
            || crate::builtins::has_ascii_file_suffix(name, ".tsx");
        let is_javascript = options.allow_js
            && (crate::builtins::has_ascii_file_suffix(name, ".js")
                || crate::builtins::has_ascii_file_suffix(name, ".mjs")
                || crate::builtins::has_ascii_file_suffix(name, ".cjs")
                || crate::builtins::has_ascii_file_suffix(name, ".jsx"));
        let is_json = crate::builtins::has_ascii_file_suffix(name, ".json");
        if is_json && !options.resolve_json_module_effective() {
            return unsupported("resolveJsonModule");
        }
        // allowNonTsExtensions (transpile routes): any other extension was
        // admitted as a TypeScript-kind root by the Program loader.
        let is_other_admitted = options.allow_non_ts_extensions == Some(true)
            // A content mapper's output, whatever its file's extension.
            || source.content_mapped();
        if !(is_typescript || is_javascript || is_json || is_other_admitted)
            || crate::builtins::has_ascii_file_suffix(name, ".d.ts")
            || crate::builtins::has_ascii_file_suffix(name, ".d.mts")
            || crate::builtins::has_ascii_file_suffix(name, ".d.cts")
        {
            return Err(EmitFailure::UnsupportedSourceExtension {
                path: source.path().to_owned(),
            });
        }
    }
    Ok(())
}

fn unsupported<T>(option: &'static str) -> Result<T, EmitFailure> {
    Err(EmitFailure::UnsupportedCompilerOption { option })
}

/// tsc-port: emitFiles @6.0.3
/// tsc-hash: 62e93c3a8e9e2840b759bbaa0fa6de5e548ebd565748dbbddb47a933a1cf442c
/// tsc-span: _tsc.js:116530-116858
///
/// Each output unit writes JavaScript before transforming and writing its
/// declaration. Later failures retain the callbacks already delivered, while
/// map callbacks precede their text and listings preserve the opposite order.
pub fn emit_files(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
    sink: &mut dyn OutputSink,
) -> Result<EmitOutcome, EmitFailure> {
    let session =
        match begin_emit_files(Some(resolver), host, &preflight, selection, diagnostic_gate)? {
            EmitFilesStart::Blocked(outcome) => return Ok(*outcome),
            EmitFilesStart::Ready(session) => session,
        };
    let units = (0..preflight.plan().units().len()).collect::<Vec<_>>();
    let emissions = emit_planned_units(resolver, host, &preflight, &units, Some(sink))
        .map_err(|error| error.failure)?;
    finish_emit_files(session, emissions, sink)
}

/// h2-6a-m-2 §8-A.1 harness-print bridge: the production
/// plan → transform → print pipeline of `emit_files`
/// WITHOUT artifacts, sinks, or the option
/// preflight — the replay suite injects a `SourceMapRecordingInputs`
/// per unit and byte-compares the returned text and generator against
/// the frozen witnesses. No production caller exists; real emits keep
/// every refusal lane.
#[doc(hidden)]
pub fn print_script_units_with_recording_for_harness(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    recording_inputs_for: &dyn Fn(JsStr<'_>) -> Option<crate::SourceMapRecordingInputs>,
) -> Result<Vec<(JsString, crate::PrintedText)>, EmitFailure> {
    let options = host.compiler_options();
    let new_line = match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        None | Some(1) => NewLineKind::LineFeed,
        Some(_) => return unsupported("newLine"),
    };
    let mut printer = create_printer(
        PrinterOptions::new(new_line)
            .with_remove_comments(options.remove_comments == Some(true))
            .with_no_emit_helpers(options.no_emit_helpers == Some(true))
            .with_import_helpers(options.import_helpers == Some(true))
            .with_target(options.emit_script_target())
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let mut printed_units = Vec::new();
    for unit in preflight.plan().units() {
        let EmitRoot::SourceFile(source_id) = unit.root();
        let Some(javascript_path) = unit.paths().javascript_path() else {
            continue;
        };
        let source = host.source_file(*source_id).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(*source_id),
        ))?;
        let syntax = source.syntax().ok_or(EmitFailure::Contract(
            EmitContractViolation::CheckedSyntaxUnavailable(*source_id),
        ))?;
        let mut arena = TransformArena::new();
        let transform_source = arena.add_source(syntax, Some(*source_id));
        let transformers = get_script_transformers_for_source(options, resolver, host, *source_id)?;
        let mut transformation = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(transform_source)],
            transformers,
            false,
        )?;
        let printed = printer.print(
            &mut transformation,
            PrintRequest::SourceFile(transform_source),
            recording_inputs_for(javascript_path),
        )?;
        printed_units.push((javascript_path.to_owned(), printed));
    }
    Ok(printed_units)
}

/// Compiler-owned entry which carries one observer from request construction
/// through callback completion.
#[doc(hidden)]
/// tsc-port: getSourceMappingURL/encodeURI @6.0.3
/// tsc-hash: ef8e1bcbc2559f9d7ee1de030c89a049c5f5330e48498632761d137b14b0277a
/// tsc-span: _tsc.js:116826-116857
///
/// The URL comment escapes the map basename with the JS `encodeURI`
/// builtin: ASCII alphanumerics and `;,/?:@&=+$-_.!~*'()#` pass
/// through, every other scalar percent-escapes its UTF-8 bytes
/// (uppercase hex). The witness `path-shapes--positive-percent-name`
/// case pins the byte behavior.
fn encode_uri<'a>(text: impl Into<JsStr<'a>>) -> Result<String, EmitFailure> {
    let text = text.into();
    let text = text
        .as_str()
        .ok_or_else(|| EmitFailure::MalformedSourceMapUrl {
            path: text.to_owned(),
        })?;
    const KEEP: &[u8] = b";,/?:@&=+$-_.!~*'()#";
    let mut encoded = String::with_capacity(text.len());
    let mut buffer = [0_u8; 4];
    for scalar in text.chars() {
        if scalar.is_ascii() && (scalar.is_ascii_alphanumeric() || KEEP.contains(&(scalar as u8))) {
            encoded.push(scalar);
        } else {
            for byte in scalar.encode_utf8(&mut buffer).as_bytes() {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    Ok(encoded)
}

/// tsc-port: getSourceRoot @6.0.3
/// tsc-hash: 13d496ce2a87d1659e7244bf582daf340c1be47e0b490f683c2790c6b3a553c1
/// tsc-span: _tsc.js:116808-116811
///
/// The map's `sourceRoot` FIELD form: `normalizeSlashes(sourceRoot||"")`
/// with a trailing directory separator ensured iff nonempty (h2-6b.md
/// §4.2). `""` (still emitted as a key) whenever the option is absent —
/// the H2.6a floor value.
#[doc(hidden)]
pub fn source_root_field(options: &CompilerOptions) -> JsString {
    let normalized = crate::source_map::paths::normalize_slashes(
        options
            .source_root
            .as_ref()
            .map(JsString::as_js)
            .unwrap_or("".into()),
    );
    if normalized.is_empty() {
        normalized
    } else {
        crate::source_map::paths::ensure_trailing_directory_separator(&normalized)
    }
}

fn normalized_display(path: JsStr<'_>) -> JsString {
    crate::source_map::paths::normalize_slashes(path)
}

/// The exact host projection the root/URL lanes consume (h2-6b-m-1).
/// Narrow on purpose: the suites replay the lanes with witness-case
/// values and no host mock, and the production caller builds it from
/// `EmitHost` once per unit.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct MapLaneInputs {
    /// `host.getCommonSourceDirectory()` with normalized slashes and the
    /// trailing separator the upstream host guarantees.
    pub common_source_directory: JsString,
    pub current_directory: JsString,
    pub use_case_sensitive_source_keys: bool,
}

pub(crate) fn map_lane_inputs(host: &dyn EmitHost) -> MapLaneInputs {
    MapLaneInputs {
        common_source_directory: crate::source_map::paths::ensure_trailing_directory_separator(
            &crate::source_map::paths::normalize_slashes(host.common_source_directory()),
        ),
        current_directory: normalized_display(host.current_directory()),
        use_case_sensitive_source_keys: host.use_case_sensitive_file_names(),
    }
}

fn directory_and_basename(normalized: JsStr<'_>) -> (JsStr<'_>, JsStr<'_>) {
    let root = crate::source_map::paths::get_root_length(normalized);
    if root == normalized.as_bytes().len() {
        return (normalized, "".into());
    }
    let slash = normalized.as_bytes().iter().rposition(|&byte| byte == b'/');
    let directory_end = root.max(slash.unwrap_or(0));
    let base_start = root.max(slash.map_or(0, |offset| offset + 1));
    (
        normalized
            .split_at_byte(directory_end)
            .expect("directory boundary is ASCII")
            .0,
        normalized
            .split_at_byte(base_start)
            .expect("basename boundary is ASCII")
            .1,
    )
}

fn trim_directory_separators(path: JsStr<'_>) -> JsStr<'_> {
    let end = path
        .as_bytes()
        .iter()
        .rposition(|&byte| byte != b'/')
        .map_or(0, |offset| offset + 1);
    path.split_at_byte(end)
        .expect("trailing separators are ASCII")
        .0
}

/// tsc-port: getSourceMapDirectory @6.0.3
/// tsc-hash: 5d1e1b69c02c83fe1f02760b408962879736368ad481a3702c120c323c06840d
/// tsc-span: _tsc.js:116812-116825
///
/// The `sourcesDirectoryPath` three-lane selection (h2-6b.md §4.2):
/// `sourceRoot` → the common source directory; `mapRoot` → the
/// normalized root, per-file nested when a source file exists, resolved
/// against the common source directory when relative; otherwise the
/// JavaScript output directory.
pub(crate) fn source_map_directory(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    javascript_path: JsStr<'_>,
    source_path: Option<JsStr<'_>>,
) -> JsString {
    use crate::source_map::paths;
    if options
        .source_root
        .as_ref()
        .is_some_and(|root| !root.is_empty())
    {
        return trim_directory_separators(lane.common_source_directory.as_js()).to_owned();
    }
    if let Some(map_root) = options.map_root.as_ref().filter(|root| !root.is_empty()) {
        let mut source_map_dir = paths::normalize_slashes(map_root);
        // per-file nesting (getSourceFilePathInNewDir): the relative
        // mapRoot stays relative through the worker; the root-length
        // check below then resolves it (upstream order).
        if let Some(source_path) = source_path {
            let nested = paths::source_file_path_in_new_dir_worker(
                &normalized_display(source_path),
                &source_map_dir,
                &lane.current_directory,
                &lane.common_source_directory,
                lane.use_case_sensitive_source_keys,
            );
            source_map_dir = directory_and_basename(nested.as_js()).0.to_owned();
        }
        if paths::get_root_length(&source_map_dir) == 0 {
            source_map_dir = paths::combine_paths(
                trim_directory_separators(lane.common_source_directory.as_js()),
                &source_map_dir,
            );
        }
        return source_map_dir;
    }
    directory_and_basename(normalized_display(javascript_path).as_js())
        .0
        .to_owned()
}

/// `createSourceMapGenerator` inputs for one script unit (upstream
/// printSourceFileOrBundle 116751-116757), generalized by h2-6b-m-1 to
/// the full root-lane selection. At the production floor (the four 6b
/// options refused) every lane input degenerates to the H2.6a values:
/// `sourceRoot` = `""`, `sourcesDirectoryPath` = the js output
/// directory, `inline_sources` = false.
#[doc(hidden)]
pub fn source_map_recording_inputs_for(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    javascript_path: JsStr<'_>,
    source_path: JsStr<'_>,
) -> SourceMapRecordingInputs {
    source_map_recording_inputs_for_output(lane, options, javascript_path, Some(source_path))
}

pub(crate) fn source_map_recording_inputs_for_output(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    javascript_path: JsStr<'_>,
    source_path: Option<JsStr<'_>>,
) -> SourceMapRecordingInputs {
    let normalized = normalized_display(javascript_path);
    let (_, basename) = directory_and_basename(normalized.as_js());
    SourceMapRecordingInputs {
        file: basename.into(),
        source_root: source_root_field(options),
        sources_directory_path: source_map_directory(lane, options, javascript_path, source_path),
        current_directory: lane.current_directory.clone(),
        use_case_sensitive_source_keys: lane.use_case_sensitive_source_keys,
        inline_sources: options.inline_sources == Some(true),
        original_positions: None,
    }
}

/// tsc-port: sys.base64encode/convertToBase64 @6.0.3
/// tsc-hash: d5b3a2fbf7db940bd61f9880c1c39156a9828158efcf36759186810b5137d7c5
/// tsc-span: _tsc.js:5007-5007
///
/// `Buffer.from(input).toString("base64")` over the UTF-8 map text:
/// standard alphabet, `=` padding, no line breaks (h2-6b.md §4.3). The
/// VLQ `base64FormatEncode` in `source_map.rs` is a different, unpadded
/// single-digit use and stays untouched.
#[doc(hidden)]
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        encoded.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        encoded.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    encoded
}

/// tsc-port: getSourceMappingURL @6.0.3
/// tsc-hash: ef8e1bcbc2559f9d7ee1de030c89a049c5f5330e48498632761d137b14b0277a
/// tsc-span: _tsc.js:116826-116857
///
/// The four-way URL selection (h2-6b.md §4.3): inline data URI (no
/// encodeURI), mapRoot rooted (absolute encodeURI'd URL), mapRoot
/// relative (resolved against the common source directory then
/// relativized FROM the js directory with the URL arm), and the
/// basename default (the H2.6a floor lane — the only one reachable in
/// production until m-2).
#[doc(hidden)]
pub fn source_mapping_url(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    map_text: &str,
    javascript_path: JsStr<'_>,
    map_path: Option<JsStr<'_>>,
    source_path: JsStr<'_>,
) -> Result<String, EmitFailure> {
    source_mapping_url_for_output(
        lane,
        options,
        map_text,
        javascript_path,
        map_path,
        Some(source_path),
    )
}

pub(crate) fn source_mapping_url_for_output(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    map_text: &str,
    javascript_path: JsStr<'_>,
    map_path: Option<JsStr<'_>>,
    source_path: Option<JsStr<'_>>,
) -> Result<String, EmitFailure> {
    use crate::source_map::paths;
    if options.inline_source_map == Some(true) {
        return Ok(format!(
            "data:application/json;base64,{}",
            base64_encode(map_text.as_bytes())
        ));
    }
    // Debug.checkDefined(sourceMapFilePath): the external lanes always
    // plan a map path; its absence is a contract violation.
    let map_path = map_path.ok_or(EmitFailure::Contract(
        EmitContractViolation::SourceMapRecordingUnavailable,
    ))?;
    let normalized_map = normalized_display(map_path);
    let (_, map_basename) = directory_and_basename(normalized_map.as_js());
    if let Some(map_root) = options.map_root.as_ref().filter(|root| !root.is_empty()) {
        let mut source_map_dir = paths::normalize_slashes(map_root);
        if let Some(source_path) = source_path {
            let nested = paths::source_file_path_in_new_dir_worker(
                &normalized_display(source_path),
                &source_map_dir,
                &lane.current_directory,
                &lane.common_source_directory,
                lane.use_case_sensitive_source_keys,
            );
            source_map_dir = directory_and_basename(nested.as_js()).0.to_owned();
        }
        if paths::get_root_length(&source_map_dir) == 0 {
            source_map_dir = paths::combine_paths(
                trim_directory_separators(lane.common_source_directory.as_js()),
                &source_map_dir,
            );
            let normalized_js = normalized_display(javascript_path);
            let (js_directory, _) = directory_and_basename(normalized_js.as_js());
            return encode_uri(&paths::get_relative_path_to_directory_or_url(
                js_directory,
                &paths::combine_paths(&source_map_dir, map_basename),
                &lane.current_directory,
                lane.use_case_sensitive_source_keys,
                true,
            ));
        }
        return encode_uri(&paths::combine_paths(&source_map_dir, map_basename));
    }
    encode_uri(map_basename)
}

pub(crate) struct ResolverGlobalNameOracle<'resolver>(pub(crate) &'resolver dyn EmitResolver);

impl crate::GlobalNameOracle for ResolverGlobalNameOracle<'_> {
    fn has_global_name(&self, name: &str) -> Result<bool, crate::EmitResolverError> {
        self.0.has_global_name(name)
    }
}

/// Mount the exact ordered members of one planned root. Cross-file declaration
/// lookup mounts the remaining Program sources separately without widening it.
/// Prepare `source`'s emit copy ahead of its emit: the detached clone with
/// its parse-time transform flags classified, as the JavaScript emit would
/// build it first thing. A driver runs this on a spare thread while the
/// checkers work and hands the copy back through
/// [`EmitHost::take_prepared_source`]; the copy depends on the parsed syntax
/// only, never on a checker.
pub fn prepare_emit_source(
    source: &SourceFile,
    program_source: Option<SourceFileId>,
) -> Result<PreparedEmitSource, TransformError> {
    let mut arena = TransformArena::new();
    let id = arena.add_source(source, program_source);
    crate::builtins::classify_prepared_source(&mut arena, id)?;
    crate::builtins::collect_prepared_source_censuses(&arena, id)?;
    Ok(arena.into_prepared_source(id))
}

pub(crate) fn mount_emit_root(
    arena: &mut TransformArena,
    host: &dyn EmitHost,
    root: &EmitRoot,
    take_prepared: bool,
) -> Result<TransformRoot, EmitFailure> {
    let mut sources = Vec::with_capacity(root.source_files().len());
    for &source in root.source_files() {
        let file = host.source_file(source).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(source),
        ))?;
        let syntax = file.syntax().ok_or(EmitFailure::Contract(
            EmitContractViolation::CheckedSyntaxUnavailable(source),
        ))?;
        let node_count = syntax.arena.nodes().len();
        // A copy prepared while the checkers ran replaces the clone and the
        // first classification; it must have come from this very syntax.
        let prepared = take_prepared
            .then(|| host.take_prepared_source(source))
            .flatten()
            .filter(|prepared| prepared.matches(syntax));
        sources.push(match prepared {
            Some(prepared) => arena.add_prepared_source(prepared),
            None => arena.add_source(syntax, Some(source)),
        });
        // The transforms attach metadata (original links, emit flags) to a
        // sizeable share of a source's nodes; one reservation replaces the
        // map's repeated rehashing (2% of the emit profile).
        arena.reserve_metadata(node_count / 4);
    }
    Ok(TransformRoot::SourceFile(sources[0]))
}

pub(crate) fn transformed_source_paths(
    result: &crate::TransformationResult<'_>,
    root: &TransformRoot,
    host: &dyn EmitHost,
) -> Result<Vec<JsString>, EmitFailure> {
    let TransformRoot::SourceFile(source) = root;
    std::slice::from_ref(source)
        .iter()
        .map(|&source| {
            let source_id = result.arena().source(source)?.program_source().ok_or(
                crate::TransformError::MissingProgramSource(result.arena().root(source)?),
            )?;
            host.source_file(source_id)
                .map(|file| file.path().to_owned())
                .ok_or(EmitFailure::Contract(
                    EmitContractViolation::PlannedSourceMissing(source_id),
                ))
        })
        .collect()
}

/// The coordinator-side state of one emitFiles execution between its gate
/// and its sink writes.
#[derive(Debug)]
pub struct EmitFilesSession {
    emitted_files_enabled: bool,
    map_options_enabled: bool,
}

/// How emitFiles proceeds after validation and handleNoEmitOptions.
#[derive(Debug)]
pub enum EmitFilesStart {
    /// noEmitOnError found diagnostics: the finished outcome, no unit runs.
    Blocked(Box<EmitOutcome>),
    /// The planned units may be transformed and printed.
    Ready(EmitFilesSession),
}

/// The products of one planned output unit. Produced by whichever thread
/// owns an emit resolver for the unit's source; consumed in plan order by
/// [`finish_emit_files`].
#[derive(Debug)]
pub struct UnitEmission {
    unit: usize,
    /// JavaScript (map first) then declaration (map first) artifacts of an
    /// ordinary source root, retained for the finishing write in plan
    /// order. Bundle roots that wrote through a sink leave this empty.
    artifacts: Vec<EmitArtifact>,
    diagnostics: DiagnosticList,
    map_observations: Vec<SourceMapObservation>,
    /// Paths a bundle root already wrote through the supplied sink.
    written_paths: std::collections::BTreeSet<JsString>,
    javascript_path: Option<JsString>,
    javascript_map_path: Option<JsString>,
    declaration_path: Option<JsString>,
    declaration_map_path: Option<JsString>,
    emit_skipped: bool,
}

impl UnitEmission {
    /// The plan index of the unit these products belong to.
    pub fn unit(&self) -> usize {
        self.unit
    }
}

/// A failure while transforming or printing one planned unit, with the
/// unit's plan index so that concurrent producers report the earliest unit.
#[derive(Debug)]
pub struct UnitEmitError {
    pub unit: usize,
    pub failure: EmitFailure,
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<UnitEmission>();
    assert_send::<UnitEmitError>();
};

/// Phase 1 of emitFiles: request validation, option observation and
/// handleNoEmitOptions (_tsc.js:125641-125668). `resolver` is consulted only
/// when the noEmitOnError gate must run the declaration transform for its
/// diagnostics; a caller without one resolver for the whole Program routes
/// that configuration through [`emit_files`].
pub fn begin_emit_files(
    resolver: Option<&dyn EmitResolver>,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
) -> Result<EmitFilesStart, EmitFailure> {
    validate_emit_request(host)?;
    let options = host.compiler_options();
    preflight.plan().validate_supported_shape()?;
    if preflight.plan().selection() != selection {
        return Err(EmitFailure::Unsupported(
            crate::UnsupportedEmitFeature::TargetedSelection,
        ));
    }

    let emitted_files_enabled = host.collects_emitted_files();
    // tsc-port: handleNoEmitOptions @6.0.3
    // tsc-hash: dd8ed6d22974cbe8efc5097db50ffc3bf3aeabd1de0ca9f3a4e0878d3aa87de1
    // tsc-span: _tsc.js:125641-125668
    if options.no_emit_on_error == Some(true) {
        let mut diagnostics = diagnostic_gate.collect_with_preflight(preflight.diagnostics());
        if diagnostics.is_empty()
            && (options.declaration == Some(true) || options.composite == Some(true))
        {
            // TypeScript checks declarations across the whole program before
            // any JavaScript emit, even for a targeted emit request.
            if let Some(cached) = &diagnostic_gate.declaration {
                diagnostics.extend_from_slice(cached);
            } else {
                let resolver = resolver.ok_or(EmitFailure::Contract(
                    EmitContractViolation::ProgramResolverRequired,
                ))?;
                let declaration_paths: &PlanDeclarationPaths = preflight.declaration_paths(host);
                for source in crate::get_source_files_to_emit(host, EmitSelection::WholeProgram)? {
                    diagnostics.extend(get_declaration_diagnostics(
                        resolver,
                        host,
                        declaration_paths,
                        source,
                    )?);
                }
            }
            sort_and_dedupe_diagnostics(&mut diagnostics);
        }
        if !diagnostics.is_empty() {
            return Ok(EmitFilesStart::Blocked(Box::new(EmitOutcome::new(
                diagnostics,
                true,
                emitted_files_enabled.then(Vec::new),
                None,
            ))));
        }
    }

    // The printer options are validated before any unit runs, exactly where
    // the single-resolver execution validated them.
    new_line_kind(options)?;
    // sourceMapDataList is allocated iff a map option is on (116532):
    // `sourceMap || inlineSourceMap || getAreDeclarationMapsEnabled(options)`.
    let map_options_enabled = javascript_map_options_enabled(options)
        || (options.declaration_map == Some(true)
            && (options.declaration == Some(true) || options.composite == Some(true)));
    Ok(EmitFilesStart::Ready(EmitFilesSession {
        emitted_files_enabled,
        map_options_enabled,
    }))
}

fn new_line_kind(options: &CompilerOptions) -> Result<NewLineKind, EmitFailure> {
    match options.new_line {
        Some(0) => Ok(NewLineKind::CarriageReturnLineFeed),
        None | Some(1) => Ok(NewLineKind::LineFeed),
        Some(_) => unsupported("newLine"),
    }
}

fn javascript_map_options_enabled(options: &CompilerOptions) -> bool {
    options.source_map == Some(true) || options.inline_source_map == Some(true)
}

/// Phase 2 of emitFiles: transform and print the planned `units` (plan
/// indices, ascending) with one resolver. Artifacts of ordinary source roots
/// are returned for [`finish_emit_files`] to write in plan order; a bundle
/// root writes through `sink` at the same points as emitJsFileOrBundle /
/// emitDeclarationFileOrBundle when a sink is supplied.
/// tsrs-native: the per-resolver body of the whole-Program emit; a checker
/// shard runs it for the units of its own files.
/// Whether `TSRS_FILE_TRACE` is set: the per-unit development trace.
fn file_trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("TSRS_FILE_TRACE").is_some())
}

/// One planned unit to emit and which of its members (tsgo `EmitOnly`: an
/// incremental program re-emits only the JavaScript or only the
/// declaration of a file whose other member is up to date).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnitEmitRequest {
    pub unit: usize,
    pub javascript: bool,
    pub declaration: bool,
}

impl UnitEmitRequest {
    pub const fn whole(unit: usize) -> Self {
        Self {
            unit,
            javascript: true,
            declaration: true,
        }
    }
}

pub fn emit_planned_units(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    units: &[usize],
    sink: Option<&mut dyn OutputSink>,
) -> Result<Vec<UnitEmission>, UnitEmitError> {
    let requests = units
        .iter()
        .map(|&unit| UnitEmitRequest::whole(unit))
        .collect::<Vec<_>>();
    emit_planned_units_with_kinds(resolver, host, preflight, &requests, sink)
}

/// [`emit_files`] over a subset of the plan's units with the members each
/// request names (tsgo `emitFilesIncremental`: the pending files, each with
/// its pending kind). The preflight covers the whole program; the listing
/// and the diagnostics cover the units emitted.
pub fn emit_planned_files(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
    sink: &mut dyn OutputSink,
    requests: &[UnitEmitRequest],
) -> Result<EmitOutcome, EmitFailure> {
    let session =
        match begin_emit_files(Some(resolver), host, &preflight, selection, diagnostic_gate)? {
            EmitFilesStart::Blocked(outcome) => return Ok(*outcome),
            EmitFilesStart::Ready(session) => session,
        };
    let emissions = emit_planned_units_with_kinds(resolver, host, &preflight, requests, Some(sink))
        .map_err(|error| error.failure)?;
    finish_emit_files(session, emissions, sink)
}

/// [`emit_planned_units`] with the members each request names.
pub fn emit_planned_units_with_kinds(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    requests: &[UnitEmitRequest],
    sink: Option<&mut dyn OutputSink>,
) -> Result<Vec<UnitEmission>, UnitEmitError> {
    PlannedUnitEmitter::new(resolver, host, preflight).emit(requests, sink)
}

struct PendingUnitEmission {
    emission: UnitEmission,
    javascript: Option<JavaScriptWork>,
    declaration: Option<crate::declarations::DeclarationWork>,
    _credit: Option<crate::print_pool::PrintCredit>,
    trace: Option<(std::time::Instant, f64, String)>,
    _span: Option<tsc_types::tracing::Span>,
}

impl PendingUnitEmission {
    fn is_ready(&mut self) -> bool {
        self.javascript
            .as_mut()
            .is_none_or(JavaScriptWork::is_ready)
            && self.declaration.as_mut().is_none_or(|work| work.is_ready())
    }

    fn has_pending_print(&self) -> bool {
        self.javascript
            .as_ref()
            .is_some_and(|work| !work.is_complete())
            || self
                .declaration
                .as_ref()
                .is_some_and(|work| work.is_pending())
    }

    fn has_known_declaration_failure(&self) -> bool {
        self.declaration
            .as_ref()
            .is_some_and(|work| work.has_known_failure())
    }

    fn finish(
        mut self,
        sink: &mut Option<&mut dyn OutputSink>,
        eager_source_roots: bool,
    ) -> Result<UnitEmission, UnitEmitError> {
        let attach = |failure| UnitEmitError {
            unit: self.emission.unit,
            failure,
        };
        if let Some(work) = self.javascript.take() {
            let (products, credit) = work.finish();
            self._credit = credit;
            let mut products = products.map_err(attach)?;
            // The owner may have prepared declarations while the JavaScript
            // printed. Publish neither until the unit's JS result is known.
            if eager_source_roots {
                if let Some(sink) = sink.as_deref_mut() {
                    self.emission.written_paths.extend(write_artifacts(
                        std::mem::take(&mut products.artifacts),
                        sink,
                        &mut self.emission.diagnostics,
                    ));
                }
            }
            products.artifacts.append(&mut self.emission.artifacts);
            self.emission.artifacts = products.artifacts;
            products
                .map_observations
                .append(&mut self.emission.map_observations);
            self.emission.map_observations = products.map_observations;
        }
        // A JavaScript failure wins over the same unit's declaration.
        if let Some(declaration) = self.declaration.take() {
            let declaration = declaration.finish().map_err(attach)?;
            self.emission.emit_skipped |= declaration.decl_blocked;
            self.emission.diagnostics.extend(declaration.diagnostics);
            if let Some(observation) = declaration.map_observation {
                self.emission.map_observations.push(observation);
            }
            if let Some(map) = declaration.map_artifact {
                self.emission.declaration_map_path = Some(map.path().to_owned());
                self.emission.artifacts.push(map);
            }
            if let Some(artifact) = declaration.artifact {
                self.emission.artifacts.push(artifact);
            }
        }
        if eager_source_roots {
            if let Some(sink) = sink.as_deref_mut() {
                self.emission.written_paths.extend(write_artifacts(
                    std::mem::take(&mut self.emission.artifacts),
                    sink,
                    &mut self.emission.diagnostics,
                ));
            }
        }
        if let Some((started, at, path)) = self.trace {
            eprintln!(
                "[unit] ms={:.3} at={:.1} js={} decl={} {path}",
                started.elapsed().as_secs_f64() * 1e3,
                at,
                self.emission.javascript_path.is_some(),
                self.emission.declaration_path.is_some()
            );
        }
        Ok(self.emission)
    }
}

#[derive(Clone, Copy)]
struct UnitEmitSettings {
    new_line: NewLineKind,
    javascript_maps: bool,
    eager_source_roots: bool,
    trace: bool,
}

/// One implementation of the owner-side emit sequence. A print pool changes
/// only where eligible prepared JavaScript finishes; absent spare workers,
/// every stage completes in place.
#[doc(hidden)]
pub struct PlannedUnitEmitter<'a> {
    resolver: &'a dyn EmitResolver,
    host: &'a dyn EmitHost,
    preflight: &'a EmitPreflight,
    print_pool: Option<&'a crate::print_pool::EmitPrintPool>,
}

impl<'a> PlannedUnitEmitter<'a> {
    pub fn new(
        resolver: &'a dyn EmitResolver,
        host: &'a dyn EmitHost,
        preflight: &'a EmitPreflight,
    ) -> Self {
        Self {
            resolver,
            host,
            preflight,
            print_pool: None,
        }
    }

    pub fn with_print_pool(mut self, pool: Option<&'a crate::print_pool::EmitPrintPool>) -> Self {
        self.print_pool = pool;
        self
    }

    pub fn emit(
        &self,
        requests: &[UnitEmitRequest],
        mut sink: Option<&mut dyn OutputSink>,
    ) -> Result<Vec<UnitEmission>, UnitEmitError> {
        let options = self.host.compiler_options();
        let first = requests.first().map_or(0, |request| request.unit);
        let new_line = new_line_kind(options).map_err(|failure| UnitEmitError {
            unit: first,
            failure,
        })?;
        let settings = UnitEmitSettings {
            new_line,
            javascript_maps: javascript_map_options_enabled(options),
            eager_source_roots: sink
                .as_deref()
                .is_some_and(OutputSink::writes_source_roots_eagerly),
            trace: file_trace_enabled(),
        };
        let mut printer = create_printer(
            PrinterOptions::new(new_line)
                .with_remove_comments(options.remove_comments == Some(true))
                .with_no_emit_helpers(options.no_emit_helpers == Some(true))
                .with_import_helpers(options.import_helpers == Some(true))
                .with_module_kind(options.emit_module_kind())
                .with_target(options.emit_script_target())
                .with_source_file_text_mode(SourceFileTextMode::Canonical),
        );
        let mut emissions = Vec::with_capacity(requests.len());
        let mut pending = std::collections::VecDeque::<PendingUnitEmission>::new();
        let mut failure = None;
        for request in requests {
            // Retire completed products before mounting another tree. Their
            // owner frees text allocated by the print worker; the credit's
            // retirement also wakes that worker for allocator collection.
            while pending
                .front_mut()
                .is_some_and(PendingUnitEmission::is_ready)
            {
                emissions.push(
                    pending
                        .pop_front()
                        .expect("ready unit")
                        .finish(&mut sink, settings.eager_source_roots)?,
                );
            }
            // Each owner retains at most two unit results, including its next
            // preparation. This also bounds output text awaiting publication.
            if pending.len() == 2 {
                emissions.push(
                    pending
                        .pop_front()
                        .expect("oldest pending unit")
                        .finish(&mut sink, settings.eager_source_roots)?,
                );
            }
            match self.prepare_unit(
                *request,
                &mut printer,
                &mut sink,
                settings,
                !pending.is_empty(),
            ) {
                Ok(unit) => {
                    if self.print_pool.is_none()
                        || (pending.is_empty() && !unit.has_pending_print())
                    {
                        emissions.push(unit.finish(&mut sink, settings.eager_source_roots)?);
                    } else {
                        let failed = unit.has_known_declaration_failure();
                        pending.push_back(unit);
                        if failed {
                            break;
                        }
                    }
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        // Drain in owner/plan order before reporting a later preparation
        // failure. Every earlier successful eager prefix remains published.
        while let Some(unit) = pending.pop_front() {
            emissions.push(unit.finish(&mut sink, settings.eager_source_roots)?);
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(emissions),
        }
    }

    fn prepare_unit(
        &self,
        request: UnitEmitRequest,
        printer: &mut crate::Printer,
        sink: &mut Option<&mut dyn OutputSink>,
        settings: UnitEmitSettings,
        has_prior_pending: bool,
    ) -> Result<PendingUnitEmission, UnitEmitError> {
        let resolver = self.resolver;
        let host = self.host;
        let preflight = self.preflight;
        let options = host.compiler_options();
        let new_line = settings.new_line;
        let javascript_map_options_enabled = settings.javascript_maps;
        let eager_source_roots = settings.eager_source_roots;
        let unit_trace = settings.trace;

        let unit_index = request.unit;
        let unit = &preflight.plan().units()[unit_index];
        let attach = |failure| UnitEmitError {
            unit: unit_index,
            failure,
        };
        let unit_started = unit_trace.then(|| {
            (
                std::time::Instant::now(),
                tsc_types::trace::since_epoch_ms(),
            )
        });
        let mut emission = UnitEmission {
            unit: unit_index,
            artifacts: if request.javascript && unit.paths().javascript_path().is_some() {
                Vec::new()
            } else {
                Vec::with_capacity(2)
            },
            diagnostics: Vec::new(),
            map_observations: Vec::new(),
            written_paths: std::collections::BTreeSet::new(),
            javascript_path: None,
            javascript_map_path: None,
            declaration_path: None,
            declaration_map_path: None,
            emit_skipped: false,
        };
        let EmitRoot::SourceFile(source_id) = *unit.root();
        let source = host
            .source_file(source_id)
            .ok_or(EmitFailure::Contract(
                EmitContractViolation::PlannedSourceMissing(source_id),
            ))
            .map_err(attach)?;
        let source_path = Some(source.path());
        // tsgo `emitter.emit`: a span per source file, around its outputs.
        let _unit_span = host.tracing().map(|tracing| {
            tracing.begin(
                Phase::Emit,
                if self.print_pool.is_some() {
                    "prepareEmitUnit"
                } else {
                    "emit"
                },
                TraceArgs::new().with("path", source.path().to_string_lossy().into_owned()),
            )
        });
        let mut javascript_printed = false;
        let mut javascript_work = None;
        let mut javascript_credit = None;
        let mut declaration_work = None;
        // A member the request leaves out is neither printed nor listed.
        let javascript_path = unit
            .paths()
            .javascript_path()
            .filter(|_| request.javascript)
            .map(JsStr::to_owned);
        let javascript_map_path = unit
            .paths()
            .javascript_map_path()
            .filter(|_| request.javascript)
            .map(JsStr::to_owned);
        let declaration_path = unit
            .paths()
            .declaration_path()
            .filter(|_| request.declaration)
            .map(JsStr::to_owned);

        let unit_credit = self
            .print_pool
            .map(crate::print_pool::EmitPrintPool::acquire);
        if let Some(javascript_path) = javascript_path.as_ref().map(JsString::as_js) {
            if preflight.is_emit_blocked(host, javascript_path) {
                emission.emit_skipped = true;
            } else {
                let _span = host.tracing().map(|tracing| {
                    tracing.begin(
                        Phase::Emit,
                        "emitJsFileOrBundle",
                        TraceArgs::new()
                            .with("jsFilePath", javascript_path.to_string_lossy().into_owned()),
                    )
                });
                let work = emit_javascript_unit(
                    resolver,
                    host,
                    preflight,
                    unit,
                    options,
                    printer,
                    new_line,
                    javascript_map_options_enabled,
                    source_id,
                    source_path,
                    javascript_path,
                    javascript_map_path.as_ref(),
                    self.print_pool,
                    unit_credit.clone(),
                    _span,
                )
                .map_err(attach)?;
                if work.is_complete() {
                    let (products, credit) = work.finish();
                    let products = products.map_err(attach)?;
                    if has_prior_pending {
                        javascript_work = Some(JavaScriptWork::Complete(Ok(products), credit));
                    } else {
                        emission.artifacts = products.artifacts;
                        emission.map_observations = products.map_observations;
                        javascript_credit = credit;
                    }
                } else {
                    javascript_work = Some(work);
                }
                javascript_printed = true;
            }
        }

        // Source outputs retain the whole-Program failure boundary: a later
        // unsupported source must fail before any artifact reaches the sink.
        if eager_source_roots && !has_prior_pending && javascript_work.is_none() {
            if let Some(sink) = sink.as_deref_mut() {
                emission.written_paths.extend(write_artifacts(
                    std::mem::take(&mut emission.artifacts),
                    sink,
                    &mut emission.diagnostics,
                ));
            }
        }

        if let Some(declaration_path) = declaration_path.as_ref().map(JsString::as_js) {
            let _span = host.tracing().map(|tracing| {
                tracing.begin(
                    Phase::Emit,
                    "emitDeclarationFileOrBundle",
                    TraceArgs::new().with(
                        "declarationFilePath",
                        declaration_path.to_string_lossy().into_owned(),
                    ),
                )
            });
            let declaration = crate::declarations::prepare_declaration_unit(
                resolver,
                host,
                preflight,
                preflight.declaration_paths(host),
                unit.root(),
                declaration_path,
                unit.paths().declaration_map_path(),
                false,
                self.print_pool,
                unit_credit.clone(),
                _span,
            );
            declaration_work = Some(match declaration {
                Ok(work) => work,
                Err(error) => crate::declarations::DeclarationWork::Complete(Err(error)),
            });
        }
        // Without a declaration path (`emitDeclarationOnly` without
        // `declaration`), tsgo emitDeclarationFile returns without marking
        // the emit skipped: the run reports its diagnostics with outputs
        // generated (exit 2), where tsc 6.0's emitDeclarationFileOrBundle
        // marked it skipped (exit 1).
        emission.javascript_path = javascript_path.filter(|_| javascript_printed);
        emission.javascript_map_path = javascript_map_path.filter(|_| javascript_printed);
        emission.declaration_path = declaration_path;
        emission.declaration_map_path = None;
        let trace = unit_started
            .map(|(started, at)| (started, at, source.path().to_string_lossy().into_owned()));
        Ok(PendingUnitEmission {
            emission,
            javascript: javascript_work,
            declaration: declaration_work,
            _credit: unit_credit.or(javascript_credit),
            trace,
            _span: if self.print_pool.is_none() {
                _unit_span
            } else {
                None
            },
        })
    }
}

pub(crate) enum OutputPath<'a> {
    Borrowed(JsStr<'a>),
    Owned(JsString),
}

impl OutputPath<'_> {
    pub(crate) fn as_js(&self) -> JsStr<'_> {
        match self {
            Self::Borrowed(path) => *path,
            Self::Owned(path) => path.as_js(),
        }
    }

    pub(crate) fn into_string(self) -> JsString {
        match self {
            Self::Borrowed(path) => path.to_owned(),
            Self::Owned(path) => path,
        }
    }

    pub(crate) fn into_owned(self) -> OutputPath<'static> {
        OutputPath::Owned(self.into_string())
    }
}

enum MappingUrl {
    Inline,
    External(Result<String, EmitFailure>),
}

struct JavaScriptPrintInputs<'a> {
    path: OutputPath<'a>,
    map_path: Option<OutputPath<'a>>,
    source_files: Vec<JsString>,
    recording: Option<SourceMapRecordingInputs>,
    mapping_url: Option<MappingUrl>,
    new_line: NewLineKind,
    emit_bom: bool,
    transform_diagnostics: DiagnosticList,
    _span: Option<tsc_types::tracing::Span>,
    worker_trace: Option<std::sync::Arc<tsc_types::tracing::Tracing>>,
}

impl JavaScriptPrintInputs<'_> {
    fn into_owned(self) -> JavaScriptPrintInputs<'static> {
        JavaScriptPrintInputs {
            path: self.path.into_owned(),
            map_path: self.map_path.map(OutputPath::into_owned),
            source_files: self.source_files,
            recording: self.recording,
            mapping_url: self.mapping_url,
            new_line: self.new_line,
            emit_bom: self.emit_bom,
            transform_diagnostics: self.transform_diagnostics,
            _span: self._span,
            worker_trace: self.worker_trace,
        }
    }

    fn input_charge(&self) -> Option<usize> {
        if matches!(self.mapping_url, Some(MappingUrl::External(Err(_))))
            || self
                .recording
                .as_ref()
                .is_some_and(|input| input.original_positions.is_some())
        {
            return None;
        }
        let mut bytes = 4096 + self.path.as_js().as_bytes().len() * 2;
        if let Some(path) = &self.map_path {
            bytes += path.as_js().as_bytes().len() * 2;
        }
        if let Some(MappingUrl::External(Ok(url))) = &self.mapping_url {
            bytes += url.capacity();
        }
        let mut usage = tsc_diagnostics::MemoryUsage::default();
        usage.vec("source paths", &self.source_files);
        for path in &self.source_files {
            usage.js_string("source paths", path);
        }
        usage.diagnostics(&self.transform_diagnostics);
        if let Some(input) = &self.recording {
            for path in [
                &input.file,
                &input.source_root,
                &input.sources_directory_path,
                &input.current_directory,
            ] {
                usage.js_string("map input paths", path);
            }
        }
        bytes.checked_add(usage.total().requested.checked_mul(2)?)
    }
}

struct JavaScriptProducts {
    artifacts: Vec<EmitArtifact>,
    map_observations: Vec<SourceMapObservation>,
}

impl Default for JavaScriptProducts {
    fn default() -> Self {
        Self {
            artifacts: Vec::with_capacity(2),
            map_observations: Vec::new(),
        }
    }
}

enum JavaScriptWork {
    Complete(
        Result<JavaScriptProducts, EmitFailure>,
        Option<crate::print_pool::PrintCredit>,
    ),
    Pending(crate::print_pool::PendingPrint<Result<JavaScriptProducts, EmitFailure>>),
}

impl JavaScriptWork {
    fn is_ready(&mut self) -> bool {
        match self {
            Self::Complete(..) => true,
            Self::Pending(work) => work.is_ready(),
        }
    }

    fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(..))
    }

    fn finish(
        self,
    ) -> (
        Result<JavaScriptProducts, EmitFailure>,
        Option<crate::print_pool::PrintCredit>,
    ) {
        match self {
            Self::Complete(result, credit) => (result, credit),
            Self::Pending(pending) => {
                let (result, credit) = pending.finish();
                (result, Some(credit))
            }
        }
    }
}

enum JavaScriptRetirement {
    Background(Option<crate::print_pool::PrintCredit>),
    Worker,
}

struct RetiredJavaScriptArena {
    // Field order releases storage before the credit's last holder.
    arena: TransformArena,
    _credit: Option<crate::print_pool::PrintCredit>,
}

/// tsc-port: emitJsFileOrBundle @6.0.3 (_tsc.js:116594-116598 and the print
/// / source-map branches that follow): the script transform, print and
/// artifact construction of one unit.
#[allow(clippy::too_many_arguments)]
fn emit_javascript_unit(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    unit: &crate::EmitOutputUnit,
    options: &CompilerOptions,
    printer: &mut crate::Printer,
    new_line: NewLineKind,
    javascript_map_options_enabled: bool,
    source_id: SourceFileId,
    source_path: Option<JsStr<'_>>,
    javascript_path: JsStr<'_>,
    javascript_map_path: Option<&JsString>,
    pool: Option<&crate::print_pool::EmitPrintPool>,
    credit: Option<crate::print_pool::PrintCredit>,
    span: Option<tsc_types::tracing::Span>,
) -> Result<JavaScriptWork, EmitFailure> {
    let mut credit = credit;
    let _ = preflight;
    let mut arena = TransformArena::new();
    let transform_root = mount_emit_root(&mut arena, host, unit.root(), true)?;
    // tsgo's import elision marks the alias references of every source it
    // transforms before it looks at an import
    // (transformers/tstransforms/importelision.go:29), so import elision
    // sees the `referenced` facts whether or not the source was checked
    // first. The resolver walks only a source its checker has not checked
    // (emit before the check, noCheck, a file excluded by
    // canIncludeBindAndCheckDiagnostics). Resolvers without a checker
    // (transform-only fixtures) answer UnavailableForSource and keep their
    // "checked file" assumption.
    for &file in unit.root().source_files() {
        match resolver.mark_linked_references(file) {
            Ok(()) | Err(EmitResolverError::UnavailableForSource { .. }) => {}
            Err(error) => return Err(TransformError::from(error).into()),
        }
    }
    let transformers = get_script_transformers_for_source(options, resolver, host, source_id)?;
    let mut transformation = transform_nodes(arena, vec![transform_root], transformers, false)?;
    if file_trace_enabled() {
        if let Some(TransformRoot::SourceFile(source)) = transformation.roots().first() {
            let source = transformation.arena().source(*source)?;
            eprintln!(
                "[unit-nodes] parsed={} total={} {}",
                source.parsed_node_count(),
                source.syntax().arena.nodes().len(),
                source.syntax().file_name.to_string_lossy()
            );
        }
    }
    let transformed_root = match transformation.roots() {
        [root] => root.clone(),
        _ => {
            return Err(EmitFailure::Transform(Box::new(
                crate::TransformError::UnsupportedCompilerOption {
                    option: "script transformer contract",
                    detail: "script transform must produce exactly one root",
                },
            )))
        }
    };
    let source_files = transformed_source_paths(&transformation, &transformed_root, host)?;
    let TransformRoot::SourceFile(root_source) = &transformed_root;
    let transform_diagnostics = transformation.diagnostics().to_vec();
    // tsc-port: shouldEmitSourceMaps @6.0.3
    // tsc-hash: 313b475b45d97ba74f69e4e404efd89763caf5fcc7ca9f94c293edf8fdea4f52
    // tsc-span: _tsc.js:116805-116807
    let json_source = source_path.is_some_and(|path| path.ends_with(".json"));
    let recording_enabled = javascript_map_options_enabled && !json_source;
    if recording_enabled && options.inline_source_map != Some(true) && javascript_map_path.is_none()
    {
        return Err(EmitFailure::Contract(
            EmitContractViolation::SourceMapRecordingUnavailable,
        ));
    }
    let recording_inputs = recording_enabled.then(|| {
        source_map_recording_inputs_for_output(
            &map_lane_inputs(host),
            options,
            javascript_path,
            source_path,
        )
    });
    let prepared = printer.prepare_source_file_print(
        &mut transformation,
        *root_source,
        Some(&ResolverGlobalNameOracle(resolver)),
    )?;
    let mapping_url = recording_enabled.then(|| {
        if options.inline_source_map == Some(true) {
            MappingUrl::Inline
        } else {
            // Retain an error until after printing, preserving the original
            // precedence of a print failure over a malformed map URL.
            MappingUrl::External(source_mapping_url_for_output(
                &map_lane_inputs(host),
                options,
                "",
                javascript_path,
                javascript_map_path.map(JsString::as_js),
                source_path,
            ))
        }
    });
    let mut inputs = JavaScriptPrintInputs {
        path: OutputPath::Borrowed(javascript_path),
        map_path: javascript_map_path.map(|path| OutputPath::Borrowed(path.as_js())),
        source_files,
        recording: recording_inputs,
        mapping_url,
        new_line,
        emit_bom: options.emit_bom == Some(true),
        transform_diagnostics,
        _span: span,
        worker_trace: None,
    };
    if let (Some(pool), Some(lease)) = (pool, credit.take()) {
        let charge = transformation
            .has_inert_print_hooks()
            .then(|| transformation.print_input_charge())
            .flatten()
            .and_then(|tree| tree.checked_add(inputs.input_charge()?));
        if let Some(input_charge) = charge.and_then(|bytes| lease.try_charge(bytes)) {
            if let Some(owned) = transformation.take_owned_print() {
                if let (Some(span), Some(bytes)) = (inputs._span.as_mut(), charge) {
                    span.set_arg("ownedPrintCharge", bytes as u64);
                }
                drop(inputs._span.take());
                inputs.worker_trace = host.tracing().cloned();
                let inputs = inputs.into_owned();
                let options = printer.options();
                return Ok(JavaScriptWork::Pending(pool.submit(lease, move |cached| {
                    let _input_charge = input_charge;
                    let printer = cached.get_or_insert_with(|| create_printer(options));
                    debug_assert_eq!(printer.options(), options);
                    let outcome = print_prepared_javascript(
                        owned.into_result(),
                        printer,
                        prepared,
                        inputs,
                        JavaScriptRetirement::Worker,
                    );
                    if outcome.is_err() {
                        // A failed logical printer is never reused by an
                        // unrelated unit assigned to the same worker.
                        *cached = None;
                    }
                    outcome
                })));
            }
        }
        credit = Some(lease);
    }
    let retirement = JavaScriptRetirement::Background(credit.clone());
    let outcome = print_prepared_javascript(transformation, printer, prepared, inputs, retirement);
    Ok(JavaScriptWork::Complete(outcome, credit))
}

fn print_prepared_javascript(
    mut transformation: crate::TransformationResult<'_>,
    printer: &mut crate::Printer,
    prepared: crate::printer::PreparedSourceFilePrint,
    inputs: JavaScriptPrintInputs<'_>,
    retirement: JavaScriptRetirement,
) -> Result<JavaScriptProducts, EmitFailure> {
    let JavaScriptPrintInputs {
        path,
        mut map_path,
        source_files,
        recording,
        mapping_url,
        new_line,
        emit_bom,
        transform_diagnostics,
        _span,
        worker_trace,
    } = inputs;
    let _worker_span = worker_trace.as_ref().map(|tracing| {
        tracing.begin(
            Phase::Emit,
            "printPreparedJavaScript",
            TraceArgs::new().with("jsFilePath", path.as_js().to_string_lossy().into_owned()),
        )
    });
    let mut products = JavaScriptProducts::default();
    let printed = printer.print_prepared_source_file(&mut transformation, prepared, recording)?;
    transformation.dispose();
    let arena = transformation.into_arena();
    let background = matches!(retirement, JavaScriptRetirement::Background(_));
    let retired = RetiredJavaScriptArena {
        arena,
        _credit: match retirement {
            JavaScriptRetirement::Background(credit) => credit,
            JavaScriptRetirement::Worker => None,
        },
    };
    if background && retired.arena.node_count() >= 32 * 1024 {
        let _ = std::thread::Builder::new()
            .name("tsc-rs-release".to_owned())
            .spawn(move || drop(retired));
    } else {
        // Worker work retains its credit through actual arena destruction.
        drop(retired);
    }
    if let Some(mapping_url) = mapping_url {
        let mut generator = printed.source_map().cloned().ok_or(EmitFailure::Contract(
            EmitContractViolation::SourceMapRecordingUnavailable,
        ))?;
        let map_json = generator.to_json_string();
        products.map_observations.push(SourceMapObservation::new(
            path.as_js().to_owned(),
            generator.raw_sources().to_vec(),
            map_json.clone().into_boxed_str(),
        ));
        let url = match mapping_url {
            MappingUrl::Inline => format!(
                "data:application/json;base64,{}",
                base64_encode(map_json.as_bytes())
            ),
            MappingUrl::External(url) => url?,
        };
        // The callback keeps the generated value's raw units; only
        // its sink projection substitutes U+FFFD.
        let end = printed.end();
        let mut javascript_text = printed.into_generated_text();
        let mut url_position = end.position();
        if end.column() != 0 {
            javascript_text.push_str(new_line.text());
            url_position = url_position
                .checked_add(new_line.text().len() as u32)
                .ok_or(EmitFailure::Contract(
                    EmitContractViolation::SourceMapRecordingUnavailable,
                ))?;
        }
        javascript_text.push_str("//# sourceMappingURL=");
        javascript_text.push_str(&url);
        if let Some(map_path) = map_path.take() {
            products.artifacts.push(EmitArtifact::javascript_map(
                map_path.into_string(),
                map_json,
                Some(source_files.clone()),
            ));
        }
        products.artifacts.push(EmitArtifact::javascript(
            path.into_string(),
            EmitCallbackText::from_generated(javascript_text),
            emit_bom,
            Some(source_files.clone()),
            EmitTextMetadata::new(transform_diagnostics, Some(url_position)),
        ));
    } else {
        products.artifacts.push(EmitArtifact::javascript(
            path.into_string(),
            EmitCallbackText::from_generated(printed.into_generated_text()),
            emit_bom,
            Some(source_files.clone()),
            EmitTextMetadata::new(transform_diagnostics, None),
        ));
    }
    Ok(products)
}

/// Phase 3 of emitFiles: write every retained artifact in plan order,
/// assemble the emitted-files listing and the sorted diagnostic collection.
pub fn finish_emit_files(
    session: EmitFilesSession,
    mut emissions: Vec<UnitEmission>,
    sink: &mut dyn OutputSink,
) -> Result<EmitOutcome, EmitFailure> {
    emissions.sort_by_key(|emission| emission.unit);
    let mut artifacts = Vec::new();
    // The emission each artifact belongs to: a failed write is that unit's
    // diagnostic.
    let mut owners = Vec::new();
    let mut written_paths = std::collections::BTreeSet::new();
    for (owner, emission) in emissions.iter_mut().enumerate() {
        owners.extend(std::iter::repeat_n(owner, emission.artifacts.len()));
        artifacts.append(&mut emission.artifacts);
    }
    for (owner, write) in owners
        .into_iter()
        .zip(write_artifact_results(artifacts, sink))
    {
        if let Some(path) = written_path(write, &mut emissions[owner].diagnostics) {
            written_paths.insert(path);
        }
    }
    let mut diagnostics: DiagnosticList = Vec::new();
    let mut source_map_observations: Vec<SourceMapObservation> = Vec::new();
    let mut emit_skipped = false;
    let mut listing = Vec::with_capacity(emissions.len());
    for emission in emissions {
        written_paths.extend(emission.written_paths);
        // tsgo CombineEmitResults: each source's emitter diagnostics (a
        // sorted collection without duplicates), in plan order.
        let mut unit_diagnostics = emission.diagnostics;
        sort_and_dedupe_diagnostics(&mut unit_diagnostics);
        diagnostics.extend(unit_diagnostics);
        source_map_observations.extend(emission.map_observations);
        emit_skipped |= emission.emit_skipped;
        listing.push((
            emission.javascript_path,
            emission.javascript_map_path,
            emission.declaration_path,
            emission.declaration_map_path,
        ));
    }
    let emitted_files = session.emitted_files_enabled.then(|| {
        // tsgo printSourceFile (compiler/emitter.go) writes and lists a
        // source map before its text, for the JavaScript and for the
        // declaration file; a failed write is not listed, nor a declaration
        // file whose write the callback skipped (an unchanged composite
        // output).
        listing
            .into_iter()
            .flat_map(
                |(javascript_path, map_path, declaration_path, declaration_map_path)| {
                    [
                        map_path,
                        javascript_path,
                        declaration_map_path,
                        declaration_path,
                    ]
                },
            )
            .flatten()
            .filter(|path| written_paths.contains(path))
            .collect()
    });

    Ok(EmitOutcome::new(
        diagnostics,
        emit_skipped,
        emitted_files,
        session
            .map_options_enabled
            .then_some(source_map_observations),
    ))
}

/// Dispatch already-built artifacts through the shared write/error boundary.
fn write_artifacts(
    artifacts: Vec<EmitArtifact>,
    sink: &mut dyn OutputSink,
    diagnostics: &mut DiagnosticList,
) -> std::collections::BTreeSet<JsString> {
    write_artifact_results(artifacts, sink)
        .into_iter()
        .filter_map(|write| written_path(write, diagnostics))
        .collect()
}

/// One artifact's write: its path, the file a failure is reported for and
/// the result.
struct ArtifactWrite {
    path: JsString,
    reported_path: JsString,
    result: Result<EmitWriteDisposition, crate::EmitIoError>,
}

/// Write a batch of artifacts; each artifact's write, in artifact order (the
/// sink may write the batch concurrently, the results are those of
/// sequential writes).
fn write_artifact_results(
    artifacts: Vec<EmitArtifact>,
    sink: &mut dyn OutputSink,
) -> Vec<ArtifactWrite> {
    use crate::EmitArtifactKind as Kind;
    // tsgo printSourceFile writes a source map right before its text and
    // reports a failed map write for the text file.
    let paths = artifacts
        .iter()
        .enumerate()
        .map(|(index, artifact)| {
            let text_kind = match artifact.kind() {
                Kind::JavaScriptMap => Some(Kind::JavaScript),
                Kind::DeclarationMap => Some(Kind::Declaration),
                _ => None,
            };
            let reported = artifacts
                .get(index + 1)
                .filter(|text| text_kind == Some(text.kind()))
                .unwrap_or(artifact);
            (artifact.path().to_owned(), reported.path().to_owned())
        })
        .collect::<Vec<_>>();
    paths
        .into_iter()
        .zip(sink.write_all(artifacts))
        .map(|((path, reported_path), result)| ArtifactWrite {
            path,
            reported_path,
            result,
        })
        .collect()
}

/// The path of an artifact the emit lists after its write (tsgo: neither a
/// failed write, which reports its diagnostic, nor an unchanged one).
fn written_path(write: ArtifactWrite, diagnostics: &mut DiagnosticList) -> Option<JsString> {
    match write.result {
        Ok(EmitWriteDisposition::Written) => Some(write.path),
        Ok(EmitWriteDisposition::SkippedUnchanged) => None,
        Err(error) => {
            diagnostics.push(write_diagnostic(
                write.reported_path.as_js(),
                error.message(),
            ));
            None
        }
    }
}

/// The declaration output of one source as tsgo's builder-signature emit
/// produces it (`EmitOnlyBuilderSignature`, compiler/emitter.go: forced
/// declaration emit, no declaration map, not blocked by its diagnostics).
/// The plan is computed once; each source's output is produced on request.
pub struct ForcedDeclarationEmitter {
    preflight: EmitPreflight,
    paths: PlanDeclarationPaths,
    /// The plan index of each source's unit, by the source's index.
    unit_by_source: std::collections::BTreeMap<usize, usize>,
}

/// One source's forced declaration output: the text, the UTF-16 position
/// of its `sourceMappingURL` comment when it has one, and the declaration
/// diagnostics of the transform.
#[derive(Clone, Debug, PartialEq)]
pub struct ForcedDeclarationOutput {
    pub text: String,
    pub source_map_url_position: Option<u32>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ForcedDeclarationEmitter {
    pub fn new(host: &dyn EmitHost) -> Result<Self, EmitFailure> {
        validate_forced_declaration_request(host)?;
        let preflight =
            crate::plan::preflight_forced_declarations(host, EmitSelection::WholeProgram)?;
        let paths = PlanDeclarationPaths::for_declaration_diagnostics(host)?;
        let unit_by_source = preflight
            .plan()
            .units()
            .iter()
            .enumerate()
            .map(|(index, unit)| {
                let EmitRoot::SourceFile(source) = unit.root();
                (source.index(), index)
            })
            .collect();
        Ok(Self {
            preflight,
            paths,
            unit_by_source,
        })
    }

    /// The forced declaration output of `source`, `None` when the source
    /// has no declaration output (a declaration file, JSON, a file that is
    /// not emitted).
    pub fn emit(
        &self,
        resolver: &dyn EmitResolver,
        host: &dyn EmitHost,
        source: SourceFileId,
    ) -> Result<Option<ForcedDeclarationOutput>, EmitFailure> {
        let Some(&unit_index) = self.unit_by_source.get(&source.index()) else {
            return Ok(None);
        };
        let unit = &self.preflight.plan().units()[unit_index];
        let Some(path) = unit.paths().declaration_path() else {
            return Ok(None);
        };
        // tsgo's builder-signature emit writes no declaration map; the text
        // before the `sourceMappingURL` comment is the same either way, and
        // the caller hashes only that (the map lane needs its path when
        // `declarationMap` is on).
        let declaration = emit_declaration_unit(
            resolver,
            host,
            &self.preflight,
            &self.paths,
            unit.root(),
            path,
            unit.paths().declaration_map_path(),
            true,
        )?;
        Ok(declaration.artifact.map(|artifact| {
            let source_map_url_position = match artifact.metadata() {
                Some(crate::EmitWriteMetadata::Text(metadata)) => metadata
                    .source_map_url_position()
                    .map(|position| position.value()),
                _ => None,
            };
            ForcedDeclarationOutput {
                text: artifact.callback_text().to_owned(),
                source_map_url_position,
                diagnostics: declaration.diagnostics,
            }
        }))
    }
}

/// The forced declaration-only branch of emitFiles (_tsc.js:116530-116858),
/// reusing emitDeclarationFileOrBundle's transform, blocking and printer owner.
/// tsrs-native: separate entry keeps ordinary emit request guards unchanged.
/// The forced declaration-only Program.emit route. Ordinary targeted/emitOnly
/// requests keep their existing typed boundaries. The Program's ordinary
/// blocked paths survive, while noEmitOnError and semantic checking are skipped.
pub fn emit_forced_declarations(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    selection: EmitSelection,
    sink: &mut dyn OutputSink,
) -> Result<EmitOutcome, EmitFailure> {
    validate_forced_declaration_request(host)?;
    let options = host.compiler_options();
    let preflight = crate::plan::preflight_forced_declarations(host, selection)?;
    let paths = PlanDeclarationPaths::for_declaration_diagnostics(host)?;
    let maps_enabled = options.source_map == Some(true)
        || options.inline_source_map == Some(true)
        || (options.declaration_map == Some(true)
            && (options.declaration == Some(true) || options.composite == Some(true)));
    let mut source_maps = Vec::new();
    let mut listing = Vec::new();
    let mut diagnostics = Vec::new();
    for unit in preflight.plan().units() {
        let path = unit
            .paths()
            .declaration_path()
            .expect("forced plan has a declaration path");
        let declaration = emit_declaration_unit(
            resolver,
            host,
            &preflight,
            &paths,
            unit.root(),
            path,
            unit.paths().declaration_map_path(),
            true,
        )?;
        // tsgo emitDeclarationFile: a forced emit is not blocked by its
        // diagnostics, the blocked paths or `noEmit`, nor marked skipped.
        let mut unit_diagnostics = declaration.diagnostics;
        if let Some(map) = declaration.map_observation {
            source_maps.push(map);
        }
        let mut artifacts = Vec::new();
        artifacts.extend(declaration.map_artifact);
        artifacts.extend(declaration.artifact);
        // Forced emit follows the per-source write boundary. A later TS
        // assertion must retain earlier JSON writes instead of discarding a
        // Program-wide preconstructed artifact vector.
        let written = write_artifacts(artifacts, sink, &mut unit_diagnostics);
        // tsgo CombineEmitResults: each source's sorted collection in turn.
        sort_and_dedupe_diagnostics(&mut unit_diagnostics);
        diagnostics.extend(unit_diagnostics);
        // tsgo printSourceFile: the map, then the declaration file, each
        // once written.
        listing.extend(
            [unit.paths().declaration_map_path(), Some(path)]
                .into_iter()
                .flatten()
                .filter(|path| written.contains(path.as_bytes()))
                .map(|path| path.to_owned()),
        );
    }
    Ok(EmitOutcome::new(
        diagnostics,
        false,
        host.collects_emitted_files().then_some(listing),
        maps_enabled.then_some(source_maps),
    ))
}

/// tsgo `Program.Emit` with `ForceEmit` and `EmitOnlyJs` (the API's
/// `getJavaScriptEmit`): the JavaScript output and its map of each selected
/// source, `noEmit`, `emitDeclarationOnly`, `noEmitOnError` and the blocked
/// outputs aside.
pub fn emit_forced_javascript(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    selection: EmitSelection,
    sink: &mut dyn OutputSink,
) -> Result<EmitOutcome, EmitFailure> {
    validate_request(host, EmitOperation::ForcedJavaScript)?;
    let preflight = crate::plan::preflight_forced_javascript(host, selection)?;
    let requests = (0..preflight.plan().units().len())
        .map(|unit| UnitEmitRequest {
            unit,
            javascript: true,
            declaration: false,
        })
        .collect::<Vec<_>>();
    let emissions =
        emit_planned_units_with_kinds(resolver, host, &preflight, &requests, Some(sink))
            .map_err(|error| error.failure)?;
    finish_emit_files(
        EmitFilesSession {
            emitted_files_enabled: host.collects_emitted_files(),
            map_options_enabled: javascript_map_options_enabled(host.compiler_options()),
        },
        emissions,
        sink,
    )
}

fn write_diagnostic(path: JsStr<'_>, message: JsStr<'_>) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js(
            &gen::Could_not_write_file_0_1,
            &[path.to_owned(), message.to_owned()],
        ),
    )
}

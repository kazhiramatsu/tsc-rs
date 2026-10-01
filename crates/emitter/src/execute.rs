use crate::artifact::EmitCallbackText;
use crate::PreparedEmitSource;
use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_diagnostics::{JsStr, JsString};
use tsc_program::SourceFileId;
use tsc_syntax::SourceFile;
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

    for (active, name) in [
        (
            operation == EmitOperation::Files && options.no_emit == Some(true),
            "noEmit",
        ),
        // `noCheck` has no ordinary-emit refusal left: `skipTypeChecking`
        // already reads the option for every file, and the emit resolver
        // resolves lazily (tsc emits the program unchecked;
        // EF7-NOCHECK-ROUTE).
        // `isolatedModules` and `verbatimModuleSyntax` have no ordinary-emit
        // refusal left: the TypeScript transform already keeps const enum
        // declarations (`should_preserve_const_enums`), skips const-value
        // folding (`tryGetConstEnumValue`, _tsc.js substituteConstantValue)
        // and reads `verbatim_module_syntax` for alias elision on every
        // route, and the checker owns both options' diagnostics
        // (H2.8a-A-RES-EMITTER-FINAL EF3 / EF7-VERBATIM-GATE).
        // `incremental` and `composite` select the builder's build info,
        // which only the file emit would write; the declaration diagnostics
        // getter reads neither (a --noEmit check of a composite project
        // reports its declaration diagnostics as tsc does).
        (
            operation != EmitOperation::DeclarationDiagnostics && options.incremental == Some(true),
            "incremental",
        ),
        (
            operation != EmitOperation::DeclarationDiagnostics && options.composite == Some(true),
            "composite",
        ),
        // assumeChangesOnlyAffectDirectDependencies changes the builder's
        // affected-file traversal only. It is inert in this fresh Program.
        // `emitDecoratorMetadata` without `experimentalDecorators` is inert
        // for emit (only the legacy decorator transform reads it) and the
        // program reports TS5052; tsc emits normally (EF7-METADATA-INERT).
    ] {
        if active {
            return unsupported(name);
        }
    }
    if !matches!(options.jsx, None | Some(1..=5)) {
        return unsupported("jsx");
    }
    if options.ts_build_info_file.is_some() {
        return unsupported("tsBuildInfoFile");
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
        let eligible = if operation == EmitOperation::ForcedDeclarations {
            crate::plan::source_file_may_emit_forced_declaration(source, host)
        } else {
            crate::plan::source_file_may_be_emitted(source, host)
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
        let is_other_admitted = options.allow_non_ts_extensions == Some(true);
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
        let EmitRoot::SourceFile(source_id) = unit.root() else {
            return Err(EmitFailure::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
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
    Ok(match root {
        EmitRoot::SourceFile(_) => TransformRoot::SourceFile(sources[0]),
        EmitRoot::Bundle(_) => TransformRoot::Bundle(crate::TransformBundle::new(sources)),
    })
}

pub(crate) fn transformed_source_paths(
    result: &crate::TransformationResult<'_>,
    root: &TransformRoot,
    host: &dyn EmitHost,
) -> Result<Vec<JsString>, EmitFailure> {
    let sources = match root {
        TransformRoot::SourceFile(source) => std::slice::from_ref(source),
        TransformRoot::Bundle(bundle) => bundle.sources(),
    };
    sources
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

    let emitted_files_enabled = options.list_emitted_files == Some(true);
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

pub fn emit_planned_units(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    units: &[usize],
    mut sink: Option<&mut dyn OutputSink>,
) -> Result<Vec<UnitEmission>, UnitEmitError> {
    let mut emissions = Vec::with_capacity(units.len());
    // An eager worker sink writes ordinary source roots as they are printed
    // (bundle roots always write through a supplied sink).
    let eager_source_roots = sink
        .as_deref()
        .is_some_and(OutputSink::writes_source_roots_eagerly);
    let options = host.compiler_options();
    let attach = |unit: usize| move |failure: EmitFailure| UnitEmitError { unit, failure };
    let first = units.first().copied().unwrap_or(0);
    let new_line = new_line_kind(options).map_err(attach(first))?;
    let declaration_paths: &PlanDeclarationPaths = preflight.declaration_paths(host);
    let mut printer = create_printer(
        PrinterOptions::new(new_line)
            .with_remove_comments(options.remove_comments == Some(true))
            .with_no_emit_helpers(options.no_emit_helpers == Some(true))
            .with_import_helpers(options.import_helpers == Some(true))
            .with_module_kind(options.emit_module_kind())
            .with_target(options.emit_script_target())
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let javascript_map_options_enabled = javascript_map_options_enabled(options);
    // `TSRS_FILE_TRACE=1` prints one line per emitted unit with its wall
    // time and timeline position (the checker prints the checked files).
    let unit_trace = file_trace_enabled();
    for &unit_index in units {
        let unit = &preflight.plan().units()[unit_index];
        let attach = attach(unit_index);
        let unit_started = unit_trace.then(|| {
            (
                std::time::Instant::now(),
                tsc_types::trace::since_epoch_ms(),
            )
        });
        let mut emission = UnitEmission {
            unit: unit_index,
            artifacts: Vec::with_capacity(2),
            diagnostics: Vec::new(),
            map_observations: Vec::new(),
            written_paths: std::collections::BTreeSet::new(),
            javascript_path: None,
            javascript_map_path: None,
            declaration_path: None,
            declaration_map_path: None,
            emit_skipped: false,
        };
        let source_id = *unit
            .root()
            .source_files()
            .first()
            .ok_or(EmitFailure::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ))
            .map_err(attach)?;
        let source = host
            .source_file(source_id)
            .ok_or(EmitFailure::Contract(
                EmitContractViolation::PlannedSourceMissing(source_id),
            ))
            .map_err(attach)?;
        let source_path = match unit.root() {
            EmitRoot::SourceFile(_) => Some(source.path()),
            EmitRoot::Bundle(_) => None,
        };
        let mut parsed_emit_metadata = None;
        let mut javascript_printed = false;
        let javascript_path = unit.paths().javascript_path().map(JsStr::to_owned);
        let javascript_map_path = unit.paths().javascript_map_path().map(JsStr::to_owned);
        let declaration_path = unit.paths().declaration_path().map(JsStr::to_owned);

        if let Some(javascript_path) = javascript_path.as_ref().map(JsString::as_js) {
            if preflight.is_emit_blocked(host, javascript_path) {
                emission.emit_skipped = true;
            } else {
                emit_javascript_unit(
                    resolver,
                    host,
                    preflight,
                    unit,
                    options,
                    &mut printer,
                    new_line,
                    javascript_map_options_enabled,
                    source_id,
                    source_path,
                    javascript_path,
                    javascript_map_path.as_ref(),
                    declaration_path.is_some(),
                    &mut emission,
                    &mut parsed_emit_metadata,
                )
                .map_err(attach)?;
                javascript_printed = true;
            }
        }

        // Bundle callbacks precede the declaration transform. Ordinary source
        // outputs retain the existing whole-Program failure boundary: a later
        // unsupported source must fail before any artifact reaches the sink.
        if eager_source_roots || matches!(unit.root(), EmitRoot::Bundle(_)) {
            if let Some(sink) = sink.as_deref_mut() {
                emission.written_paths.extend(write_artifacts(
                    std::mem::take(&mut emission.artifacts),
                    sink,
                    &mut emission.diagnostics,
                ));
            }
        }

        let mut printed_declaration_map_path = None;
        if let Some(declaration_path) = declaration_path.as_ref().map(JsString::as_js) {
            let declaration = emit_declaration_unit(
                resolver,
                host,
                preflight,
                declaration_paths,
                unit.root(),
                declaration_path,
                unit.paths().declaration_map_path(),
                false,
                parsed_emit_metadata.as_ref(),
            )
            .map_err(attach)?;
            emission.emit_skipped |= declaration.decl_blocked;
            emission.diagnostics.extend(declaration.diagnostics);
            if let Some(observation) = declaration.map_observation {
                emission.map_observations.push(observation);
            }
            if let Some(map) = declaration.map_artifact {
                printed_declaration_map_path = Some(map.path().to_owned());
                emission.artifacts.push(map);
            }
            if let Some(artifact) = declaration.artifact {
                emission.artifacts.push(artifact);
            }
        } else if options.emit_declaration_only == Some(true) {
            // emitDeclarationFileOrBundle also marks a missing declaration
            // path as skipped. An all-.d.ts program has no units to visit.
            emission.emit_skipped = true;
        }
        if eager_source_roots || matches!(unit.root(), EmitRoot::Bundle(_)) {
            if let Some(sink) = sink.as_deref_mut() {
                emission.written_paths.extend(write_artifacts(
                    std::mem::take(&mut emission.artifacts),
                    sink,
                    &mut emission.diagnostics,
                ));
            }
        }
        emission.javascript_path = javascript_path.filter(|_| javascript_printed);
        emission.javascript_map_path = javascript_map_path.filter(|_| javascript_printed);
        emission.declaration_path = declaration_path;
        emission.declaration_map_path = printed_declaration_map_path;
        if let Some((started, at)) = unit_started {
            let path = host
                .source_file(source_id)
                .map(|source| source.path().to_string_lossy().into_owned())
                .unwrap_or_default();
            eprintln!(
                "[unit] ms={:.3} at={:.1} js={} decl={} {path}",
                started.elapsed().as_secs_f64() * 1e3,
                at,
                emission.javascript_path.is_some(),
                emission.declaration_path.is_some(),
            );
        }
        emissions.push(emission);
    }
    Ok(emissions)
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
    declaration_planned: bool,
    emission: &mut UnitEmission,
    parsed_emit_metadata: &mut Option<crate::ParsedEmitMetadata>,
) -> Result<(), EmitFailure> {
    let _ = preflight;
    let mut arena = TransformArena::new();
    let transform_root = mount_emit_root(&mut arena, host, unit.root(), true)?;
    // tsc-port: emitJsFileOrBundle @6.0.3 (_tsc.js:116594-116598)
    // Unchecked sources (noCheck, or a file excluded by
    // canIncludeBindAndCheckDiagnostics) have their alias
    // references marked lazily before the script transform so
    // import elision sees the same `referenced` facts a checked
    // file would have. Resolvers without a checker
    // (transform-only fixtures) answer UnavailableForSource and
    // keep their pre-H2.8c "checked file" assumption.
    for &file in unit.root().source_files() {
        let unchecked = if options.no_check == Some(true) {
            true
        } else {
            match resolver.can_include_bind_and_check_diagnostics(file) {
                Ok(can_include) => !can_include,
                Err(EmitResolverError::UnavailableForSource { .. }) => false,
                Err(error) => return Err(TransformError::from(error).into()),
            }
        };
        if unchecked {
            resolver
                .mark_linked_references(file)
                .map_err(TransformError::from)?;
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
    let print_request = match &transformed_root {
        TransformRoot::SourceFile(source) => PrintRequest::SourceFile(*source),
        TransformRoot::Bundle(bundle) => PrintRequest::Bundle(bundle.clone()),
    };
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
    let printed = printer.print_javascript_with_global_names(
        &mut transformation,
        print_request,
        recording_inputs,
        &ResolverGlobalNameOracle(resolver),
    )?;
    // TS transformNodes.dispose clears annotated parse nodes for a
    // SourceFile root. A Bundle root has no parse SourceFile and
    // retains its children's metadata for declaration emission.
    if declaration_planned && matches!(transformed_root, TransformRoot::Bundle(_)) {
        *parsed_emit_metadata = Some(transformation.arena().snapshot_parsed_emit_metadata(host)?);
    }
    transformation.dispose();
    // Dropping a large source's emit copy (its nodes, their identifier texts,
    // the transforms' additions) takes milliseconds on the tail of the run;
    // the disposed arena is released on a detached thread instead. A refused
    // thread simply drops it here.
    let arena = transformation.into_arena();
    if arena.node_count() >= 32 * 1024 {
        let _ = std::thread::Builder::new()
            .name("tsc-rs-release".to_owned())
            .spawn(move || drop(arena));
    } else {
        drop(arena);
    }
    if recording_enabled {
        let map_path = javascript_map_path;
        let mut generator = printed.source_map().cloned().ok_or(EmitFailure::Contract(
            EmitContractViolation::SourceMapRecordingUnavailable,
        ))?;
        let map_json = generator.to_json_string();
        emission.map_observations.push(SourceMapObservation::new(
            generator.raw_sources().to_vec(),
            map_json.clone().into_boxed_str(),
        ));
        let url = source_mapping_url_for_output(
            &map_lane_inputs(host),
            options,
            &map_json,
            javascript_path,
            map_path.map(JsString::as_js),
            source_path,
        )?;
        // The callback keeps the generated value's raw units; only
        // its sink projection substitutes U+FFFD.
        let mut javascript_text = printed.generated_text().clone();
        let mut url_position = printed.end().position();
        if printed.end().column() != 0 {
            javascript_text.push_str(new_line.text());
            url_position = url_position
                .checked_add(new_line.text().len() as u32)
                .ok_or(EmitFailure::Contract(
                    EmitContractViolation::SourceMapRecordingUnavailable,
                ))?;
        }
        javascript_text.push_str("//# sourceMappingURL=");
        javascript_text.push_str(&url);
        if let Some(map_path) = map_path {
            emission.artifacts.push(EmitArtifact::javascript_map(
                map_path.clone(),
                map_json,
                Some(source_files.clone()),
            ));
        }
        emission.artifacts.push(EmitArtifact::javascript(
            javascript_path,
            EmitCallbackText::from_generated(javascript_text),
            options.emit_bom == Some(true),
            Some(source_files.clone()),
            EmitTextMetadata::new(transform_diagnostics, Some(url_position)),
        ));
    } else {
        emission.artifacts.push(EmitArtifact::javascript(
            javascript_path,
            EmitCallbackText::from_generated(printed.generated_text().clone()),
            options.emit_bom == Some(true),
            Some(source_files.clone()),
            EmitTextMetadata::new(transform_diagnostics, None),
        ));
    }
    Ok(())
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
    let mut written_paths = std::collections::BTreeSet::new();
    let mut diagnostics: DiagnosticList = Vec::new();
    let mut source_map_observations: Vec<SourceMapObservation> = Vec::new();
    let mut emit_skipped = false;
    let mut listing = Vec::with_capacity(emissions.len());
    for emission in emissions {
        artifacts.extend(emission.artifacts);
        written_paths.extend(emission.written_paths);
        diagnostics.extend(emission.diagnostics);
        source_map_observations.extend(emission.map_observations);
        emit_skipped |= emission.emit_skipped;
        listing.push((
            emission.javascript_path,
            emission.javascript_map_path,
            emission.declaration_path,
            emission.declaration_map_path,
        ));
    }
    written_paths.extend(write_artifacts(artifacts, sink, &mut diagnostics));
    let emitted_files = session.emitted_files_enabled.then(|| {
        let mut emitted = Vec::new();
        for (javascript_path, map_path, declaration_path, declaration_map_path) in listing {
            // emitJsFileOrBundle ignores skippedDtsWrite: after successful
            // printing both planned JS members are listed. Only declarations
            // use the write callback's skipped disposition below.
            emitted.extend(javascript_path);
            emitted.extend(map_path);
            if let Some(declaration_path) = declaration_path {
                if written_paths.contains(&declaration_path) {
                    emitted.push(declaration_path);
                }
            }
            if let Some(declaration_map_path) = declaration_map_path {
                emitted.push(declaration_map_path);
            }
        }
        emitted
    });

    // tsc-port: emitFiles returns `emitterDiagnostics.getDiagnostics()` — a
    // DiagnosticCollection (@6.0.3 _tsc.js:16199-16264 createDiagnosticCollection;
    // :116534-116551 emitFiles): file-less diagnostics first, files in
    // case-sensitive name order, each file's list in diagnostic order with equal
    // entries dropped. The units append in plan order above, so sort and dedupe
    // the aggregate here (h2-7b-w1: the declarationEmitMixinPrivateProtected
    // row's frozen order interleaves two files).
    sort_and_dedupe_diagnostics(&mut diagnostics);
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
    let mut written_paths: std::collections::BTreeSet<JsString> = std::collections::BTreeSet::new();
    let paths = artifacts
        .iter()
        .map(|artifact| artifact.path().to_owned())
        .collect::<Vec<_>>();
    // The sink may write the batch concurrently; the results come back in
    // artifact order, so the observations below are those of sequential
    // writes.
    let results = sink.write_all(artifacts);
    for (path, result) in paths.into_iter().zip(results) {
        let include_in_emitted_files = match result {
            Ok(EmitWriteDisposition::Written) => true,
            Ok(EmitWriteDisposition::SkippedUnchanged) => false,
            Err(error) => {
                diagnostics.push(write_diagnostic(path.as_js(), error.message()));
                true
            }
        };
        if include_in_emitted_files {
            written_paths.insert(path);
        }
    }
    written_paths
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
    let mut emit_skipped = false;
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
            None,
        )?;
        emit_skipped |= declaration.decl_blocked;
        diagnostics.extend(declaration.diagnostics);
        if let Some(map) = declaration.map_observation {
            source_maps.push(map);
        }
        let printed = declaration.artifact.is_some();
        let mut artifacts = Vec::new();
        artifacts.extend(declaration.map_artifact);
        artifacts.extend(declaration.artifact);
        // Forced emit follows the per-source write boundary. A later TS
        // assertion must retain earlier JSON writes instead of discarding a
        // Program-wide preconstructed artifact vector.
        let written = write_artifacts(artifacts, sink, &mut diagnostics);
        if written.contains(path.as_bytes()) {
            listing.push(path.to_owned());
        }
        if printed {
            // This list entry is unconditional even for a JSON source, whose
            // shouldEmitSourceMaps branch produces no map artifact at all.
            if let Some(map_path) = unit.paths().declaration_map_path() {
                listing.push(map_path.to_owned());
            }
        }
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    Ok(EmitOutcome::new(
        diagnostics,
        emit_skipped,
        (options.list_emitted_files == Some(true)).then_some(listing),
        maps_enabled.then_some(source_maps),
    ))
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

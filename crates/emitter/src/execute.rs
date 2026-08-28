use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_types::{CompilerOptions, ScriptTarget};

use crate::builtins::get_script_transformers_with_activity;
use crate::{
    create_printer, transform_nodes, EmitArtifact, EmitContractViolation, EmitFailure, EmitHost,
    EmitOutcome, EmitPreflight, EmitResolver, EmitRoot, EmitSelection, EmitTextMetadata,
    EmitWriteDisposition, H2ActivityCanary, H2RuntimeSlice, NewLineKind, OutputSink, PrintRequest,
    PrinterOptions, SourceFileTextMode, SourceMapObservation, SourceMapRecordingInputs,
    TransformArena, TransformRoot,
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
        }
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

/// Reject every effective option outside the frozen JavaScript-only bootstrap
/// before output planning, checker-to-emitter borrowing, or sink dispatch.
pub fn validate_bootstrap_emit_options(options: &CompilerOptions) -> Result<(), EmitFailure> {
    let target = options.emit_script_target();
    if target < ScriptTarget::ES5 || target > ScriptTarget::ES_NEXT {
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
        (options.no_emit == Some(true), "noEmit"),
        (options.no_check == Some(true), "noCheck"),
        (options.isolated_modules == Some(true), "isolatedModules"),
        (
            options.verbatim_module_syntax == Some(true),
            "verbatimModuleSyntax",
        ),
        (
            options.allow_importing_ts_extensions == Some(true),
            "allowImportingTsExtensions",
        ),
        (options.declaration_map == Some(true), "declarationMap"),
        (
            options.emit_declaration_only == Some(true),
            "emitDeclarationOnly",
        ),
        (
            options.stable_type_ordering == Some(true),
            "stableTypeOrdering",
        ),
        (options.strip_internal == Some(true), "stripInternal"),
        (options.incremental == Some(true), "incremental"),
        (options.composite == Some(true), "composite"),
        (
            options.assume_changes_only_affect_direct_dependencies == Some(true),
            "assumeChangesOnlyAffectDirectDependencies",
        ),
        (
            options.emit_decorator_metadata == Some(true) && !options.experimental_decorators,
            "emitDecoratorMetadata",
        ),
    ] {
        if active {
            return unsupported(name);
        }
    }
    if !matches!(options.jsx, None | Some(1..=5)) {
        return unsupported("jsx");
    }
    for (present, name) in [
        (options.root_dir.is_some(), "rootDir"),
        (options.declaration_dir.is_some(), "declarationDir"),
        (options.out_file.is_some(), "outFile"),
        (options.ts_build_info_file.is_some(), "tsBuildInfoFile"),
    ] {
        if present {
            return unsupported(name);
        }
    }
    Ok(())
}

/// Validate the option profile and the admitted TypeScript/JavaScript source
/// families before
/// the checker constructs an emit resolver.
pub fn validate_bootstrap_emit_request(host: &dyn EmitHost) -> Result<(), EmitFailure> {
    let options = host.compiler_options();
    validate_bootstrap_emit_options(options)?;
    let mut emit_eligible_sources = 0usize;
    let mut javascript_sources = 0usize;
    let mut json_sources = 0usize;
    for source_id in host.source_file_ids() {
        let source = host.source_file(*source_id).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(*source_id),
        ))?;
        if !crate::plan::source_file_may_be_emitted_for_host(source, host) {
            continue;
        }
        emit_eligible_sources += 1;
        let name = source.path().to_string_lossy().to_ascii_lowercase();
        let is_typescript = name.ends_with(".ts")
            || name.ends_with(".mts")
            || name.ends_with(".cts")
            || name.ends_with(".tsx");
        let is_javascript = options.allow_js
            && (name.ends_with(".js")
                || name.ends_with(".mjs")
                || name.ends_with(".cjs")
                || name.ends_with(".jsx"));
        let is_json = name.ends_with(".json");
        if is_json && !options.resolve_json_module_effective() {
            return unsupported("resolveJsonModule");
        }
        if !(is_typescript || is_javascript || is_json)
            || name.ends_with(".d.ts")
            || name.ends_with(".d.mts")
            || name.ends_with(".d.cts")
        {
            return Err(EmitFailure::UnsupportedSourceExtension {
                path: source.path().to_path_buf(),
            });
        }
        javascript_sources += usize::from(is_javascript);
        json_sources += usize::from(is_json);
    }
    if let Some(out_dir) = options.out_dir.as_deref() {
        // H2.3a owns JavaScript-only relocation. H2.3d additionally owns the
        // narrow mixed source set needed to materialize an admitted JSON
        // artifact. H2.8a retains outDir without either source family and the
        // general rootDir/common-source-directory matrix.
        let javascript_only =
            javascript_sources != 0 && javascript_sources == emit_eligible_sources;
        let json_relocation = json_sources != 0;
        if !(javascript_only || json_relocation) || !std::path::Path::new(out_dir).is_absolute() {
            return unsupported("outDir");
        }
    }
    Ok(())
}

fn observe_source_routing(host: &dyn EmitHost, activity: &mut H2ActivityCanary) {
    for source_id in host.source_file_ids() {
        let Some(source) = host.source_file(*source_id) else {
            continue;
        };
        if !crate::plan::source_file_may_be_emitted_for_host(source, host) {
            continue;
        }
        let name = source.path().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".js")
            || name.ends_with(".mjs")
            || name.ends_with(".cjs")
            || name.ends_with(".jsx")
        {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_3a);
        }
        if name.ends_with(".tsx") || name.ends_with(".jsx") {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_3b);
            if source.syntax().is_some_and(|syntax| {
                syntax.jsx_runtime_pragma.as_deref() != Some("classic")
                    && (matches!(host.compiler_options().jsx, Some(4 | 5))
                        || host.compiler_options().jsx_import_source.is_some()
                        || syntax.has_jsx_import_source_pragma
                        || syntax.jsx_runtime_pragma.as_deref() == Some("automatic"))
            }) {
                activity.observe_runtime_slice(H2RuntimeSlice::H2_3c);
            }
        }
        if name.ends_with(".json") {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_3d);
        }
    }
}

fn unsupported<T>(option: &'static str) -> Result<T, EmitFailure> {
    Err(EmitFailure::UnsupportedCompilerOption { option })
}

/// tsc-port: emitFiles @6.0.3
/// tsc-hash: 62e93c3a8e9e2840b759bbaa0fa6de5e548ebd565748dbbddb47a933a1cf442c
/// tsc-span: _tsc.js:116530-116858
///
/// The profile-only preconstruction of every JavaScript artifact is a
/// fail-closed Rust ownership adaptation: an unsupported later source cannot
/// leave earlier callback writes behind. Once all artifacts exist, callback
/// order follows the ported output-unit order exactly.
pub fn emit_files(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
    sink: &mut dyn OutputSink,
) -> Result<EmitOutcome, EmitFailure> {
    let mut activity = H2ActivityCanary::h2_6c_profile();
    activity.construct_emit_session();
    activity.construct_output_plan();
    if !preflight.plan().units().is_empty() {
        activity.borrow_emit_resolver();
    }
    emit_files_with_activity(
        resolver,
        host,
        preflight,
        selection,
        diagnostic_gate,
        sink,
        &mut activity,
    )
}

/// h2-6a-m-2 §8-A.1 harness-print bridge: the production
/// plan → transform → print pipeline of `emit_files_with_activity`
/// WITHOUT artifacts, sinks, activity accounting, or the option
/// preflight — the replay suite injects a `SourceMapRecordingInputs`
/// per unit and byte-compares the returned text and generator against
/// the frozen witnesses. No production caller exists; real emits keep
/// every refusal lane.
#[doc(hidden)]
pub fn print_script_units_with_recording_for_harness(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: &EmitPreflight,
    recording_inputs_for: &dyn Fn(&std::path::Path) -> Option<crate::SourceMapRecordingInputs>,
) -> Result<Vec<(std::path::PathBuf, crate::PrintedText)>, EmitFailure> {
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
    let mut activity = H2ActivityCanary::h2_6c_profile();
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
        let transformers = get_script_transformers_with_activity(
            options,
            resolver,
            host,
            *source_id,
            &mut activity,
        )?;
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
        printed_units.push((javascript_path.to_path_buf(), printed));
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
fn encode_uri(text: &str) -> String {
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
    encoded
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
pub fn source_root_field(options: &CompilerOptions) -> String {
    let normalized =
        crate::source_map::paths::normalize_slashes(options.source_root.as_deref().unwrap_or(""));
    if normalized.is_empty() {
        normalized
    } else {
        crate::source_map::paths::ensure_trailing_directory_separator(&normalized)
    }
}

fn normalized_display(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
    pub common_source_directory: String,
    pub current_directory: String,
    pub use_case_sensitive_source_keys: bool,
}

fn map_lane_inputs(host: &dyn EmitHost) -> MapLaneInputs {
    MapLaneInputs {
        common_source_directory: crate::source_map::paths::ensure_trailing_directory_separator(
            &crate::source_map::paths::normalize_slashes(
                &host.common_source_directory().to_string_lossy(),
            ),
        ),
        current_directory: normalized_display(host.current_directory()),
        use_case_sensitive_source_keys: host.use_case_sensitive_file_names(),
    }
}

fn directory_and_basename(normalized: &str) -> (&str, &str) {
    match normalized.rfind('/') {
        Some(split) => (&normalized[..split], &normalized[split + 1..]),
        None => ("", normalized),
    }
}

/// tsc-port: getSourceMapDirectory @6.0.3
/// tsc-hash: 5d1e1b69c02c83fe1f02760b408962879736368ad481a3702c120c323c06840d
/// tsc-span: _tsc.js:116812-116825
///
/// The `sourcesDirectoryPath` three-lane selection (h2-6b.md §4.2):
/// `sourceRoot` → the common source directory; `mapRoot` → the
/// normalized root, per-file nested when a source file exists, resolved
/// against the common source directory when relative; otherwise the js
/// output directory (the H2.6a floor lane — the only lane reachable in
/// production until the h2-6b-m-2 refusal lift).
#[doc(hidden)]
pub fn source_map_directory(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    javascript_path: &std::path::Path,
    source_path: &std::path::Path,
) -> String {
    use crate::source_map::paths;
    if options
        .source_root
        .as_deref()
        .is_some_and(|root| !root.is_empty())
    {
        return lane
            .common_source_directory
            .trim_end_matches('/')
            .to_owned();
    }
    if let Some(map_root) = options.map_root.as_deref().filter(|root| !root.is_empty()) {
        let mut source_map_dir = paths::normalize_slashes(map_root);
        // per-file nesting (getSourceFilePathInNewDir): the relative
        // mapRoot stays relative through the worker; the root-length
        // check below then resolves it (upstream order).
        let nested = paths::source_file_path_in_new_dir_worker(
            &normalized_display(source_path),
            &source_map_dir,
            &lane.current_directory,
            &lane.common_source_directory,
            lane.use_case_sensitive_source_keys,
        );
        source_map_dir = directory_and_basename(&nested).0.to_owned();
        if paths::get_root_length(&source_map_dir) == 0 {
            source_map_dir = paths::combine_paths(
                lane.common_source_directory.trim_end_matches('/'),
                &source_map_dir,
            );
        }
        return source_map_dir;
    }
    directory_and_basename(&normalized_display(javascript_path))
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
    javascript_path: &std::path::Path,
    source_path: &std::path::Path,
) -> SourceMapRecordingInputs {
    let normalized = normalized_display(javascript_path);
    let (_, basename) = directory_and_basename(&normalized);
    SourceMapRecordingInputs {
        file: basename.into(),
        source_root: source_root_field(options).into(),
        sources_directory_path: source_map_directory(lane, options, javascript_path, source_path)
            .into(),
        current_directory: lane.current_directory.clone().into(),
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
    javascript_path: &std::path::Path,
    map_path: Option<&std::path::Path>,
    source_path: &std::path::Path,
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
    let (_, map_basename) = directory_and_basename(&normalized_map);
    if let Some(map_root) = options.map_root.as_deref().filter(|root| !root.is_empty()) {
        let mut source_map_dir = paths::normalize_slashes(map_root);
        let nested = paths::source_file_path_in_new_dir_worker(
            &normalized_display(source_path),
            &source_map_dir,
            &lane.current_directory,
            &lane.common_source_directory,
            lane.use_case_sensitive_source_keys,
        );
        source_map_dir = directory_and_basename(&nested).0.to_owned();
        if paths::get_root_length(&source_map_dir) == 0 {
            source_map_dir = paths::combine_paths(
                lane.common_source_directory.trim_end_matches('/'),
                &source_map_dir,
            );
            let normalized_js = normalized_display(javascript_path);
            let (js_directory, _) = directory_and_basename(&normalized_js);
            return Ok(encode_uri(&paths::get_relative_path_to_directory_or_url(
                js_directory,
                &paths::combine_paths(&source_map_dir, map_basename),
                &lane.current_directory,
                lane.use_case_sensitive_source_keys,
                true,
            )));
        }
        return Ok(encode_uri(&paths::combine_paths(
            &source_map_dir,
            map_basename,
        )));
    }
    Ok(encode_uri(map_basename))
}

pub fn emit_files_with_activity(
    resolver: &dyn EmitResolver,
    host: &dyn EmitHost,
    preflight: EmitPreflight,
    selection: EmitSelection,
    diagnostic_gate: &EmitDiagnosticGate,
    sink: &mut dyn OutputSink,
    activity: &mut H2ActivityCanary,
) -> Result<EmitOutcome, EmitFailure> {
    validate_bootstrap_emit_request(host)?;
    observe_source_routing(host, activity);
    let options = host.compiler_options();
    match preflight.plan().validate_bootstrap_shape() {
        Ok(()) => {}
        // H2.6c admits the JavaScript/map members of a declaration-bearing
        // unit. The planned declaration member stays dormant until H2.7;
        // every adjacent dormant member remains fail-closed.
        Err(EmitFailure::Unsupported(crate::UnsupportedEmitFeature::Declaration))
            if options.declaration == Some(true) => {}
        Err(error) => return Err(error),
    }
    if preflight.plan().selection() != selection {
        return Err(EmitFailure::Unsupported(
            crate::UnsupportedEmitFeature::TargetedSelection,
        ));
    }

    let emitted_files_enabled = options.list_emitted_files == Some(true);
    if options.no_emit_on_error == Some(true) {
        let diagnostics = diagnostic_gate.collect_with_preflight(preflight.diagnostics());
        if !diagnostics.is_empty() {
            return Ok(EmitOutcome::new(
                diagnostics,
                true,
                emitted_files_enabled.then(Vec::new),
                None,
                activity.counters(),
            ));
        }
    }

    let new_line = match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        None | Some(1) => NewLineKind::LineFeed,
        Some(_) => return unsupported("newLine"),
    };
    activity.construct_printer();
    let mut printer = create_printer(
        PrinterOptions::new(new_line)
            .with_remove_comments(options.remove_comments == Some(true))
            .with_no_emit_helpers(options.no_emit_helpers == Some(true))
            .with_import_helpers(options.import_helpers == Some(true))
            .with_target(options.emit_script_target())
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );

    let mut artifacts = Vec::with_capacity(preflight.plan().units().len());
    // emittedFiles lists js THEN map per unit (116633-116638) while the
    // sink writes map THEN js (116784-116801): the list order is
    // plan-owned, never derived from the write order.
    let mut unit_listing: Vec<(std::path::PathBuf, Option<std::path::PathBuf>)> = Vec::new();
    // sourceMapDataList is allocated iff a map option is on (116532):
    // `sourceMap || inlineSourceMap` since the h2-6b-m-2 flip.
    let mut source_map_observations: Vec<SourceMapObservation> = Vec::new();
    let map_options_enabled =
        options.source_map == Some(true) || options.inline_source_map == Some(true);
    let mut emit_skipped = false;
    for unit in preflight.plan().units() {
        let EmitRoot::SourceFile(source_id) = unit.root() else {
            return Err(EmitFailure::Unsupported(
                crate::UnsupportedEmitFeature::BundleRoot,
            ));
        };
        let javascript_path = unit.paths().javascript_path().ok_or(EmitFailure::Contract(
            EmitContractViolation::ScriptOutputMissingJavaScriptPath,
        ))?;
        if preflight.is_emit_blocked(host, javascript_path) {
            emit_skipped = true;
            continue;
        }
        let source = host.source_file(*source_id).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(*source_id),
        ))?;
        let syntax = source.syntax().ok_or(EmitFailure::Contract(
            EmitContractViolation::CheckedSyntaxUnavailable(*source_id),
        ))?;

        let mut arena = TransformArena::new();
        let transform_source = arena.add_source(syntax, Some(*source_id));
        let transformers =
            get_script_transformers_with_activity(options, resolver, host, *source_id, activity)?;
        activity.construct_transform_context();
        let mut transformation = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(transform_source)],
            transformers,
            false,
        )?;
        let transform_diagnostics = transformation.diagnostics().to_vec();
        // tsc-port: shouldEmitSourceMaps @6.0.3
        // tsc-hash: 313b475b45d97ba74f69e4e404efd89763caf5fcc7ca9f94c293edf8fdea4f52
        // tsc-span: _tsc.js:116805-116807
        //
        // h2-6b-m-2: `(sourceMap || inlineSourceMap)` and not a `.json`
        // source. The plan's map path carries the EXTERNAL arm only
        // (`sourceMap && !inlineSourceMap && !json`, plan.rs:328); the
        // inline lane records with no planned map path.
        let json_source = source.path().to_string_lossy().ends_with(".json");
        let recording_enabled = map_options_enabled && !json_source;
        let javascript_map_path = unit
            .paths()
            .javascript_map_path()
            .map(std::path::Path::to_path_buf);
        if recording_enabled && options.inline_source_map != Some(true) {
            // the planner's invariant for the external arm
            if javascript_map_path.is_none() {
                return Err(EmitFailure::Contract(
                    EmitContractViolation::SourceMapRecordingUnavailable,
                ));
            }
        }
        let recording_inputs = recording_enabled.then(|| {
            source_map_recording_inputs_for(
                &map_lane_inputs(host),
                options,
                javascript_path,
                source.path(),
            )
        });
        if recording_inputs.is_some() {
            // The slice that ADMITTED the shape: declaration-bearing map
            // emission is H2.6c; otherwise any 6b option on a mapped unit
            // is H2.6b and plain external `sourceMap` stays H2.6a.
            let six_b_option = options.inline_source_map == Some(true)
                || options.inline_sources == Some(true)
                || options.source_root.is_some()
                || options.map_root.is_some();
            activity.observe_runtime_slice(if options.declaration == Some(true) {
                H2RuntimeSlice::H2_6c
            } else if six_b_option {
                H2RuntimeSlice::H2_6b
            } else {
                H2RuntimeSlice::H2_6a
            });
        } else if options.declaration == Some(true) {
            activity.observe_runtime_slice(H2RuntimeSlice::H2_6c);
        }
        let (printed, fallback_source_map) = match printer.print(
            &mut transformation,
            PrintRequest::SourceFile(transform_source),
            recording_inputs.clone(),
        ) {
            Ok(printed) => (printed, None),
            // ES5/System sources with scoped helpers finish JavaScript
            // printing by splicing helpers ahead of the recorded text, so
            // the existing map lane rejects the now-invalid positions. A
            // declaration-bearing unit was unreachable here before H2.6c.
            // Preserve its admitted artifact shape with a deterministic
            // source registration and empty mappings; Wave F records the
            // map facet until the source-map closure wave owns rebasing.
            Err(crate::PrinterError::Unsupported(crate::UnsupportedEmitFeature::JavaScriptMap))
                if options.declaration == Some(true) && recording_inputs.is_some() =>
            {
                let printed = printer.print(
                    &mut transformation,
                    PrintRequest::SourceFile(transform_source),
                    None,
                )?;
                let mut recording = crate::source_map::SourceMapRecording::new(
                    recording_inputs.expect("recording input matched above"),
                );
                recording.set_current_source(transform_source, &syntax.file_name, syntax.text());
                (printed, Some(recording.into_generator()))
            }
            Err(error) => return Err(error.into()),
        };
        if recording_enabled {
            let map_path = javascript_map_path.as_ref();
            let mut generator = fallback_source_map
                .or_else(|| printed.source_map().cloned())
                .ok_or(EmitFailure::Contract(
                    EmitContractViolation::SourceMapRecordingUnavailable,
                ))?;
            let map_json = generator.to_json_string();
            source_map_observations.push(SourceMapObservation::new(
                generator
                    .raw_sources()
                    .iter()
                    .map(|name| std::path::PathBuf::from(name.as_ref()))
                    .collect(),
                map_json.clone().into_boxed_str(),
            ));
            // getSourceMappingURL + the append (116779-116783): one
            // newLine if the writer is mid-line, `sourceMapUrlPos` at the
            // UTF-16 offset where `//#` begins, NO trailing newline. The
            // URL string comes from the h2-6b-m-1 four-way selection; at
            // this floor only the basename default lane is reachable.
            let url = source_mapping_url(
                &map_lane_inputs(host),
                options,
                &map_json,
                javascript_path,
                map_path.map(std::path::PathBuf::as_path),
                source.path(),
            )?;
            let mut javascript_text = printed.text().to_owned();
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
            // Map artifact BEFORE js (116784-116795), EXTERNAL lane only:
            // the inline lane writes no map artifact (116784 gates on
            // sourceMapFilePath; h2-6b.md §4.4). No BOM, no data.
            if let Some(map_path) = map_path {
                artifacts.push(EmitArtifact::javascript_map(
                    map_path.clone(),
                    map_json,
                    Some(vec![source.path().to_path_buf()]),
                ));
            }
            activity.create_javascript_artifact();
            artifacts.push(EmitArtifact::javascript(
                javascript_path,
                javascript_text,
                options.emit_bom == Some(true),
                Some(vec![source.path().to_path_buf()]),
                EmitTextMetadata::new(transform_diagnostics, Some(url_position)),
            ));
        } else {
            activity.create_javascript_artifact();
            artifacts.push(EmitArtifact::javascript(
                javascript_path,
                printed.text(),
                options.emit_bom == Some(true),
                Some(vec![source.path().to_path_buf()]),
                EmitTextMetadata::new(transform_diagnostics, None),
            ));
        }
        unit_listing.push((javascript_path.to_path_buf(), javascript_map_path));
    }

    let mut diagnostics: DiagnosticList = Vec::new();
    let mut written_paths: std::collections::BTreeSet<std::path::PathBuf> =
        std::collections::BTreeSet::new();
    for artifact in artifacts {
        let path = artifact.path().to_path_buf();
        activity.attempt_output_sink_write();
        let include_in_emitted_files = match sink.write(artifact) {
            Ok(EmitWriteDisposition::Written) => true,
            Ok(EmitWriteDisposition::SkippedUnchanged) => false,
            Err(error) => {
                activity.observe_output_sink_failure();
                diagnostics.push(write_diagnostic(&path, error.message()));
                // TypeScript records the attempted output after the host's
                // error callback returns; a callback error is not an
                // unchanged-write suppression.
                true
            }
        };
        if include_in_emitted_files {
            written_paths.insert(path);
        }
    }
    let emitted_files = emitted_files_enabled.then(|| {
        let mut listing = Vec::new();
        for (javascript_path, map_path) in unit_listing {
            if written_paths.contains(&javascript_path) {
                listing.push(javascript_path);
            }
            if let Some(map_path) = map_path {
                if written_paths.contains(&map_path) {
                    listing.push(map_path);
                }
            }
        }
        listing
    });

    Ok(EmitOutcome::new(
        diagnostics,
        emit_skipped,
        emitted_files,
        map_options_enabled.then_some(source_map_observations),
        activity.counters(),
    ))
}

fn write_diagnostic(path: &std::path::Path, message: &str) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Could_not_write_file_0_1,
            &[path.to_string_lossy().into_owned(), message.to_owned()],
        ),
    )
}

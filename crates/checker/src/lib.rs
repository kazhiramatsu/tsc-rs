#![forbid(unsafe_code)]

pub mod access;
pub mod annotate;
pub mod calls;
pub mod check;
pub mod class;
pub mod conditional;
pub mod constraints;
pub mod contextual;
mod display_clone;
mod display_clone_body;
mod display_clone_module;
pub mod elaboration;
pub mod engine;
pub mod evaluate;
pub mod expr;
pub mod facts;
pub mod flow;
pub mod functions;
pub mod globals;
pub mod indexed;
pub mod inference;
pub mod instantiate;
pub mod intersect;
pub mod iterate;
mod js_grammar;
mod jsdoc;
pub mod jsx;
pub mod links;
pub mod literals;
pub mod mapped;
pub mod merge;
pub mod modules;
pub mod narrow;
pub mod operators;
mod plain_js_errors;
pub mod program;
pub mod relate;
pub mod relpin;
pub mod resolve;
pub mod speculate;
pub mod spell;
pub mod state;
pub mod statements;
pub mod structural;
pub mod unions;
mod unused;
pub mod variance;
pub mod widen;

use tsc_diagnostics::{Diagnostic, DiagnosticCategory, DiagnosticList};

pub use tsc_types::CompilerOptions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputFile {
    pub name: String,
    pub text: String,
}

/// Stable caller-owned identity for one source admitted to an authoritative
/// checker run. The token is deliberately independent of the checker's
/// parsed/bound file index: library filtering, unsupported extensions, and
/// same-name shadowing can all change that index.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AuthoritativeSourceToken(pub u32);

/// The exact `ResolutionMode` key used at the host module-resolution seam.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AuthoritativeResolutionMode {
    CommonJs,
    EsNext,
    Unspecified,
}

/// Caller-owned facts for one [`InputFile`]. Metadata slices passed to the
/// authoritative entry are positional peers of their input slices; the file
/// name is repeated so the boundary can validate that relationship rather
/// than assuming it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeSourceMetadata {
    pub token: AuthoritativeSourceToken,
    pub file_name: String,
    /// Exact source-side `sourceFileMayBeEmitted` verdict. This must remain
    /// separate from per-resolution external-library provenance.
    pub may_be_emitted: bool,
    /// Raw `SourceFile.impliedNodeFormat` observed while the source was
    /// created.
    pub implied_node_format: Option<AuthoritativeResolutionMode>,
    /// Effective `getImpliedNodeFormatForEmitWorker` result. This remains
    /// distinct from the raw format: an ordinary file below `node_modules`
    /// can default to CommonJS while a non-Node emit module kind deliberately
    /// ignores that default unless a package scope states its `type`.
    pub implied_node_format_for_emit: Option<AuthoritativeResolutionMode>,
}

/// One exact checker-to-host module lookup. `containing_file` is diagnostic
/// context only; providers must key by the stable source token, specifier,
/// and mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoritativeModuleRequest<'a> {
    pub source_token: AuthoritativeSourceToken,
    pub containing_file: &'a str,
    pub specifier: &'a str,
    pub mode: AuthoritativeResolutionMode,
}

/// Package identity attached by the authoritative resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativePackageId {
    pub name: String,
    pub submodule_name: String,
    pub version: String,
    pub peer_dependencies: Option<String>,
}

/// A loaded source selected by the authoritative host table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeResolvedModule {
    pub target_token: AuthoritativeSourceToken,
    /// Exact `resolvedUsingTsExtension` host fact. Package-map providers must
    /// derive this from the selected raw target before pattern substitution;
    /// the final resolved file extension alone is insufficient for TS2877.
    pub resolved_using_ts_extension: bool,
    pub is_tsx: bool,
    pub is_arbitrary_extension: bool,
    /// The host found this target through an external-library package lookup.
    /// This is an authoritative resolution fact, not a reason to reject an
    /// otherwise loaded source.
    pub is_external_library_import: bool,
    pub package_id: Option<AuthoritativePackageId>,
    /// Per-resolution facts observed by `createModuleNotFoundChain` when an
    /// admitted external JavaScript source has no declarations.
    pub alternate_result: Option<String>,
    pub types_package_exists: bool,
    pub package_bundles_types: bool,
}

/// A successfully resolved target that was deliberately not loaded into the
/// source program, together with the exact facts needed by the TS7016,
/// unloaded-JSX TS6142, and arbitrary-extension TS6263 branches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeUntypedModule {
    pub resolved_file_name: String,
    pub package_name: Option<String>,
    pub alternate_result: Option<String>,
    pub types_package_exists: bool,
    pub package_bundles_types: bool,
    pub resolution_diagnostic: Option<AuthoritativeModuleResolutionDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoritativeModuleResolutionDiagnostic {
    JsxWithoutJsxOption,
    ArbitraryExtensionWithoutOption,
}

/// An unsuccessful authoritative lookup together with host facts that remain
/// observable in the module-not-found diagnostic chain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoritativeNotFoundModule {
    pub alternate_result: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthoritativeModuleResolution {
    Resolved(AuthoritativeResolvedModule),
    Untyped(AuthoritativeUntypedModule),
    NotFound(AuthoritativeNotFoundModule),
}

/// A present table row that this checker slice cannot yet consume
/// losslessly. These are infrastructure failures, never `NotFound`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedAuthoritativeResolution {
    ResolutionDiagnostics,
    ResolvedFileIdentity,
    OriginalPath,
    UnloadedTargetExtension,
    UnloadedTargetAdmission,
    UnloadedJsxWithoutJsxOption,
}

/// Provider-local failure. The checker attaches the exact owned request and
/// publishes it as [`AuthoritativeModuleFailure`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoritativeModuleLookupFailure {
    Missing,
    InvalidSourceToken,
    Unsupported(UnsupportedAuthoritativeResolution),
}

/// Object-safe host boundary used only by the authoritative production
/// entry. Legacy checker entries install no provider and retain their
/// existing in-memory heuristic resolver.
pub trait AuthoritativeModuleProvider {
    fn resolve_module(
        &self,
        request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure>;
}

/// Fail-closed authoritative execution error. The checker records only the
/// first failure and completes internal unwinding without exposing partial
/// diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthoritativeModuleFailure {
    InvalidMetadata {
        detail: String,
    },
    Lookup {
        source_token: AuthoritativeSourceToken,
        containing_file: String,
        specifier: String,
        mode: AuthoritativeResolutionMode,
        failure: AuthoritativeModuleLookupFailure,
    },
    UnknownSourceToken {
        file_index: usize,
        containing_file: String,
    },
    UnknownTargetToken {
        source_token: AuthoritativeSourceToken,
        containing_file: String,
        specifier: String,
        mode: AuthoritativeResolutionMode,
        target_token: AuthoritativeSourceToken,
    },
}

impl std::fmt::Display for AuthoritativeModuleFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMetadata { detail } => {
                write!(formatter, "invalid authoritative checker metadata: {detail}")
            }
            Self::Lookup {
                containing_file,
                specifier,
                mode,
                failure,
                ..
            } => write!(
                formatter,
                "authoritative module lookup failed for ({containing_file}, {specifier:?}, {mode:?}): {failure:?}"
            ),
            Self::UnknownSourceToken {
                file_index,
                containing_file,
            } => write!(
                formatter,
                "authoritative checker file {file_index} ({containing_file}) has no source token"
            ),
            Self::UnknownTargetToken {
                containing_file,
                specifier,
                mode,
                target_token,
                ..
            } => write!(
                formatter,
                "authoritative module lookup for ({containing_file}, {specifier:?}, {mode:?}) selected unavailable source token {}",
                target_token.0
            ),
        }
    }
}

impl std::error::Error for AuthoritativeModuleFailure {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckResult {
    pub diagnostics: DiagnosticList,
    /// `program.getSyntacticDiagnostics(sourceFile)`, flattened in
    /// fixture-file ordinal order.
    pub syntactic_diagnostics: DiagnosticList,
    /// `program.getSemanticDiagnostics(sourceFile)`, flattened in
    /// fixture-file ordinal order.
    pub semantic_diagnostics: DiagnosticList,
    /// `program.getGlobalDiagnostics()` for the owned no-emit entry.
    ///
    /// The legacy conformance entry observes only per-file getters and keeps
    /// this empty so its established lazy-global timing remains unchanged.
    pub global_diagnostics: DiagnosticList,
    /// `program.getSuggestionDiagnostics(sourceFile)`, flattened in
    /// fixture-file ordinal order. Unlike the syntactic and semantic
    /// getters, tsc does not sort/deduplicate this pass.
    pub suggestion_diagnostics: DiagnosticList,
    /// Authoritative public-getter observations. The outer vector is
    /// fixture-file ordinal order; each pass retains the order and
    /// multiplicity returned by its corresponding tsc getter.
    pub file_diagnostics: Vec<FileDiagnosticPasses>,
    /// Source ranges whose semantic check stopped at an explicit
    /// partial-model boundary. This is audit evidence, not a
    /// diagnostic filter. Typed oracle-crash containment is
    /// deliberately excluded; its range participates only in internal
    /// comment-directive accounting.
    pub partial_checks: Vec<PartialCheck>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileDiagnosticPasses {
    pub file_name: String,
    pub syntactic: DiagnosticList,
    pub semantic: DiagnosticList,
    pub suggestion: DiagnosticList,
}

/// Coarse production-worker boundaries in the checker driver.
///
/// Formatting is owned by the caller because it occurs after
/// `CheckResult` has been produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckPhase {
    Parse,
    Bind,
    Check,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialCheck {
    pub file_name: String,
    /// UTF-16 offset, matching diagnostic and oracle coordinates.
    pub start: u32,
    pub length: u32,
    pub reason: String,
}

/// tsc getSupportedExtensions: JS roots only join the program with allowJs.
fn is_supported_source_file_name(name: &str, allow_js: bool) -> bool {
    let ts_like = [".ts", ".tsx", ".mts", ".cts", ".json"];
    ts_like.iter().any(|extension| name.ends_with(extension)) || (allow_js && is_js_file_name(name))
}

/// tsc-port: hasJSFileExtension @6.0.3
/// tsc-hash: 26f2de10186fd7377e0fc90d254165421f27320a1b95dca68e43ee8f2f71128d
/// tsc-span: _tsc.js:18654-18656
pub(crate) fn is_js_file_name(name: &str) -> bool {
    [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// tsc check directive: extractPragmas walks
/// getLeadingCommentRanges(text, 0) — single-line comments BEFORE the
/// first token — and the LAST ts-check/ts-nocheck pragma wins
/// (processPragmasIntoFields); skipTypeChecking then drops the file's
/// bind+check diagnostics whole (parse diagnostics stay). Pragma names
/// lowercase; the name must end at whitespace/colon/EOL like
/// `@([^\s:]+)`. This producer stays TEXTUAL (exact over leading
/// trivia, which is all extractPragmas reads); the 5.8e directive
/// completion moved @ts-ignore/@ts-expect-error to scanner-collected
/// SourceFile.comment_directives — swap this too if the parser ever
/// grows real pragma processing (M8 surface).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckDirective {
    Check,
    NoCheck,
}

fn check_directive(text: &str) -> Option<CheckDirective> {
    let mut rest = text;
    // getLeadingCommentRanges starts after a leading shebang. Keep
    // this test on the RAW offset zero: a BOM before `#!` makes it an
    // ordinary token sequence, not shebang trivia.
    if let Some(after) = rest.strip_prefix("#!") {
        let line_end = after
            .find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .unwrap_or(after.len());
        rest = &after[line_end..];
    }
    let mut directive = None;
    loop {
        // JS WhiteSpace includes BOM; Rust's is_whitespace does not.
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
        if let Some(after) = rest.strip_prefix("//") {
            let line_end = after
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .unwrap_or(after.len());
            let comment = &after[..line_end];
            // singleLinePragmaRegEx: ^///?\s*@([^\s:]+)
            let body = comment.strip_prefix('/').unwrap_or(comment).trim_start();
            if let Some(name_and_tail) = body.strip_prefix('@') {
                let name_end = name_and_tail
                    .find(|c: char| c.is_whitespace() || c == ':')
                    .unwrap_or(name_and_tail.len());
                match name_and_tail[..name_end].to_ascii_lowercase().as_str() {
                    "ts-nocheck" => directive = Some(CheckDirective::NoCheck),
                    "ts-check" => directive = Some(CheckDirective::Check),
                    _ => {}
                }
            }
            rest = &after[line_end..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("/*") {
            match after.find("*/") {
                Some(end) => {
                    rest = &after[end + 2..];
                    continue;
                }
                None => break,
            }
        }
        break;
    }
    directive
}

fn can_include_bind_and_check_diagnostics(
    javascript_file: bool,
    directive: Option<CheckDirective>,
    options: &CompilerOptions,
) -> bool {
    match directive {
        Some(CheckDirective::NoCheck) => false,
        // A per-file @ts-check overrides an explicit checkJs:false.
        Some(CheckDirective::Check) => true,
        None => !javascript_file || options.check_js != Some(false),
    }
}

/// tsc isPlainJsFile (12876): a JS/JSX file is "plain" only when
/// neither a per-file check directive nor the project-level checkJs
/// option was supplied. Checked JS uses the same comment-directive
/// merge as TypeScript files.
fn is_plain_js_file(
    javascript_file: bool,
    directive: Option<CheckDirective>,
    options: &CompilerOptions,
) -> bool {
    javascript_file && directive.is_none() && options.check_js.is_none()
}

/// tsc-port: markPrecedingCommentDirectiveLine @6.0.3
/// tsc-hash: 5fd3ed53a22559eabfbc34ecee39efa38b2df133d5cc00e86dcd42ecae6ea88b
/// tsc-span: _tsc.js:123766-123784
///
/// getDiagnosticsWithPrecedingDirectives (123756) over one file's
/// bind+check list: keep a diagnostic only when no comment directive
/// precedes it. Directives come from the SCANNER
/// (SourceFile.comment_directives) and key on the line of range.end —
/// the line holding a single-line comment, or the line holding a
/// multi-line comment's `*/` (createCommentDirectivesMap 12963; a
/// second directive ending on the same line collapses into it). The
/// walk starts one line above the diagnostic and stops at the first
/// line that is non-empty and not a `//` comment after a JS trim —
/// unlike the retired interim filter, block-comment shell lines STOP
/// the walk, exactly as in tsc.
///
fn preceding_comment_directive_line(
    text: &str,
    byte_line_starts: &[usize],
    directive_lines: &std::collections::HashSet<usize>,
    utf16_line_starts: &[u32],
    diagnostic_start: u32,
) -> Option<usize> {
    let diagnostic_line = match utf16_line_starts.binary_search(&diagnostic_start) {
        Ok(line) => line,
        Err(insert) => insert.saturating_sub(1),
    };
    let mut line = diagnostic_line;
    while line > 0 {
        line -= 1;
        if directive_lines.contains(&line) {
            return Some(line);
        }
        let start = byte_line_starts[line];
        let end = byte_line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(text.len());
        let trimmed = text[start..end].trim_matches(tsc_syntax::is_js_whitespace);
        if !trimmed.is_empty() && !trimmed.starts_with("//") {
            break;
        }
    }
    None
}

fn filter_by_comment_directives_and_mark_used(
    source: &tsc_syntax::SourceFile,
    diagnostics: impl Iterator<Item = tsc_diagnostics::Diagnostic>,
    mut used_directive_lines: Option<&mut std::collections::HashSet<usize>>,
) -> Vec<tsc_diagnostics::Diagnostic> {
    // getMergedBindAndCheckDiagnostics (123744): no directives, no
    // filtering.
    if source.comment_directives.is_empty() {
        return diagnostics.collect();
    }
    let text = source.text.as_str();
    // LineMap.line_starts are UTF-16 offsets; build BYTE line starts
    // with the same break set (\r\n, \r, \n, U+2028, U+2029) for text
    // slicing and for placing the byte-offset directive ranges.
    let byte_line_starts = compute_byte_line_starts(text);
    let line_of_byte = |offset: usize| -> usize {
        match byte_line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        }
    };
    let directive_lines: std::collections::HashSet<usize> = source
        .comment_directives
        .iter()
        .map(|directive| line_of_byte(directive.end as usize))
        .collect();
    // Diagnostic.start is UTF-16, matching line_starts' units.
    let utf16_line_starts: &[u32] = &source.line_map.line_starts;
    let mut result = Vec::new();
    for diagnostic in diagnostics {
        // Suggestion diagnostics come from getSuggestionDiagnostics,
        // outside getMergedBindAndCheckDiagnostics' comment-directive
        // filter. They neither consume @ts-ignore/@ts-expect-error nor
        // disappear behind one.
        if diagnostic.category() == DiagnosticCategory::Suggestion {
            result.push(diagnostic);
            continue;
        }
        let Some(start) = diagnostic.start else {
            result.push(diagnostic);
            continue;
        };
        if let Some(line) = preceding_comment_directive_line(
            text,
            &byte_line_starts,
            &directive_lines,
            utf16_line_starts,
            start,
        ) {
            if let Some(used) = used_directive_lines.as_deref_mut() {
                used.insert(line);
            }
            continue;
        }
        result.push(diagnostic);
    }
    result
}

/// Recorded intent (b0cd3802; m4-review DR-F6): only the START face of
/// each partial range consumes a preceding directive. Containments are
/// SHELL-shaped — rows elsewhere in the bracketed region still fire —
/// so a blanket interior exemption would silence unused-directive
/// 2578s the oracle reports (the
/// directive_inside_a_checked_mapped_type_is_not_blanket_exempted pin
/// forces this split).
fn mark_comment_directives_for_partial_ranges(
    source: &tsc_syntax::SourceFile,
    partial_ranges: &[(u32, u32)],
    used_directive_lines: &mut std::collections::HashSet<usize>,
) {
    if source.comment_directives.is_empty() || partial_ranges.is_empty() {
        return;
    }
    let text = source.text.as_str();
    let byte_line_starts = compute_byte_line_starts(text);
    let line_of_byte = |offset: usize| -> usize {
        match byte_line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        }
    };
    let directive_lines: std::collections::HashSet<usize> = source
        .comment_directives
        .iter()
        .map(|directive| line_of_byte(directive.end as usize))
        .collect();

    for &(start, _) in partial_ranges {
        let start = tsc_syntax::skip_trivia(text, start as usize);
        let start_utf16 = source
            .line_map
            .byte_to_utf16
            .get(start)
            .copied()
            .unwrap_or(start as u32);
        if let Some(line) = preceding_comment_directive_line(
            text,
            &byte_line_starts,
            &directive_lines,
            &source.line_map.line_starts,
            start_utf16,
        ) {
            used_directive_lines.insert(line);
        }
    }
}

fn unused_expect_error_diagnostics(
    source: &tsc_syntax::SourceFile,
    used_directive_lines: &std::collections::HashSet<usize>,
) -> Vec<tsc_diagnostics::Diagnostic> {
    use tsc_syntax::CommentDirectiveKind;

    if source.comment_directives.is_empty() {
        return Vec::new();
    }
    let byte_line_starts = compute_byte_line_starts(&source.text);
    let line_of_byte = |offset: usize| -> usize {
        match byte_line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        }
    };
    // createCommentDirectivesMap uses Map construction, so the last
    // directive ending on a line replaces earlier directives there.
    let mut directives_by_line = std::collections::BTreeMap::new();
    for directive in &source.comment_directives {
        directives_by_line.insert(line_of_byte(directive.end as usize), *directive);
    }
    directives_by_line
        .into_iter()
        .filter_map(|(line, directive)| {
            if directive.kind != CommentDirectiveKind::ExpectError
                || used_directive_lines.contains(&line)
            {
                return None;
            }
            let start = source
                .line_map
                .byte_to_utf16
                .get(directive.pos as usize)
                .copied()
                .unwrap_or(directive.pos);
            let end = source
                .line_map
                .byte_to_utf16
                .get(directive.end as usize)
                .copied()
                .unwrap_or(directive.end);
            Some(tsc_diagnostics::Diagnostic::new(
                Some(source.file_name.clone()),
                Some(start),
                Some(end.saturating_sub(start)),
                tsc_diagnostics::MessageChain::new(
                    &tsc_diagnostics::gen::Unused_ts_expect_error_directive,
                    &[],
                ),
            ))
        })
        .collect()
}

/// tsc-port: filterSemanticDiagnostics @6.0.3
/// tsc-hash: 5585b227fa5ab80bc9c14222bfcb199f66a2d8fb5d2fa640667c188b5152fa22
/// tsc-span: _tsc.js:125664-125666
///
/// tsc filters each file's getSemanticDiagnostics output with
/// `!d.skippedOn || !option[d.skippedOn]` (getSemanticDiagnosticsForFile
/// 123698). The only key any emitter passes is "noEmit" (the checker
/// collision band 83235-83353 + the __esModule marker 90103), no
/// parse/bind emitter sets it, and the predicate is per-diagnostic —
/// so one pass over the aggregate list is equivalent to tsc's
/// per-file filter. Runs beside filter_by_comment_directives at the
/// program-layer diagnostics-finalize seam (m4-58 §0 skippedOn).
fn filter_semantic_diagnostics(
    diagnostics: &mut tsc_diagnostics::DiagnosticList,
    options: &CompilerOptions,
) {
    if options.no_emit == Some(true) {
        diagnostics.retain(|diagnostic| !diagnostic.skipped_on_no_emit);
    }
}

/// Byte-offset line starts with tsc's line-break set (\r\n, \r, \n,
/// U+2028, U+2029) — index-compatible with LineMap.line_starts.
fn compute_byte_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    let mut chars = text.char_indices().peekable();
    while let Some((byte, ch)) = chars.next() {
        match ch {
            '\r' => {
                let mut next_start = byte + 1;
                if let Some(&(next_byte, '\n')) = chars.peek() {
                    chars.next();
                    next_start = next_byte + 1;
                }
                starts.push(next_start);
            }
            '\n' => starts.push(byte + 1),
            '\u{2028}' | '\u{2029}' => starts.push(byte + ch.len_utf8()),
            _ => {}
        }
    }
    starts
}

/// tsrs-native: public single-lib-list adapter around the checker
/// program harness; tsc exposes Program/TypeChecker objects instead.
pub fn check_program(files: &[InputFile], options: &CompilerOptions) -> CheckResult {
    check_program_with_libs(&[], files, options)
}

/// Program construction under the oracle contract
/// (m4-lib-loading-steps.md §1): `libs` are ORDINARY files prepended
/// to the program in the order given (the harness's priority-sorted
/// expansion; the oracle host runs noLib:true with the same list as
/// prepended roots, so `<reference lib>` is inert and getSourceFiles
/// order == libs ++ files). They ride the same parse/bind/globals-
/// merge pipeline through a per-lib-set CACHED prefix (LibBundle:
/// same-key programs share one parsed+bound copy — exact, because
/// libs are the program prefix and their id bases are therefore
/// identical across programs). Lib files are never CHECKED and no
/// diagnostic band of theirs surfaces — tsc checks files lazily per
/// getDiagnostics(file) call and the oracle driver only ever asks for
/// fixture files, so a lib file's checkSourceFileWorker never runs
/// and diagnostics FILED under a lib file are never collected.
/// tsrs-native: public cwd-defaulting adapter around the Rust
/// in-memory program harness; tsc has no function with this API.
pub fn check_program_with_libs(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
) -> CheckResult {
    check_program_with_libs_at(libs, files, options, "/")
}

/// Resolve the harness cwd in the same order as
/// `normalizeFileName(path.posix.resolve(cwd))` in program-host.mjs.
///
/// Backslashes must remain ordinary characters while `.` and `..`
/// segments are resolved. Only after that POSIX-path pass does the
/// oracle turn them into separators with normalizeFileName.
fn resolve_host_current_directory(current_directory: &str) -> String {
    let raw_path = if current_directory.starts_with('/') {
        current_directory.to_owned()
    } else {
        let process_cwd = std::env::current_dir()
            .map(|dir| {
                let raw = dir.to_string_lossy().into_owned();
                if cfg!(windows) {
                    let flipped = raw.replace('\\', "/");
                    match flipped.find('/') {
                        Some(root) => flipped[root..].to_owned(),
                        None => flipped,
                    }
                } else {
                    raw
                }
            })
            .unwrap_or_default();
        format!("{process_cwd}/{current_directory}")
    };

    let absolute = raw_path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in raw_path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| *last != "..") {
                    segments.pop();
                } else if !absolute {
                    segments.push(segment);
                }
            }
            other => segments.push(other),
        }
    }
    let normalized = segments.join("/");
    let resolved = if absolute {
        format!("/{normalized}")
    } else if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized
    };
    resolved.replace('\\', "/")
}

fn is_supported_path_reference(file_name: &str, options: &CompilerOptions) -> bool {
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| file_name.ends_with(extension))
        || (options.allow_js && is_js_file_name(file_name))
        || (options.resolve_json_module_effective() && file_name.ends_with(".json"))
}

/// tsc-port: createProgram/getSourceFileFromReferenceWorker @6.0.3
/// tsc-hash: 7bf2d246bac2296b6c17a46308c9c67109a0318702c78b61b086fa4bb353581f
/// tsc-span: _tsc.js:124173-124211
///
/// Producer-owned M7 8.5a face: a leading `/// <reference path=... />`
/// with an explicit supported extension reaches the host lookup and
/// reports 6053 when absent. Extensionless, unsupported-extension,
/// redirect, config, and project-reference faces remain outside this
/// slice.
fn missing_path_reference_diagnostics(
    sources: &[tsc_syntax::SourceFile],
    host_files: impl Iterator<Item = String>,
    options: &CompilerOptions,
    current_directory: &str,
) -> DiagnosticList {
    let known_paths: std::collections::HashSet<String> = host_files.collect();
    let mut diagnostics = Vec::new();
    for source in sources {
        let source_path =
            state::CheckerState::normalize_program_path(&source.file_name, current_directory);
        let source_directory = source_path
            .rsplit_once('/')
            .map_or("/", |(directory, _)| directory);
        for reference in &source.referenced_files {
            if !is_supported_path_reference(&reference.file_name, options) {
                continue;
            }
            let resolved =
                state::CheckerState::normalize_program_path(&reference.file_name, source_directory);
            if known_paths.contains(&resolved) {
                continue;
            }
            diagnostics.push(Diagnostic::new(
                Some(source.file_name.clone()),
                Some(reference.pos),
                Some(reference.end.saturating_sub(reference.pos)),
                tsc_diagnostics::MessageChain::new(
                    &tsc_diagnostics::gen::File_0_not_found,
                    &[resolved],
                ),
            ));
        }
    }
    diagnostics
}

fn parse_host_package_json(text: &str) -> Option<serde_json::Value> {
    // tsc's JSON scanner accepts a leading BOM as whitespace;
    // serde_json requires the host boundary to remove it first.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    serde_json::from_str(text).ok()
}

/// tsrs-native: the cwd-carrying entry — `current_directory` is the
/// harness ProgramJson `cwd` (tsc host.getCurrentDirectory), which the
/// oracle host uses to absolutize every program fileName. It follows
/// path.posix.resolve (program-host.mjs decodeProgram): a RELATIVE cwd
/// — including a "\\"-led one, which posix.resolve does NOT treat as
/// absolute — roots at Node's posixCwd (the process working directory;
/// drive-stripped on Windows), not "/". Display-side
/// path rendering roots relative file names against it; the "/"-rooted
/// resolution world is unaffected (see
/// CheckerState::host_current_directory).
pub fn check_program_with_libs_at(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
) -> CheckResult {
    check_program_with_libs_at_observed(libs, files, options, current_directory, |_| {})
}

/// tsrs-native: prepare an opaque, process-lifetime standard-library bundle for the
/// differential conformance harness.
///
/// The returned handle is only a lookup hint. Every use revalidates the
/// projected parser/binder options and the exact ordered library names and
/// texts before reusing it; a mismatch falls back to the ordinary cache.
/// Production program sessions do not use this API.
#[doc(hidden)]
pub fn prepare_harness_lib_bundle(
    libs: &[InputFile],
    options: &CompilerOptions,
) -> Option<PreparedHarnessLibBundle> {
    if std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_some_and(|value| value == "0") {
        return None;
    }
    let libs = libs.iter().collect::<Vec<_>>();
    (!libs.is_empty()).then(|| PreparedHarnessLibBundle {
        bundle: lib_bundle(&libs, options),
    })
}

/// tsrs-native: return the opaque parser/binder option projection used by prepared harness
/// bundles. Harnesses may use this as a small cache key without learning or
/// duplicating the projection's fields.
#[doc(hidden)]
pub fn harness_lib_bundle_options_key(options: &CompilerOptions) -> HarnessLibBundleOptionsKey {
    HarnessLibBundleOptionsKey(lib_bundle_options(options))
}

/// tsrs-native: run one harness case with a previously prepared standard-library lookup
/// hint. Exact validation and cache-off behavior are identical to
/// [`check_program_with_libs_at`].
#[doc(hidden)]
pub fn check_program_with_prepared_harness_libs_at(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
    prepared: PreparedHarnessLibBundle,
) -> CheckResult {
    let cache_enabled = std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_none_or(|value| value != "0");
    let mut observe_phase = |_| {};
    check_program_with_libs_at_observed_cache_mode_prepared(
        libs,
        files,
        options,
        current_directory,
        cache_enabled,
        Some(prepared),
        &mut observe_phase,
    )
}

/// tsrs-native: phase-observed adapter around the batch checker driver.
/// The production-worker entry point. The observer is invoked exactly
/// once before each coarse checker phase and never from a node visit,
/// keeping the ordinary checker path allocation- and branch-free at
/// node granularity.
pub fn check_program_with_libs_at_observed(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
    mut observe_phase: impl FnMut(CheckPhase),
) -> CheckResult {
    let cache_enabled = std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_none_or(|value| value != "0");
    check_program_with_libs_at_observed_cache_mode(
        libs,
        files,
        options,
        current_directory,
        cache_enabled,
        &mut observe_phase,
    )
}

fn check_program_with_libs_at_observed_cache_mode(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
    cache_enabled: bool,
    observe_phase: &mut impl FnMut(CheckPhase),
) -> CheckResult {
    check_program_with_libs_at_observed_cache_mode_prepared(
        libs,
        files,
        options,
        current_directory,
        cache_enabled,
        None,
        observe_phase,
    )
}

fn check_program_with_libs_at_observed_cache_mode_prepared(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
    cache_enabled: bool,
    prepared: Option<PreparedHarnessLibBundle>,
    observe_phase: &mut impl FnMut(CheckPhase),
) -> CheckResult {
    observe_phase(CheckPhase::Parse);

    let fixture_names: std::collections::HashSet<&str> =
        files.iter().map(|file| file.name.as_str()).collect();
    let effective_libs: Vec<&InputFile> = libs
        .iter()
        .filter(|lib| !fixture_names.contains(lib.name.as_str()))
        .collect();

    if !effective_libs.is_empty() && !cache_enabled {
        // Cache-off is the L3 A/B path. Keep the parsed and bound prefix local
        // so repeated disabled-cache calls do not leak one bundle each.
        let bundle_options = lib_bundle_options(options);
        let lib_sources = parse_lib_sources(&effective_libs, &bundle_options);
        let lib_binders = bind_lib_sources(&lib_sources, &bundle_options);
        return check_program_with_prebound_libs_at_observed(
            libs,
            files,
            options,
            current_directory,
            &lib_sources,
            &lib_binders,
            false,
            observe_phase,
            None,
        )
        .result;
    }

    let bundle = (!effective_libs.is_empty()).then(|| {
        let bundle_options = lib_bundle_options(options);
        prepared
            .and_then(|prepared| prepared.validated(&effective_libs, &bundle_options))
            .unwrap_or_else(|| lib_bundle(&effective_libs, options))
    });
    let (lib_sources, lib_binders): (&[tsc_syntax::SourceFile], &[tsc_binder::Binder<'_>]) =
        match bundle {
            Some(bundle) => (bundle.sources, bundle.binders),
            None => (&[], &[]),
        };

    check_program_with_prebound_libs_at_observed(
        libs,
        files,
        options,
        current_directory,
        lib_sources,
        lib_binders,
        false,
        observe_phase,
        None,
    )
    .result
}

/// tsrs-native: run one owned-lib batch for the no-emit program session.
///
/// Execute one owned batch program without entering the process-lifetime lib
/// bundle cache. Library sources, binders, and all checker borrows are local
/// to this call and are dropped before it returns.
pub fn check_program_with_owned_libs_at(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
) -> CheckResult {
    let fixture_names: std::collections::HashSet<&str> =
        files.iter().map(|file| file.name.as_str()).collect();
    let effective_libs: Vec<&InputFile> = libs
        .iter()
        .filter(|lib| !fixture_names.contains(lib.name.as_str()))
        .collect();
    let bundle_options = lib_bundle_options(options);
    let lib_sources = parse_lib_sources(&effective_libs, &bundle_options);
    let lib_binders = bind_lib_sources(&lib_sources, &bundle_options);
    let mut observe_phase = |_| {};

    check_program_with_prebound_libs_at_observed(
        libs,
        files,
        options,
        current_directory,
        &lib_sources,
        &lib_binders,
        true,
        &mut observe_phase,
        None,
    )
    .result
}

struct AuthoritativeRun<'a> {
    provider: &'a dyn AuthoritativeModuleProvider,
    lib_metadata: Vec<AuthoritativeSourceMetadata>,
    file_metadata: Vec<AuthoritativeSourceMetadata>,
}

struct CheckExecution {
    result: CheckResult,
    authoritative_failure: Option<AuthoritativeModuleFailure>,
}

/// tsrs-native: run one owned checker batch whose module lookups are supplied exclusively
/// by an exact caller-owned table. The legacy in-memory resolver is never a
/// fallback while `provider` is installed.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: &str,
    provider: &dyn AuthoritativeModuleProvider,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        false,
    )
}

/// tsrs-native: conformance-harness adapter for authoritative module facts.
///
/// Unlike [`check_program_with_authoritative_modules_at`], this entry may
/// reuse the harness's exact-match, process-lifetime lib bundle. Production
/// H0 sessions must keep using the owned entry above; this exists only to
/// avoid reparsing and rebinding the same vendored lib prefix for every
/// conformance case. `TSRS_LIB_BUNDLE_CACHE=0` retains the owned path for the
/// cache-off evidence run.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_harness_cached(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: &str,
    provider: &dyn AuthoritativeModuleProvider,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let cache_enabled = std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_none_or(|value| value != "0");
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        cache_enabled,
    )
}

#[allow(clippy::too_many_arguments)]
fn check_program_with_authoritative_modules_at_cache_mode(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: &str,
    provider: &dyn AuthoritativeModuleProvider,
    cache_enabled: bool,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    validate_authoritative_metadata(libs, lib_metadata, "library")?;
    validate_authoritative_metadata(files, file_metadata, "program")?;
    let mut seen_tokens = std::collections::HashSet::new();
    for source in lib_metadata.iter().chain(file_metadata) {
        if !seen_tokens.insert(source.token) {
            return Err(AuthoritativeModuleFailure::InvalidMetadata {
                detail: format!(
                    "authoritative source token {} occurs more than once",
                    source.token.0
                ),
            });
        }
    }

    let fixture_names: std::collections::HashSet<&str> =
        files.iter().map(|file| file.name.as_str()).collect();
    let mut effective_libs = Vec::new();
    let mut effective_lib_metadata = Vec::new();
    for (lib, metadata) in libs.iter().zip(lib_metadata) {
        if !fixture_names.contains(lib.name.as_str()) {
            effective_libs.push(lib);
            effective_lib_metadata.push(metadata.clone());
        }
    }
    let run = AuthoritativeRun {
        provider,
        lib_metadata: effective_lib_metadata,
        file_metadata: file_metadata.to_vec(),
    };
    let mut observe_phase = |_| {};
    let execution = if cache_enabled {
        let bundle = (!effective_libs.is_empty()).then(|| lib_bundle(&effective_libs, options));
        let (lib_sources, lib_binders): (&[tsc_syntax::SourceFile], &[tsc_binder::Binder<'_>]) =
            match bundle {
                Some(bundle) => (bundle.sources, bundle.binders),
                None => (&[], &[]),
            };
        check_program_with_prebound_libs_at_observed(
            libs,
            files,
            options,
            current_directory,
            lib_sources,
            lib_binders,
            true,
            &mut observe_phase,
            Some(&run),
        )
    } else {
        let bundle_options = lib_bundle_options(options);
        let lib_sources = parse_lib_sources(&effective_libs, &bundle_options);
        let lib_binders = bind_lib_sources(&lib_sources, &bundle_options);
        check_program_with_prebound_libs_at_observed(
            libs,
            files,
            options,
            current_directory,
            &lib_sources,
            &lib_binders,
            true,
            &mut observe_phase,
            Some(&run),
        )
    };
    match execution.authoritative_failure {
        Some(failure) => Err(failure),
        None => Ok(execution.result),
    }
}

fn validate_authoritative_metadata(
    inputs: &[InputFile],
    metadata: &[AuthoritativeSourceMetadata],
    kind: &str,
) -> Result<(), AuthoritativeModuleFailure> {
    if inputs.len() != metadata.len() {
        return Err(AuthoritativeModuleFailure::InvalidMetadata {
            detail: format!(
                "authoritative {kind} metadata has {} rows for {} inputs",
                metadata.len(),
                inputs.len()
            ),
        });
    }
    for (index, (input, source)) in inputs.iter().zip(metadata).enumerate() {
        if input.name != source.file_name {
            return Err(AuthoritativeModuleFailure::InvalidMetadata {
                detail: format!(
                    "authoritative {kind} metadata row {index} names {:?}, input is {:?}",
                    source.file_name, input.name
                ),
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_program_with_prebound_libs_at_observed(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: &str,
    lib_sources: &[tsc_syntax::SourceFile],
    lib_binders: &[tsc_binder::Binder<'_>],
    collect_global_diagnostics: bool,
    observe_phase: &mut impl FnMut(CheckPhase),
    authoritative_run: Option<&AuthoritativeRun<'_>>,
) -> CheckExecution {
    let mut file_diagnostics = Vec::new();
    let mut partial_checks = Vec::new();
    let mut global_diagnostics = Vec::new();
    let mut authoritative_failure = None;
    // getImpliedNodeFormatForFileWorker's package-scope input. Build it
    // before parsing because getSetExternalModuleIndicator's Auto mode
    // consults the implied format while SourceFiles are created.
    let host_package_json_module_types: std::collections::HashMap<
        String,
        state::PackageJsonModuleType,
    > = files
        .iter()
        .filter(|file| {
            file.name
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|name| name == "package.json")
        })
        .map(|file| {
            let module_type = parse_host_package_json(&file.text)
                .and_then(|value| {
                    value
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .map(|value| match value {
                            "module" => state::PackageJsonModuleType::Module,
                            "commonjs" => state::PackageJsonModuleType::CommonJs,
                            _ => state::PackageJsonModuleType::Other,
                        })
                })
                .unwrap_or(state::PackageJsonModuleType::Missing);
            (
                state::CheckerState::normalize_program_path(&file.name, ""),
                module_type,
            )
        })
        .collect();
    // Fixture-file shadowing (unchanged from the libless world): a
    // later file with the same name shadows an earlier one entirely.
    let mut last_index_by_name = std::collections::BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        last_index_by_name.insert(file.name.as_str(), index);
    }

    // Fixture parse pass (M4 5.0): files parse in program order with
    // contiguous NodeId/NodeArrayId bases CONTINUING FROM THE LIB
    // PREFIX so the checker sees tsc's one-heap identity space. JSON
    // files remain in that same program: the binder publishes their
    // root value as the module's default/export= property.
    let mut program_sources: Vec<tsc_syntax::SourceFile> = Vec::new();
    let mut authoritative_program_metadata = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if last_index_by_name.get(file.name.as_str()) != Some(&index) {
            continue;
        }
        // tsc createProgram only loads roots with supported extensions;
        // anything else (.txt, extensionless, .js without allowJs) never
        // yields syntactic diagnostics.
        if !is_supported_source_file_name(&file.name, options.allow_js) {
            continue;
        }
        let authoritative_implied_node_format = authoritative_run
            .and_then(|run| run.file_metadata.get(index))
            .and_then(|source| source.implied_node_format);
        if let Some(run) = authoritative_run {
            authoritative_program_metadata.push(run.file_metadata[index].clone());
        }
        // tsc ensureScriptKind: .json programs parse as JSON values.
        if file.name.ends_with(".json") {
            let (node_id_base, node_array_id_base) = match program_sources.last() {
                Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
                None => lib_sources
                    .last()
                    .map(|previous| (previous.arena.node_end(), previous.arena.array_end()))
                    .unwrap_or((0, 0)),
            };
            let source_file = tsc_syntax::parse_json_text_with_bases(
                file.name.clone(),
                file.text.clone(),
                node_id_base,
                node_array_id_base,
            );
            let mut syntactic = source_file.parse_diagnostics.clone();
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut syntactic);
            file_diagnostics.push(FileDiagnosticPasses {
                file_name: source_file.file_name.clone(),
                syntactic,
                semantic: Vec::new(),
                suggestion: Vec::new(),
            });
            program_sources.push(source_file);
            continue;
        }
        // tsc getLanguageVariant: JSX scanning for TSX/JSX/JS script kinds.
        let javascript_file = is_js_file_name(&file.name);
        let language_variant = if file.name.ends_with(".tsx") || javascript_file {
            tsc_syntax::LanguageVariant::Jsx
        } else {
            tsc_syntax::LanguageVariant::Standard
        };
        // getSetExternalModuleIndicator (17973-17993): syntax-based
        // indicators stay in the parser; this seam supplies the
        // option/host-dependent Force and Auto inputs.
        let is_declaration_file = file.name.ends_with(".d.ts")
            || file.name.ends_with(".d.cts")
            || file.name.ends_with(".d.mts");
        let module_detection = options.emit_module_detection_kind();
        let force_external_module = !is_declaration_file
            && match module_detection {
                // Force: every non-declaration file is a module.
                3 => true,
                // Auto: explicit module formats always count; for
                // ordinary TS/JS files an ESM package scope counts
                // when getImpliedNodeFormatForFileWorker would read it.
                2 => {
                    let explicit_module_format = [".cjs", ".cts", ".mjs", ".mts"]
                        .iter()
                        .any(|extension| file.name.ends_with(extension));
                    if explicit_module_format {
                        true
                    } else {
                        let normalized =
                            state::CheckerState::normalize_program_path(&file.name, "");
                        let package_lookup_enabled = (3..=99)
                            .contains(&options.emit_module_resolution_kind())
                            || normalized
                                .split('/')
                                .any(|segment| segment == "node_modules");
                        let package_eligible = [".ts", ".tsx", ".js", ".jsx"]
                            .iter()
                            .any(|extension| file.name.ends_with(extension));
                        let package_scope_is_module = if authoritative_run.is_some() {
                            authoritative_implied_node_format
                                == Some(AuthoritativeResolutionMode::EsNext)
                        } else if package_lookup_enabled && package_eligible {
                            let mut directory = normalized
                                .rsplit_once('/')
                                .map(|(directory, _)| directory)
                                .unwrap_or("");
                            loop {
                                let package_json = if directory.is_empty() {
                                    "/package.json".to_owned()
                                } else {
                                    format!("{directory}/package.json")
                                };
                                if let Some(&module_type) =
                                    host_package_json_module_types.get(&package_json)
                                {
                                    break module_type == state::PackageJsonModuleType::Module;
                                }
                                let Some((parent, _)) = directory.rsplit_once('/') else {
                                    break false;
                                };
                                directory = parent;
                            }
                        } else {
                            false
                        };
                        package_scope_is_module
                    }
                }
                // Legacy (and invalid values, which option validation
                // owns) uses syntax indicators only.
                _ => false,
            };
        let detect_external_module_from_jsx =
            !is_declaration_file && module_detection == 2 && matches!(options.jsx, Some(4 | 5));
        let (node_id_base, node_array_id_base) = match program_sources.last() {
            Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
            None => lib_sources
                .last()
                .map(|previous| (previous.arena.node_end(), previous.arena.array_end()))
                .unwrap_or((0, 0)),
        };
        let source_file = tsc_syntax::parse_source_file(
            file.name.clone(),
            file.text.clone(),
            tsc_syntax::ParseOptions {
                script_target: options.emit_script_target(),
                language_variant,
                javascript_file,
                force_external_module,
                detect_external_module_from_jsx,
                node_id_base,
                node_array_id_base,
                js_doc_parsing_mode: tsc_syntax::JSDocParsingMode::ParseAll,
            },
            None,
        );
        // tsc getSyntacticDiagnosticsForFile: JS files prepend the
        // TypeScript-only-syntax walker output to their parse diagnostics.
        let mut syntactic = if is_js_file_name(&file.name) {
            js_grammar::get_js_syntactic_diagnostics(&source_file, options.experimental_decorators)
        } else {
            Vec::new()
        };
        syntactic.extend(source_file.parse_diagnostics.iter().cloned());
        // program.getSyntacticDiagnostics(sourceFile) passes the raw
        // JS-grammar + parser stream through getDiagnosticsHelper.
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut syntactic);
        file_diagnostics.push(FileDiagnosticPasses {
            file_name: source_file.file_name.clone(),
            syntactic,
            semantic: Vec::new(),
            suggestion: Vec::new(),
        });
        program_sources.push(source_file);
    }

    let host_current_directory = resolve_host_current_directory(current_directory);
    let program_diagnostics = missing_path_reference_diagnostics(
        &program_sources,
        libs.iter().chain(files.iter()).map(|file| {
            state::CheckerState::normalize_program_path(&file.name, &host_current_directory)
        }),
        options,
        &host_current_directory,
    );

    // Fixture bind pass: per-file binders with contiguous SymbolId
    // bases continuing from the lib prefix (tsc bindSourceFile per
    // file over one heap).
    // Parse the per-file check directive (ts-check/ts-nocheck pragma)
    // ONCE; @ts-ignore/@ts-expect-error ride on each SourceFile's
    // scanner-collected comment_directives.
    observe_phase(CheckPhase::Bind);

    let check_directives: std::collections::HashMap<&str, Option<CheckDirective>> = program_sources
        .iter()
        .map(|source| (source.file_name.as_str(), check_directive(&source.text)))
        .collect();
    let mut bind_diagnostics_by_file = Vec::with_capacity(program_sources.len());
    let mut binders: Vec<tsc_binder::Binder<'_>> = Vec::new();
    for source_file in &program_sources {
        let (symbol_id_seed, symbol_base) = match binders.last() {
            Some(previous) => (previous.next_symbol_id(), previous.symbols.next_id().0),
            None => lib_binders
                .last()
                .map(|previous| (previous.next_symbol_id(), previous.symbols.next_id().0))
                .unwrap_or((1, 0)),
        };
        let mut binder =
            tsc_binder::Binder::with_bases(source_file, options, symbol_id_seed, symbol_base);
        binder.bind_source_file();
        bind_diagnostics_by_file.push(binder.bind_diagnostics.clone());
        binders.push(binder);
    }

    // Checker-state construction (M4 5.0) + the check driver (M4 5.4):
    // the initializeTypeChecker slice runs in from_program (globals
    // merge across non-module files — lib prefix first — plus the
    // cross-file duplicate reporting), then FIXTURE files check IN
    // PROGRAM ORDER (tsc getSemanticDiagnostics per file over one
    // checker; lib files are never asked for). Options diagnostics
    // (bad option combos, core-interfaces §8) would gate ahead of this
    // block — none are modeled yet, so the gate is vacuously open.
    observe_phase(CheckPhase::Check);

    let binder_refs: Vec<&tsc_binder::Binder<'_>> =
        lib_binders.iter().chain(binders.iter()).collect();
    if binder_refs.is_empty() && collect_global_diagnostics {
        global_diagnostics = globals::missing_init_global_type_diagnostics(options);
    }
    if !binder_refs.is_empty() {
        let lib_count = lib_binders.len();
        let mut state = state::CheckerState::from_program(binder_refs, options);
        if let Some(run) = authoritative_run {
            let mut metadata = run.lib_metadata.clone();
            metadata.extend(authoritative_program_metadata.iter().cloned());
            if let Err(failure) =
                state.install_authoritative_module_provider(run.provider, &metadata)
            {
                state.record_authoritative_module_failure(failure);
            }
        }
        // path.posix.resolve absoluteness test (charAt(0) === '/') on
        // the RAW value — a "\\"-led cwd is RELATIVE there, so the
        // process-cwd join and POSIX dot-segment resolution both happen
        // on the raw string BEFORE normalizeFileName flips "\\" into
        // separators. The join base is Node's posixCwd: process.cwd()
        // untouched on POSIX; on Windows backslashes flipped and
        // everything before the first "/" (the drive) dropped. ""
        // (the old "/"-rooted world) is the no-cwd degenerate fallback.
        state.host_current_directory = host_current_directory;
        // The resolver's host view (M4 5.8d): every INPUT path, incl.
        // files the program dropped (.json bodies, .js without
        // allowJs) — the suppression probes need them to keep 2307
        // FP-free.
        state.host_file_paths = files
            .iter()
            .map(|file| state::CheckerState::normalize_program_path(&file.name, ""))
            .collect();
        state.host_package_json_module_types = host_package_json_module_types;
        state.host_package_json_values = files
            .iter()
            .filter_map(|file| {
                let file_name = file.name.rsplit(['/', '\\']).next()?;
                if file_name != "package.json" {
                    return None;
                }
                let value = parse_host_package_json(&file.text)?;
                Some((
                    state::CheckerState::normalize_program_path(&file.name, ""),
                    value,
                ))
            })
            .collect();
        state.host_package_json_names = state
            .host_package_json_values
            .iter()
            .filter_map(|(path, value)| {
                let name = value.get("name")?.as_str()?.trim();
                if name.is_empty() {
                    return None;
                }
                Some((path.clone(), name.to_owned()))
            })
            .collect();
        // initializeTypeChecker's augmentation passes (88769/88874)
        // run here — AFTER the resolver's host view exists (pass 2
        // resolves module names), BEFORE any file checks.
        state.merge_module_augmentations();
        if collect_global_diagnostics {
            state.materialize_init_global_diagnostics();
            global_diagnostics = state.visible_global_diagnostics.clone();
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut global_diagnostics);
        }
        // getDiagnosticsWorker snapshots global diagnostics around each
        // requested source. Only newly-published file-less rows are
        // prepended to that source's checker diagnostics.
        let mut global_checker_diagnostics_by_file = vec![Vec::new(); program_sources.len()];
        for (source_index, index) in (lib_count..state.binder.file_count()).enumerate() {
            let source = state.binder.source(index);
            let javascript_file = is_js_file_name(&source.file_name);
            let directive = check_directives
                .get(source.file_name.as_str())
                .copied()
                .flatten();
            let skip = options.skip_lib_check == Some(true) && source.is_declaration_file
                || !can_include_bind_and_check_diagnostics(javascript_file, directive, options);
            if !skip {
                let global_start = state.visible_global_diagnostics.len();
                state.check_source_file(index);
                global_checker_diagnostics_by_file[source_index].extend(
                    state.visible_global_diagnostics[global_start..]
                        .iter()
                        .cloned(),
                );
            }
        }

        // Public per-file getter assembly. This deliberately does not
        // use a name-sorted map: the outer observation order is the
        // CaseSpec/program fixture ordinal.
        for (source_index, source) in program_sources.iter().enumerate() {
            let javascript_file = is_js_file_name(&source.file_name);
            let directive = check_directives
                .get(source.file_name.as_str())
                .copied()
                .flatten();
            let skip = options.skip_lib_check == Some(true) && source.is_declaration_file
                || !can_include_bind_and_check_diagnostics(javascript_file, directive, options);
            if skip {
                continue;
            }

            let plain_js = is_plain_js_file(javascript_file, directive, options);
            let checker_for_file = state.diagnostics.iter().filter(|diagnostic| {
                diagnostic.file_name.as_deref() == Some(source.file_name.as_str())
            });

            // getSuggestionDiagnostics is a separate checker
            // collection and does not pass through
            // getDiagnosticsHelper. Preserve its collection order and
            // multiplicity exactly.
            file_diagnostics[source_index].suggestion.extend(
                checker_for_file
                    .clone()
                    .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Suggestion)
                    .cloned(),
            );

            // getBindAndCheckDiagnosticsForFileNoCache:
            // bind -> check (new globals first) -> checked-JS JSDoc.
            let mut bind_and_check = Vec::new();
            bind_and_check.extend(bind_diagnostics_by_file[source_index].iter().cloned());
            bind_and_check.extend(
                global_checker_diagnostics_by_file[source_index]
                    .iter()
                    .filter(|diagnostic| diagnostic.category() != DiagnosticCategory::Suggestion)
                    .cloned(),
            );
            bind_and_check.extend(
                checker_for_file
                    .filter(|diagnostic| diagnostic.category() != DiagnosticCategory::Suggestion)
                    .cloned(),
            );
            if javascript_file && !plain_js {
                bind_and_check.extend(source.js_doc_diagnostics.iter().cloned());
            }

            if plain_js {
                bind_and_check
                    .retain(|diagnostic| plain_js_errors::is_plain_js_error(diagnostic.code()));
            } else {
                let mut used_directive_lines = std::collections::HashSet::new();
                bind_and_check = filter_by_comment_directives_and_mark_used(
                    source,
                    bind_and_check.into_iter(),
                    Some(&mut used_directive_lines),
                );
                if let Some(partial_ranges) = state
                    .partially_checked_ranges
                    .get(&(lib_count + source_index))
                {
                    mark_comment_directives_for_partial_ranges(
                        source,
                        partial_ranges,
                        &mut used_directive_lines,
                    );
                }
                bind_and_check.extend(unused_expect_error_diagnostics(
                    source,
                    &used_directive_lines,
                ));
            }

            // filterSemanticDiagnostics applies only to the
            // bind/check half, before getProgramDiagnostics is
            // concatenated.
            filter_semantic_diagnostics(&mut bind_and_check, options);

            let mut program_for_file = program_diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.as_deref() == Some(source.file_name.as_str())
                })
                .cloned()
                .collect::<Vec<_>>();
            if !source.comment_directives.is_empty() {
                // getProgramDiagnostics owns a fresh directive map;
                // use in bind/check does not consume this one.
                program_for_file = filter_by_comment_directives_and_mark_used(
                    source,
                    program_for_file.into_iter(),
                    None,
                );
            }
            bind_and_check.extend(program_for_file);

            // program.getSemanticDiagnostics(sourceFile) uses
            // getDiagnosticsHelper; suggestion intentionally does not.
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut bind_and_check);
            file_diagnostics[source_index].semantic = bind_and_check;
        }
        partial_checks = state.partial_check_records.clone();
        authoritative_failure = state.take_authoritative_module_failure();
    }

    let syntactic_diagnostics = file_diagnostics
        .iter()
        .flat_map(|file| file.syntactic.iter().cloned())
        .collect();
    let semantic_diagnostics = file_diagnostics
        .iter()
        .flat_map(|file| file.semantic.iter().cloned())
        .collect();
    let suggestion_diagnostics = file_diagnostics
        .iter()
        .flat_map(|file| file.suggestion.iter().cloned())
        .collect();

    // The legacy aggregate remains the oracle driver's final
    // ts.sortAndDeduplicateDiagnostics over public getter occurrences.
    let mut diagnostics = file_diagnostics
        .iter()
        .flat_map(|file| {
            file.syntactic
                .iter()
                .chain(&file.semantic)
                .chain(&file.suggestion)
                .cloned()
        })
        .collect();
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);

    debug_assert!(tsc_binder::is_scaffolded());
    debug_assert!(tsc_types::is_scaffolded());

    CheckExecution {
        result: CheckResult {
            diagnostics,
            syntactic_diagnostics,
            semantic_diagnostics,
            global_diagnostics,
            suggestion_diagnostics,
            file_diagnostics,
            partial_checks,
        },
        authoritative_failure,
    }
}

/// A parsed+bound lib-set prefix, shared across programs.
///
/// EXACTNESS (m4-lib-loading-steps.md D3): libs are the program
/// PREFIX, so for a fixed lib list every lib file's
/// NodeId/NodeArrayId/SymbolId bases are identical across programs —
/// the cached arenas ARE the arenas an uncached run would build. The
/// bundle is deliberately leaked (process-lifetime; bounded by the
/// distinct lib-set count, 39 across the conformance corpus), which
/// resolves the sources↔binders self-reference without unsafe.
/// This cache is legacy harness infrastructure only; the H0 production
/// ProgramSession uses the locally owned entry above and never reaches it.
/// Read-only-after-bind is structural: ProgramBinder holds shared
/// references and its symbol_mut refuses file-owned ids.
struct LibBundle {
    options: &'static CompilerOptions,
    sources: &'static [tsc_syntax::SourceFile],
    binders: &'static [tsc_binder::Binder<'static>],
}

/// Opaque exact-match hint returned by [`prepare_harness_lib_bundle`].
///
/// Keeping the bundle private prevents callers from bypassing the validation
/// in [`check_program_with_prepared_harness_libs_at`].
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct PreparedHarnessLibBundle {
    bundle: &'static LibBundle,
}

impl PreparedHarnessLibBundle {
    fn validated(
        self,
        libs: &[&InputFile],
        options: &CompilerOptions,
    ) -> Option<&'static LibBundle> {
        self.bundle
            .exactly_matches(libs, options)
            .then_some(self.bundle)
    }
}

/// Opaque cache key for the exact parser/binder option projection used by a
/// [`PreparedHarnessLibBundle`].
#[doc(hidden)]
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct HarnessLibBundleOptionsKey(CompilerOptions);

impl LibBundle {
    fn exactly_matches(&self, libs: &[&InputFile], options: &CompilerOptions) -> bool {
        self.options == options
            && self.sources.len() == libs.len()
            && self
                .sources
                .iter()
                .zip(libs)
                .all(|(source, lib)| source.file_name == lib.name && source.text == lib.text)
    }
}

/// The per-lib-set bundle cache. Indexed by the ordered (name, text
/// fingerprint) list plus the projection of CompilerOptions onto the parser
/// target and binder's three option observables — the only option fields a
/// cached bundle can expose. Parsing reads `emit_script_target()` for
/// scanner classification and SourceFile.language_version. The binder
/// reads that same computed target (declare.rs language_version,
/// bind.rs ES2015 gate),
/// `always_strict_effective()` (bind.rs use-strict prologue) and
/// `no_fallthrough_cases_in_switch == Some(true)` (bindCaseBlock), and
/// `Binder.options` is read nowhere outside the binder crate. Keying
/// the full struct rebuilt+leaked one identical bundle per matrix
/// option combination (~11.5 GB peak over the conformance corpus);
/// the projection restores the per-lib-set bound. A new `options.`
/// read in the binder MUST extend this projection.
/// The fingerprint key only selects a bucket. Reuse additionally requires
/// exact ordered file-name, full-text, and projected-option equality.
/// `TSRS_LIB_BUNDLE_CACHE=0` bypasses this process-lifetime harness cache and
/// builds a locally owned prefix — the L3 A/B lever proving reuse changes
/// nothing without leaking one fresh bundle per call.
fn lib_bundle_options(options: &CompilerOptions) -> CompilerOptions {
    // Each field holds the observable's canonical preimage, so the
    // projected struct evaluates every binder read identically to the
    // program's own options (ES3/absent targets share the computed
    // ES2025, options.rs:139) while bind-inert fields collapse to one
    // key. A new `options.` read in the binder must extend this
    // projection.
    CompilerOptions {
        target: Some(options.emit_script_target().bits()),
        always_strict: Some(options.always_strict_effective()),
        no_fallthrough_cases_in_switch: Some(options.no_fallthrough_cases_in_switch == Some(true)),
        ..CompilerOptions::default()
    }
}

fn lib_bundle(libs: &[&InputFile], options: &CompilerOptions) -> &'static LibBundle {
    lib_bundle_with_fingerprint(libs, options, lib_text_fingerprint)
}

fn lib_bundle_with_fingerprint(
    libs: &[&InputFile],
    options: &CompilerOptions,
    fingerprint: impl Fn(&str) -> u64,
) -> &'static LibBundle {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    type Key = (Vec<(String, u64)>, CompilerOptions);
    type Bucket = Arc<Mutex<Vec<&'static LibBundle>>>;
    type Buckets = HashMap<Key, Bucket>;
    static CACHE: OnceLock<Mutex<Buckets>> = OnceLock::new();

    // The bundle is built from the projection too: whichever program
    // builds first, the leaked options are the same struct.
    let bundle_options = lib_bundle_options(options);

    let key: Key = (
        libs.iter()
            .map(|lib| (lib.name.clone(), fingerprint(&lib.text)))
            .collect(),
        bundle_options.clone(),
    );
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let bucket = {
        let mut cache = cache.lock().expect("lib bundle cache");
        Arc::clone(
            cache
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(Vec::new()))),
        )
    };
    let mut bucket = bucket.lock().expect("lib bundle cache bucket");
    if let Some(&bundle) = bucket
        .iter()
        .find(|bundle| bundle.exactly_matches(libs, &bundle_options))
    {
        return bundle;
    }

    // Build under the per-index-key lock so equal cold callers cannot leak
    // duplicate process-lifetime bundles. Distinct lib sets still build in
    // parallel without holding the short-lived map lock.
    let bundle = build_lib_bundle(libs, &bundle_options);
    bucket.push(bundle);
    bundle
}

/// Content fingerprint for selecting a bundle-cache bucket. Exact text
/// equality is checked inside the bucket before reuse. A word-folding FNV
/// variant keeps full-text coverage at a fraction of the SipHash cost, which
/// dominated per-case conformance time.
fn lib_text_fingerprint(text: &str) -> u64 {
    let bytes = text.as_bytes();
    let mut hash = 0xcbf29ce484222325u64;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_le_bytes(chunk.try_into().expect("8-byte chunk"));
        hash = (hash ^ word).wrapping_mul(0x100000001b3).rotate_left(23);
    }
    let mut tail = [0u8; 8];
    tail[..chunks.remainder().len()].copy_from_slice(chunks.remainder());
    hash = (hash ^ u64::from_le_bytes(tail)).wrapping_mul(0x100000001b3);
    hash ^ bytes.len() as u64
}

fn build_lib_bundle(libs: &[&InputFile], options: &CompilerOptions) -> &'static LibBundle {
    // Binder borrows its CompilerOptions for the bundle's lifetime.
    let options: &'static CompilerOptions = Box::leak(Box::new(options.clone()));
    let sources: &'static [tsc_syntax::SourceFile] =
        Box::leak(parse_lib_sources(libs, options).into_boxed_slice());
    let binders: &'static [tsc_binder::Binder<'static>] =
        Box::leak(bind_lib_sources(sources, options).into_boxed_slice());
    Box::leak(Box::new(LibBundle {
        options,
        sources,
        binders,
    }))
}

fn parse_lib_sources(
    libs: &[&InputFile],
    options: &CompilerOptions,
) -> Vec<tsc_syntax::SourceFile> {
    let mut sources: Vec<tsc_syntax::SourceFile> = Vec::new();
    for lib in libs {
        let (node_id_base, node_array_id_base) = match sources.last() {
            Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
            None => (0, 0),
        };
        sources.push(tsc_syntax::parse_source_file(
            lib.name.clone(),
            lib.text.clone(),
            tsc_syntax::ParseOptions {
                script_target: options.emit_script_target(),
                language_variant: tsc_syntax::LanguageVariant::Standard,
                javascript_file: false,
                force_external_module: false,
                detect_external_module_from_jsx: false,
                node_id_base,
                node_array_id_base,
                js_doc_parsing_mode: tsc_syntax::JSDocParsingMode::ParseAll,
            },
            None,
        ));
    }
    sources
}

fn bind_lib_sources<'a>(
    sources: &'a [tsc_syntax::SourceFile],
    options: &'a CompilerOptions,
) -> Vec<tsc_binder::Binder<'a>> {
    let mut binders: Vec<tsc_binder::Binder<'a>> = Vec::new();
    for source in sources {
        let (symbol_id_seed, symbol_base) = match binders.last() {
            Some(previous) => (previous.next_symbol_id(), previous.symbols.next_id().0),
            None => (1, 0),
        };
        let mut binder =
            tsc_binder::Binder::with_bases(source, options, symbol_id_seed, symbol_base);
        binder.bind_source_file();
        binders.push(binder);
    }
    binders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_engine_returns_no_diagnostics() {
        let result = check_program(&[], &CompilerOptions::default());
        assert!(result.diagnostics.is_empty());
        assert!(result.syntactic_diagnostics.is_empty());
        assert!(result.semantic_diagnostics.is_empty());
        assert!(result.global_diagnostics.is_empty());
        assert!(result.suggestion_diagnostics.is_empty());
        assert!(result.file_diagnostics.is_empty());
    }

    #[test]
    fn owned_no_emit_entry_keeps_library_borrows_local_and_matches_file_getters() {
        let libs = [InputFile {
            name: "/lib.d.ts".to_owned(),
            text: "interface IArguments {}\ninterface Array<T> {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                .to_owned(),
        }];
        let files = [InputFile {
            name: "/main.ts".to_owned(),
            text: "const value: string = 1;\n".to_owned(),
        }];
        let options = CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        };

        let cached = check_program_with_libs_at(&libs, &files, &options, "/");
        let owned = check_program_with_owned_libs_at(&libs, &files, &options, "/");

        assert_eq!(owned.syntactic_diagnostics, cached.syntactic_diagnostics);
        assert_eq!(owned.semantic_diagnostics, cached.semantic_diagnostics);
        assert!(owned.global_diagnostics.is_empty());
        assert_eq!(
            owned
                .semantic_diagnostics
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            [2322]
        );
    }

    #[test]
    fn owned_no_emit_entry_materializes_global_diagnostics_before_semantics() {
        let result = check_program_with_owned_libs_at(
            &[],
            &[InputFile {
                name: "/main.ts".to_owned(),
                text: "export {};\n".to_owned(),
            }],
            &CompilerOptions {
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
            "/",
        );

        assert_eq!(
            result
                .global_diagnostics
                .iter()
                .map(Diagnostic::message_text)
                .collect::<Vec<_>>(),
            [
                "Cannot find global type 'Array'.",
                "Cannot find global type 'Boolean'.",
                "Cannot find global type 'CallableFunction'.",
                "Cannot find global type 'Function'.",
                "Cannot find global type 'IArguments'.",
                "Cannot find global type 'NewableFunction'.",
                "Cannot find global type 'Number'.",
                "Cannot find global type 'Object'.",
                "Cannot find global type 'RegExp'.",
                "Cannot find global type 'String'.",
            ]
        );
        assert!(result.semantic_diagnostics.is_empty());
    }

    #[test]
    fn owned_no_emit_entry_materializes_globals_without_a_source_binder() {
        let result = check_program_with_owned_libs_at(
            &[],
            &[],
            &CompilerOptions {
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
            "/",
        );

        assert_eq!(
            result
                .global_diagnostics
                .iter()
                .map(Diagnostic::message_text)
                .collect::<Vec<_>>(),
            [
                "Cannot find global type 'Array'.",
                "Cannot find global type 'Boolean'.",
                "Cannot find global type 'CallableFunction'.",
                "Cannot find global type 'Function'.",
                "Cannot find global type 'IArguments'.",
                "Cannot find global type 'NewableFunction'.",
                "Cannot find global type 'Number'.",
                "Cannot find global type 'Object'.",
                "Cannot find global type 'RegExp'.",
                "Cannot find global type 'String'.",
            ]
        );
        assert!(result.semantic_diagnostics.is_empty());

        let relaxed = check_program_with_owned_libs_at(
            &[],
            &[],
            &CompilerOptions {
                no_emit: Some(true),
                strict: Some(false),
                ..CompilerOptions::default()
            },
            "/",
        );
        assert_eq!(relaxed.global_diagnostics.len(), 8);
        assert!(relaxed.global_diagnostics.iter().all(|diagnostic| {
            !diagnostic.message_text().contains("CallableFunction")
                && !diagnostic.message_text().contains("NewableFunction")
        }));
    }

    #[test]
    fn owned_no_emit_entry_keeps_located_global_shape_errors_semantic() {
        let result = check_program_with_owned_libs_at(
            &[],
            &[InputFile {
                name: "/main.ts".to_owned(),
                text: "interface IArguments {}\ninterface Array {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
            "/",
        );

        assert!(result.global_diagnostics.is_empty());
        assert_eq!(
            result
                .semantic_diagnostics
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            [2317]
        );
    }

    #[test]
    fn observed_entry_reports_each_coarse_phase_once() {
        let mut phases = Vec::new();
        let result = check_program_with_libs_at_observed(
            &[],
            &[],
            &CompilerOptions::default(),
            "/",
            |phase| phases.push(phase),
        );
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            phases,
            [CheckPhase::Parse, CheckPhase::Bind, CheckPhase::Check]
        );
    }

    #[test]
    fn public_getter_passes_keep_fixture_ordinal_before_global_sort() {
        let result = check_program(
            &[
                InputFile {
                    name: "z.ts".to_owned(),
                    text: "/// <reference path=\"/z-missing.d.ts\" />\n".to_owned(),
                },
                InputFile {
                    name: "a.ts".to_owned(),
                    text: "/// <reference path=\"/a-missing.d.ts\" />\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );

        assert_eq!(
            result
                .file_diagnostics
                .iter()
                .map(|file| file.file_name.as_str())
                .collect::<Vec<_>>(),
            ["z.ts", "a.ts"]
        );
        assert!(result
            .file_diagnostics
            .iter()
            .all(|file| file.syntactic.is_empty() && file.suggestion.is_empty()));
        assert_eq!(
            result
                .semantic_diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.file_name.as_deref(), diagnostic.code(),))
                .collect::<Vec<_>>(),
            [(Some("z.ts"), 6053), (Some("a.ts"), 6053)]
        );

        let mut assembled = result
            .file_diagnostics
            .iter()
            .flat_map(|file| {
                file.syntactic
                    .iter()
                    .chain(&file.semantic)
                    .chain(&file.suggestion)
                    .cloned()
            })
            .collect::<Vec<_>>();
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut assembled);
        assert_eq!(result.diagnostics, assembled);
    }

    #[test]
    fn missing_leading_path_reference_reports_exact_6053() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "/// <reference path=\"/missing.d.ts\" />\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(
            (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.category(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            ),
            (
                Some("a.ts"),
                6053,
                DiagnosticCategory::Error,
                Some(21),
                Some(13),
                "File '/missing.d.ts' not found.",
            )
        );
        assert!(result.syntactic_diagnostics.is_empty());
    }

    #[test]
    fn relative_single_quoted_path_reference_resolves_against_the_source() {
        let result = check_program(
            &[InputFile {
                name: "src/a.ts".to_owned(),
                text: "///<reference path='../typescript.ts' />\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(
            (
                diagnostic.code(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            ),
            (6053, Some(20), Some(16), "File '/typescript.ts' not found.",)
        );
    }

    #[test]
    fn existing_path_reference_is_loaded_without_a_missing_file_diagnostic() {
        let result = check_program(
            &[
                InputFile {
                    name: "src/a.ts".to_owned(),
                    text: "/// <reference path=\"./dep.d.ts\" />\n".to_owned(),
                },
                InputFile {
                    name: "src/dep.d.ts".to_owned(),
                    text: "declare const dep: number;\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 6053),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn path_reference_projection_stays_on_its_owned_pragma_face() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: concat!(
                    "/// <reference types=\"node\" path=\"/not-a-path-ref.d.ts\" />\n",
                    "/// <reference path=\"/unsupported.html\" />\n",
                    "const text = '/// <reference path=\"/inside-string.d.ts\" />';\n",
                    "/// <reference path=\"/after-token.d.ts\" />\n",
                )
                .to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 6053),
            "{:?}",
            result.diagnostics
        );
    }

    /// Node posixCwd — path.posix.resolve's implicit base: the process
    /// working directory untouched on POSIX; on Windows backslashes
    /// flipped and the pre-"/" drive prefix dropped. The expectation
    /// twin of the derivation in check_program_with_libs_at.
    fn posix_process_cwd() -> String {
        let raw = std::env::current_dir()
            .expect("test process has a working directory")
            .to_string_lossy()
            .into_owned();
        if cfg!(windows) {
            let flipped = raw.replace('\\', "/");
            let root = flipped
                .find('/')
                .expect("an absolute Windows cwd has a separator");
            flipped[root..].to_owned()
        } else {
            raw
        }
    }

    fn cwd_probe_diagnostic_rows(current_directory: &str) -> Vec<(String, u32, u32, u32, String)> {
        let result = check_program_with_libs_at(
            &[],
            &[
                InputFile {
                    name: "b.ts".to_owned(),
                    text: "export const bee = 1;\n".to_owned(),
                },
                InputFile {
                    name: "a.ts".to_owned(),
                    text: "import * as b from \"./b\";\nb.nope;\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
            current_directory,
        );
        result
            .diagnostics
            .iter()
            .map(|diag| {
                (
                    diag.file_name.clone().unwrap_or_default(),
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                    diag.message_text().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn relative_cwd_roots_at_the_process_working_directory() {
        // The oracle host resolves ProgramJson cwd with
        // path.posix.resolve (program-host.mjs decodeProgram), so a
        // RELATIVE cwd roots at Node's posixCwd (drive-stripped on
        // Windows) — not "/". Must ride the PUBLIC entry: the check.rs
        // cwd pins set host_current_directory directly
        // (post-normalization) and cannot catch a regression at this
        // seam.
        let process_cwd = posix_process_cwd();
        assert_eq!(
            cwd_probe_diagnostic_rows("review-relative"),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                format!(
                    "Property 'nope' does not exist on type 'typeof import(\"{process_cwd}/review-relative/b\")'."
                )
            )]
        );
    }

    #[test]
    fn backslash_led_cwd_is_relative_under_posix_resolve() {
        // path.posix.resolve treats "\\" as an ordinary character, so a
        // "\\"-led cwd is RELATIVE — it joins onto posixCwd and the
        // later separator flip collapses "<cwd>/\\x" into "<cwd>/x".
        // Normalizing separators BEFORE the absoluteness test would
        // wrongly re-root it at "/" and drop the process cwd.
        let process_cwd = posix_process_cwd();
        assert_eq!(
            cwd_probe_diagnostic_rows("\\review-relative"),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                format!(
                    "Property 'nope' does not exist on type 'typeof import(\"{process_cwd}/review-relative/b\")'."
                )
            )]
        );
    }

    #[test]
    fn mixed_separator_cwd_resolves_dot_segments_before_backslash_flip() {
        // path.posix.resolve sees "\\" as a literal segment here, so
        // the following POSIX "/.." removes that segment and leaves
        // posixCwd unchanged. Flipping "\\" first would instead let
        // ".." remove the final segment of posixCwd.
        let process_cwd = posix_process_cwd();
        let module_path = state::CheckerState::normalize_program_path("b", &process_cwd);
        assert_eq!(
            cwd_probe_diagnostic_rows("\\/.."),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                format!(
                    "Property 'nope' does not exist on type 'typeof import(\"{module_path}\")'."
                )
            )]
        );
    }

    #[test]
    fn absolute_cwd_backslash_segments_stay_literal_during_dot_resolution() {
        // posix.resolve("/a\\b/..") = "/": "a\\b" is ONE literal
        // segment eaten by "..". Flipping "\\" first would split it
        // and leave "/a". Oracle-probed (driver.mjs): import("/b").
        assert_eq!(
            cwd_probe_diagnostic_rows("/a\\b/.."),
            [(
                "a.ts".to_owned(),
                2339,
                28,
                4,
                "Property 'nope' does not exist on type 'typeof import(\"/b\")'.".to_owned()
            )]
        );
    }

    #[test]
    fn lib_bundle_key_projects_to_bind_observables() {
        use tsc_types::flags::ScriptTarget;
        // A lib name unique to this test: the cache is process-global.
        let lib = InputFile {
            name: "lib.bundle-key-probe.d.ts".to_owned(),
            text: "declare const bundleKeyProbe: number;\n".to_owned(),
        };
        let libs = [&lib];
        let base = CompilerOptions::default();
        let shared = lib_bundle(&libs, &base);
        assert_eq!(shared.sources[0].language_version, ScriptTarget::ES2025);

        // Bind-inert options reuse the bundle: the checker consumes
        // them per program, never through the cached prefix.
        let inert = CompilerOptions {
            strict_null_checks: Some(false),
            jsx: Some(2),
            no_emit: Some(true),
            module_resolution: Some(1),
            ..base.clone()
        };
        assert!(std::ptr::eq(shared, lib_bundle(&libs, &inert)));

        // ES3 and an absent target compute the same ES2025
        // languageVersion (options.rs:139) — one bundle.
        let es3 = CompilerOptions {
            target: Some(ScriptTarget::ES3.bits()),
            ..base.clone()
        };
        assert!(std::ptr::eq(shared, lib_bundle(&libs, &es3)));

        // Each bind-time observable splits the key.
        let es5 = CompilerOptions {
            target: Some(ScriptTarget::ES5.bits()),
            ..base.clone()
        };
        let es5_bundle = lib_bundle(&libs, &es5);
        assert!(!std::ptr::eq(shared, es5_bundle));
        assert_eq!(es5_bundle.sources[0].language_version, ScriptTarget::ES5);
        let loose = CompilerOptions {
            always_strict: Some(false),
            ..base.clone()
        };
        assert!(!std::ptr::eq(shared, lib_bundle(&libs, &loose)));
        let fallthrough = CompilerOptions {
            no_fallthrough_cases_in_switch: Some(true),
            ..base.clone()
        };
        assert!(!std::ptr::eq(shared, lib_bundle(&libs, &fallthrough)));
    }

    #[test]
    fn lib_bundle_forced_fingerprint_collision_requires_exact_text() {
        fn collide_all_text(_: &str) -> u64 {
            0
        }

        let first = InputFile {
            name: "lib.bundle-collision-probe.d.ts".to_owned(),
            text: "declare const collisionProbe: string;\n".to_owned(),
        };
        let second = InputFile {
            name: first.name.clone(),
            text: "declare const collisionProbe: number;\n".to_owned(),
        };
        let options = CompilerOptions::default();

        let first_bundle = lib_bundle_with_fingerprint(&[&first], &options, collide_all_text);
        let second_bundle = lib_bundle_with_fingerprint(&[&second], &options, collide_all_text);
        let first_again = lib_bundle_with_fingerprint(&[&first], &options, collide_all_text);

        assert!(!std::ptr::eq(first_bundle, second_bundle));
        assert!(std::ptr::eq(first_bundle, first_again));
        assert_eq!(first_bundle.sources[0].text, first.text);
        assert_eq!(second_bundle.sources[0].text, second.text);
    }

    #[test]
    fn prepared_harness_bundle_validates_exact_text_and_projected_options() {
        fn assert_handle_traits<T: Copy + Send + Sync + 'static>() {}

        assert_handle_traits::<PreparedHarnessLibBundle>();
        let original = InputFile {
            name: "lib.prepared-validation-probe.d.ts".to_owned(),
            text: "declare const preparedProbe: string;\n".to_owned(),
        };
        let changed = InputFile {
            name: original.name.clone(),
            text: "declare const preparedProbe: number;\n".to_owned(),
        };
        let base = CompilerOptions::default();
        let prepared = prepare_harness_lib_bundle(std::slice::from_ref(&original), &base).unwrap();
        let base_projection = lib_bundle_options(&base);

        assert!(prepared.validated(&[&original], &base_projection).is_some());
        assert!(prepared.validated(&[&changed], &base_projection).is_none());

        let bind_inert = CompilerOptions {
            strict_null_checks: Some(true),
            no_emit: Some(true),
            ..base.clone()
        };
        assert!(
            harness_lib_bundle_options_key(&base) == harness_lib_bundle_options_key(&bind_inert)
        );
        assert!(prepared
            .validated(&[&original], &lib_bundle_options(&bind_inert))
            .is_some());

        let bind_observable = CompilerOptions {
            always_strict: Some(false),
            ..base.clone()
        };
        assert!(
            harness_lib_bundle_options_key(&base)
                != harness_lib_bundle_options_key(&bind_observable)
        );
        assert!(prepared
            .validated(&[&original], &lib_bundle_options(&bind_observable))
            .is_none());

        let second = InputFile {
            name: "lib.prepared-validation-second.d.ts".to_owned(),
            text: "declare const preparedSecond: boolean;\n".to_owned(),
        };
        let ordered = [original.clone(), second.clone()];
        let ordered_prepared = prepare_harness_lib_bundle(&ordered, &base).unwrap();
        assert!(ordered_prepared
            .validated(&[&ordered[0], &ordered[1]], &base_projection)
            .is_some());
        assert!(ordered_prepared
            .validated(&[&ordered[1], &ordered[0]], &base_projection)
            .is_none());
        let renamed = InputFile {
            name: "lib.prepared-validation-renamed.d.ts".to_owned(),
            text: second.text.clone(),
        };
        assert!(ordered_prepared
            .validated(&[&ordered[0], &renamed], &base_projection)
            .is_none());
    }

    #[test]
    fn stale_prepared_harness_bundle_falls_back_to_ordinary_exact_bundle() {
        fn rows(result: &CheckResult) -> Vec<(u32, String)> {
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code(), diagnostic.message_text().to_owned()))
                .collect()
        }

        let original = InputFile {
            name: "lib.prepared-fallback-probe.d.ts".to_owned(),
            text: "declare const preparedFallbackProbe: string;\n".to_owned(),
        };
        let changed = InputFile {
            name: original.name.clone(),
            text: "declare const preparedFallbackProbe: number;\n".to_owned(),
        };
        let files = [InputFile {
            name: "/prepared-fallback.ts".to_owned(),
            text: "const value: string = preparedFallbackProbe;\n".to_owned(),
        }];
        let options = CompilerOptions::default();
        let prepared =
            prepare_harness_lib_bundle(std::slice::from_ref(&original), &options).unwrap();

        let ordinary =
            check_program_with_libs_at(std::slice::from_ref(&changed), &files, &options, "/");
        let hinted = check_program_with_prepared_harness_libs_at(
            std::slice::from_ref(&changed),
            &files,
            &options,
            "/",
            prepared,
        );

        assert_eq!(rows(&hinted), rows(&ordinary));
        assert!(rows(&hinted).iter().any(|(code, _)| *code == 2322));

        let mut observe_phase = |_| {};
        let cache_off = check_program_with_libs_at_observed_cache_mode_prepared(
            std::slice::from_ref(&changed),
            &files,
            &options,
            "/",
            false,
            Some(prepared),
            &mut observe_phase,
        );
        assert_eq!(rows(&cache_off), rows(&ordinary));

        let shadowing_file = [InputFile {
            name: original.name.clone(),
            text: "const localOnly = 1;\n".to_owned(),
        }];
        let ordinary_shadowed = check_program_with_libs_at(
            std::slice::from_ref(&original),
            &shadowing_file,
            &options,
            "/",
        );
        let hinted_shadowed = check_program_with_prepared_harness_libs_at(
            std::slice::from_ref(&original),
            &shadowing_file,
            &options,
            "/",
            prepared,
        );
        assert_eq!(rows(&hinted_shadowed), rows(&ordinary_shadowed));
    }

    #[test]
    fn parallel_cold_lib_bundle_callers_share_one_exact_entry() {
        let lib = InputFile {
            name: "lib.bundle-parallel-cold-probe.d.ts".to_owned(),
            text: (0..512)
                .map(|index| format!("interface ColdProbe{index} {{ value: number }}\n"))
                .collect(),
        };
        let options = CompilerOptions::default();
        let start = std::sync::Barrier::new(3);

        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                start.wait();
                lib_bundle(&[&lib], &options)
            });
            let second = scope.spawn(|| {
                start.wait();
                lib_bundle(&[&lib], &options)
            });
            start.wait();
            (
                first.join().expect("first cold cache caller"),
                second.join().expect("second cold cache caller"),
            )
        });

        assert!(std::ptr::eq(first, second));
    }

    #[test]
    fn cache_off_owned_prefix_matches_cached_harness_result() {
        let libs = [InputFile {
            name: "lib.cache-mode-probe.d.ts".to_owned(),
            text: "interface IArguments {}\ninterface Array<T> {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                .to_owned(),
        }];
        let files = [InputFile {
            name: "cache-mode-probe.ts".to_owned(),
            text: "const value: string = 1;\n".to_owned(),
        }];
        let options = CompilerOptions::default();
        let mut cached_phases = Vec::new();
        let mut owned_phases = Vec::new();

        let cached = check_program_with_libs_at_observed_cache_mode(
            &libs,
            &files,
            &options,
            "/",
            true,
            &mut |phase| cached_phases.push(phase),
        );
        let owned = check_program_with_libs_at_observed_cache_mode(
            &libs,
            &files,
            &options,
            "/",
            false,
            &mut |phase| owned_phases.push(phase),
        );

        assert_eq!(owned, cached);
        assert_eq!(owned_phases, cached_phases);
        assert_eq!(
            owned_phases,
            [CheckPhase::Parse, CheckPhase::Bind, CheckPhase::Check]
        );
    }

    #[test]
    fn authoritative_owned_and_harness_cached_modes_are_exactly_equivalent() {
        struct Provider {
            fail: bool,
        }

        impl AuthoritativeModuleProvider for Provider {
            fn resolve_module(
                &self,
                request: AuthoritativeModuleRequest<'_>,
            ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure>
            {
                assert_eq!(request.source_token, AuthoritativeSourceToken(1));
                assert_eq!(request.containing_file, "/main.ts");
                assert_eq!(request.specifier, "pkg");
                if self.fail {
                    Err(AuthoritativeModuleLookupFailure::Missing)
                } else {
                    Ok(AuthoritativeModuleResolution::NotFound(
                        AuthoritativeNotFoundModule::default(),
                    ))
                }
            }
        }

        let libs = [InputFile {
            name: "/lib.authoritative-cache-mode-probe.d.ts".to_owned(),
            text: "interface IArguments {}\ninterface Array<T> {}\ninterface Object {}\ninterface Function {}\ninterface CallableFunction extends Function {}\ninterface NewableFunction extends Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\n"
                .to_owned(),
        }];
        let files = [InputFile {
            name: "/main.ts".to_owned(),
            text: "import 'pkg';\nconst value: string = 1;\n".to_owned(),
        }];
        let lib_metadata = [AuthoritativeSourceMetadata {
            token: AuthoritativeSourceToken(0),
            file_name: libs[0].name.clone(),
            may_be_emitted: false,
            implied_node_format: None,
            implied_node_format_for_emit: None,
        }];
        let file_metadata = [AuthoritativeSourceMetadata {
            token: AuthoritativeSourceToken(1),
            file_name: files[0].name.clone(),
            may_be_emitted: true,
            implied_node_format: None,
            implied_node_format_for_emit: None,
        }];
        let options = CompilerOptions {
            no_emit: Some(true),
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        };
        let run = |cache_enabled, provider: &Provider| {
            check_program_with_authoritative_modules_at_cache_mode(
                &libs,
                &files,
                &lib_metadata,
                &file_metadata,
                &options,
                "/",
                provider,
                cache_enabled,
            )
        };

        let owned = run(false, &Provider { fail: false }).expect("owned authoritative result");
        let cached = run(true, &Provider { fail: false }).expect("cached authoritative result");
        assert_eq!(owned, cached);
        assert_eq!(
            cached
                .semantic_diagnostics
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            [2882, 2322]
        );

        let owned_failure =
            run(false, &Provider { fail: true }).expect_err("owned authoritative failure");
        let cached_failure =
            run(true, &Provider { fail: true }).expect_err("cached authoritative failure");
        assert_eq!(owned_failure, cached_failure);
    }

    #[test]
    fn authoritative_not_found_facts_reach_the_node10_diagnostic_chain() {
        struct Provider;

        impl AuthoritativeModuleProvider for Provider {
            fn resolve_module(
                &self,
                request: AuthoritativeModuleRequest<'_>,
            ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure>
            {
                assert_eq!(request.source_token, AuthoritativeSourceToken(1));
                assert_eq!(request.containing_file, "/index.ts");
                assert_eq!(request.specifier, "pkg");
                assert_eq!(request.mode, AuthoritativeResolutionMode::Unspecified);
                Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule {
                        alternate_result: Some(
                            "/node_modules/pkg/definitely-not-index.d.ts".to_owned(),
                        ),
                    },
                ))
            }
        }

        let source = "import { pkg } from \"pkg\";\n";
        let files = [InputFile {
            name: "/index.ts".to_owned(),
            text: source.to_owned(),
        }];
        let metadata = [AuthoritativeSourceMetadata {
            token: AuthoritativeSourceToken(1),
            file_name: files[0].name.clone(),
            may_be_emitted: true,
            implied_node_format: None,
            implied_node_format_for_emit: None,
        }];
        let result = check_program_with_authoritative_modules_at_cache_mode(
            &[],
            &files,
            &[],
            &metadata,
            &CompilerOptions {
                no_emit: Some(true),
                module_resolution: Some(2),
                ..CompilerOptions::default()
            },
            "/",
            &Provider,
            false,
        )
        .expect("authoritative alternate-result miss");

        let diagnostic = result
            .semantic_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2307)
            .expect("module-not-found diagnostic");
        assert_eq!(
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            ),
            (
                Some("/index.ts"),
                Some(source.find("\"pkg\"").expect("module specifier") as u32),
                Some("\"pkg\"".len() as u32),
                "Cannot find module 'pkg' or its corresponding type declarations.",
            )
        );
        assert_eq!(diagnostic.message.next.len(), 1);
        assert_eq!(
            (
                diagnostic.message.next[0].code,
                diagnostic.message.next[0].category,
                diagnostic.message.next[0].text.as_str(),
            ),
            (
                6280,
                DiagnosticCategory::Message,
                "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
            )
        );
    }

    #[test]
    fn program_parser_receives_the_effective_script_target() {
        let files = [InputFile {
            name: "a.ts".to_owned(),
            text: "foo.\u{08a1};\n".to_owned(),
        }];
        let es5 = check_program(
            &files,
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES5.bits()),
                ..CompilerOptions::default()
            },
        );
        let es2015 = check_program(
            &files,
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES2015.bits()),
                ..CompilerOptions::default()
            },
        );

        assert_eq!(
            es5.syntactic_diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
                .collect::<Vec<_>>(),
            vec![(1127, Some(4), Some(1))]
        );
        assert!(es2015.syntactic_diagnostics.is_empty());
    }

    #[test]
    fn js_files_report_typescript_only_syntax() {
        // Pins from tsc program.getSyntacticDiagnostics on an allowJs program.
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "function f(x: number): string { return \"\"; }\ninterface I { a: string }\nenum E { A }\nvar x!;\nimport eq = require(\"m\");\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .syntactic_diagnostics
            .iter()
            .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
            .collect();
        assert_eq!(
            pins,
            [
                (8010, 14, 6),
                (8010, 23, 6),
                (8006, 55, 1),
                (8006, 76, 1),
                (8002, 92, 25),
            ]
        );
    }

    #[test]
    fn js_files_report_type_only_imports_and_export_equals() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "import type { A } from \"m\";\nimport { type B } from \"m\";\nexport type { C };\nexport = 5;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .syntactic_diagnostics
            .iter()
            .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
            .collect();
        assert_eq!(
            pins,
            [(8006, 0, 27), (8006, 37, 6), (8006, 56, 18), (8003, 75, 11)]
        );
    }

    fn codes_of(source: &str) -> Vec<u32> {
        codes_of_with_options(source, &CompilerOptions::default())
    }

    fn codes_of_with_options(source: &str, options: &CompilerOptions) -> Vec<u32> {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: source.to_owned(),
            }],
            options,
        );
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
            .map(|d| d.code())
            .collect()
    }

    #[test]
    fn bom_before_arrow_at_line_end_does_not_create_a_line_terminator_error() {
        let without_bom = codes_of("const f = () =>\n  1;\n");
        let with_bom = codes_of("\u{feff}const f = () =>\n  1;\n");
        assert_eq!(with_bom, without_bom);
        assert!(!with_bom.contains(&1200));

        let invalid_without_bom = codes_of("const f = ()\n  => 1;\n");
        let invalid_with_bom = codes_of("\u{feff}const f = ()\n  => 1;\n");
        assert_eq!(invalid_with_bom, invalid_without_bom);
        assert!(invalid_with_bom.contains(&1200));
    }

    #[test]
    fn host_package_json_accepts_one_leading_bom() {
        assert_eq!(
            parse_host_package_json("\u{feff}{\"type\":\"module\"}"),
            parse_host_package_json("{\"type\":\"module\"}")
        );
    }

    fn strict_options() -> CompilerOptions {
        CompilerOptions {
            strict: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        }
    }

    #[test]
    fn typeof_import_follows_value_alias_reexports() {
        let result = check_program(
            &[
                InputFile {
                    name: "a.ts".to_owned(),
                    text: "export const x = 1;\n".to_owned(),
                },
                InputFile {
                    name: "b.ts".to_owned(),
                    text: "export { x } from \"./a\";\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "type T = typeof import(\"./b\").x;\nlet y: T = \"bad\";\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2322]
        );
    }

    #[test]
    fn implicit_external_modules_exclude_umd_global_aliases() {
        let run = |file_name: &str,
                   file_text: &str,
                   options: CompilerOptions,
                   extra_files: &[InputFile]| {
            let mut files = vec![InputFile {
                name: "umd.d.ts".to_owned(),
                text: "export as namespace U;\nexport const s: unique symbol;\n".to_owned(),
            }];
            files.extend_from_slice(extra_files);
            files.push(InputFile {
                name: file_name.to_owned(),
                text: file_text.to_owned(),
            });
            let result = check_program(&files, &options);
            result
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code() == 2741)
                .expect("the computed-property assignment should report 2741")
                .message_text()
                .to_owned()
        };
        let assignment = "declare let a: {};\nlet b: {\n  // @ts-ignore\n  [U.s]: number\n} = a;\n";
        let expected =
            "Property '[U.s]' is missing in type '{}' but required in type '{ [s]: number; }'.";

        // Auto mode: .mts/.cts are modules even without import/export.
        assert_eq!(
            run("a.mts", assignment, CompilerOptions::default(), &[]),
            expected
        );
        // Force mode: every non-declaration source file is a module.
        assert_eq!(
            run(
                "a.ts",
                assignment,
                CompilerOptions {
                    module_detection: Some(3),
                    ..CompilerOptions::default()
                },
                &[]
            ),
            expected
        );
        // Auto + React JSX: a real JSX tag is the indicator.
        assert_eq!(
            run(
                "a.tsx",
                &format!("{assignment}const element = <div />;\n"),
                CompilerOptions {
                    jsx: Some(4),
                    ..CompilerOptions::default()
                },
                &[]
            ),
            expected
        );
        // Auto + Node-flavored package lookup: a nearest `type: module`
        // package scope supplies an ESNext implied format.
        assert_eq!(
            run(
                "/src/a.ts",
                assignment,
                CompilerOptions {
                    module: Some(7),
                    module_resolution: Some(3),
                    module_detection: Some(2),
                    ..CompilerOptions::default()
                },
                &[InputFile {
                    name: "/package.json".to_owned(),
                    text: r#"{"type":"module"}"#.to_owned(),
                }]
            ),
            expected
        );
        // Legacy mode intentionally retains syntax-only detection.
        assert_eq!(
            run(
                "a.mts",
                assignment,
                CompilerOptions {
                    module_detection: Some(1),
                    ..CompilerOptions::default()
                },
                &[]
            ),
            "Property '[U.s]' is missing in type '{}' but required in type '{ [U.s]: number; }'."
        );
    }

    #[test]
    fn import_type_missing_member_uses_absolute_module_name() {
        let result = check_program(
            &[
                InputFile {
                    name: "m.ts".to_owned(),
                    text: "export interface Present {}\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "type T = import(\"./m\").Missing;\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2694)
            .expect("missing import-type member should report 2694");
        assert_eq!(
            diagnostic.message_text(),
            "Namespace '\"/m\"' has no exported member 'Missing'."
        );
    }

    #[test]
    fn bare_import_defer_does_not_run_import_meta_module_checks() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "const x = import.defer;\n".to_owned(),
            }],
            &CompilerOptions {
                module: Some(1),
                ..CompilerOptions::default()
            },
        );
        assert!(!result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1343));
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [1005]
        );
    }

    #[test]
    fn node16_plain_ts_uses_package_scope_for_import_meta() {
        let options = CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        };
        let commonjs = check_program(
            &[InputFile {
                name: "src/main.ts".to_owned(),
                text: "const x = import.meta;\n".to_owned(),
            }],
            &options,
        );
        assert!(commonjs
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1470));

        let esm = check_program(
            &[
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{\"type\":\"module\"}\n".to_owned(),
                },
                InputFile {
                    name: "src/main.ts".to_owned(),
                    text: "const x = import.meta;\n".to_owned(),
                },
            ],
            &options,
        );
        assert!(!esm
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1470));
    }

    #[test]
    fn node16_windows_paths_use_package_scope_for_import_meta() {
        let result = check_program(
            &[
                InputFile {
                    name: r"C:\pkg\package.json".to_owned(),
                    text: "{\"type\":\"module\"}\n".to_owned(),
                },
                InputFile {
                    name: r"C:\pkg\main.ts".to_owned(),
                    text: "const x = import.meta;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 1470),
            "Windows path separators must not hide package.json: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node16_package_commonjs_format_applies_to_default_import_and_export_equals() {
        let result = check_program(
            &[
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{\"type\":\"commonjs\"}\n".to_owned(),
                },
                InputFile {
                    name: "dep.ts".to_owned(),
                    text: "const value = { a: 1 };\nexport = value;\n".to_owned(),
                },
                InputFile {
                    name: "main.mts".to_owned(),
                    text: "import value from \"./dep.js\";\nvalue.a;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic.code(), 1192 | 1203)),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn unrelated_package_inputs_do_not_hide_a_bare_module_miss() {
        let result = check_program(
            &[
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{\"name\":\"unrelated\"}\n".to_owned(),
                },
                InputFile {
                    name: "node_modules/other/index.d.ts".to_owned(),
                    text: "export {};\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "import { value } from \"definitely-missing\";\nvalue;\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2307),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn base_url_miss_without_a_paths_match_reports_2307() {
        let result = check_program(
            &[InputFile {
                name: "src/main.ts".to_owned(),
                text: "import { value } from \"definitely-missing\";\nvalue;\n".to_owned(),
            }],
            &CompilerOptions {
                base_url: Some("src".to_owned()),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2307),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_definite_relative_module_miss_is_public() {
        let result = check_program(
            &[
                InputFile {
                    name: "foo.js".to_owned(),
                    text: "export const value = 1;\n".to_owned(),
                },
                InputFile {
                    name: "main.mjs".to_owned(),
                    text: "import { value } from \"./foo\";\nvalue;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                module: Some(100),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert_eq!(codes, [2835], "{:#?}", result.diagnostics);
    }

    #[test]
    fn checked_js_global_this_collision_is_public() {
        let result = check_program(
            &[InputFile {
                name: "globalThisCollision.js".to_owned(),
                text: "var globalThis;".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                no_emit: Some(true),
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                )
            })
            .collect();
        assert_eq!(pins, [(2397, 4, 10)], "{:#?}", result.diagnostics);
    }

    #[test]
    fn checked_js_publishes_namespace_export_declaration_bind_diagnostic() {
        let files = [
            InputFile {
                name: "cls.js".to_owned(),
                text: "export class Foo {}\n".to_owned(),
            },
            InputFile {
                name: "globalNs.js".to_owned(),
                text: "export * from \"./cls\";\nexport as namespace GLO;\n".to_owned(),
            },
        ];
        let checked = check_program(
            &files,
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                module: Some(1),
                ..CompilerOptions::default()
            },
        );
        let pins = checked
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1315)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.as_deref(),
                    diagnostic.start,
                    diagnostic.length,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            pins,
            [(
                Some("globalNs.js"),
                Some(files[1].text.find("export as").expect("namespace export") as u32),
                Some("export as namespace GLO;".len() as u32),
            )]
        );

        let plain = check_program(
            &files,
            &CompilerOptions {
                allow_js: true,
                module: Some(1),
                ..CompilerOptions::default()
            },
        );
        assert!(
            plain
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 1315),
            "plain JS must retain the plainJSErrors publication surface: {:#?}",
            plain.diagnostics
        );
    }

    #[test]
    fn checked_js_host_dependent_module_resolution_stays_suppressed() {
        let result = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.js".to_owned(),
                    text: "export const value = 1;\n".to_owned(),
                },
                InputFile {
                    name: "main.js".to_owned(),
                    text: "import { value } from \"pkg\";\nvalue;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                module: Some(100),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2307),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn external_emit_helpers_validate_an_in_program_tslib() {
        let result = check_program(
            &[
                InputFile {
                    name: "types.d.ts".to_owned(),
                    text: "declare module \"tslib\" { export {}; }\n".to_owned(),
                },
                InputFile {
                    name: "a.ts".to_owned(),
                    text: "export {};\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "export * as ns from \"./a\";\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(1),
                import_helpers: Some(true),
                ..CompilerOptions::default()
            },
        );
        let helper = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2343)
            .expect("missing __importStar should report");
        assert!(helper.message_text().contains("__importStar"));
    }

    #[test]
    fn external_emit_helpers_report_only_definite_tslib_misses() {
        let files = [
            InputFile {
                name: "a.ts".to_owned(),
                text: "export {};\n".to_owned(),
            },
            InputFile {
                name: "main.ts".to_owned(),
                text: "export * as ns from \"./a\";\n".to_owned(),
            },
        ];
        let options = CompilerOptions {
            module: Some(1),
            import_helpers: Some(true),
            ..CompilerOptions::default()
        };
        let missing = check_program(&files, &options);
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2354),
            "{:#?}",
            missing.diagnostics
        );

        let mut host_dependent = files.to_vec();
        host_dependent.push(InputFile {
            name: "node_modules/tslib/index.d.ts".to_owned(),
            text: "export {};\n".to_owned(),
        });
        let suppressed = check_program(&host_dependent, &options);
        assert!(
            suppressed
                .diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.code(), 2343 | 2354 | 2807)),
            "{:#?}",
            suppressed.diagnostics
        );
    }

    #[test]
    fn external_emit_helpers_check_spread_array_arity() {
        let result = check_program(
            &[
                InputFile {
                    name: "types.d.ts".to_owned(),
                    text: "declare module \"tslib\" {\n  export function __spreadArray(to: any[], from: any[]): any[];\n}\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "export {};\nconst values = [1, ...[2], 3];\n".to_owned(),
                },
            ],
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES5.bits()),
                import_helpers: Some(true),
                ..CompilerOptions::default()
            },
        );
        let helper = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2807)
            .expect("two-parameter __spreadArray should report");
        assert!(helper.message_text().contains("3 parameters"));
    }

    #[test]
    fn external_emit_helpers_check_private_get_and_set_arity() {
        let tslib = InputFile {
            name: "types.d.ts".to_owned(),
            text: concat!(
                "declare module \"tslib\" {\n",
                "  export function __classPrivateFieldGet<T extends object, V>(receiver: T, state: any): V;\n",
                "  export function __classPrivateFieldSet<T extends object, V>(receiver: T, state: any, value: V): V;\n",
                "}\n",
            )
            .to_owned(),
        };
        let cases = [
            (
                "instance.ts",
                concat!(
                    "\nexport class C {\n",
                    "    #a = 1;\n",
                    "    #b() { this.#c = 42; }\n",
                    "    set #c(v: number) { this.#a += v; }\n",
                    "}\n",
                ),
                [
                    (41, 7, "__classPrivateFieldSet", "5 parameters"),
                    (81, 7, "__classPrivateFieldGet", "4 parameters"),
                ],
            ),
            (
                "static.ts",
                concat!(
                    "\nexport class S {\n",
                    "    static #a = 1;\n",
                    "    static #b() { this.#a = 42; }\n",
                    "    static get #c() { return S.#b(); }\n",
                    "}\n",
                ),
                [
                    (55, 7, "__classPrivateFieldSet", "5 parameters"),
                    (100, 4, "__classPrivateFieldGet", "4 parameters"),
                ],
            ),
        ];
        let options = CompilerOptions {
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            import_helpers: Some(true),
            isolated_modules: Some(true),
            ..CompilerOptions::default()
        };

        for (file_name, text, expected) in cases {
            let result = check_program(
                &[
                    tslib.clone(),
                    InputFile {
                        name: file_name.to_owned(),
                        text: text.to_owned(),
                    },
                ],
                &options,
            );
            let observed = result
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2807)
                .map(|diagnostic| {
                    (
                        diagnostic.start.unwrap_or_default(),
                        diagnostic.length.unwrap_or_default(),
                        diagnostic.message_text(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(observed.len(), expected.len(), "{file_name}: {observed:#?}");
            for (observed, expected) in observed.iter().zip(expected) {
                assert_eq!((observed.0, observed.1), (expected.0, expected.1));
                assert!(
                    observed.2.contains(expected.2),
                    "{file_name}: {}",
                    observed.2
                );
                assert!(
                    observed.2.contains(expected.3),
                    "{file_name}: {}",
                    observed.2
                );
            }
        }

        let native = check_program(
            &[
                tslib,
                InputFile {
                    name: "native.ts".to_owned(),
                    text: "export class C { #x = 1; read() { return this.#x; } }\n".to_owned(),
                },
            ],
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
                import_helpers: Some(true),
                isolated_modules: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(native
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 2807));
    }

    #[test]
    fn external_emit_helpers_cover_decorator_named_evaluation_helpers() {
        let result = check_program(
            &[
                InputFile {
                    name: "types.d.ts".to_owned(),
                    text: "declare module \"tslib\" { export {}; }\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "export {};\ndeclare let dec: any;\ndeclare let key: any;\n({ [key]: @dec class {} });\n".to_owned(),
                },
            ],
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                module: Some(1),
                import_helpers: Some(true),
                ..CompilerOptions::default()
            },
        );
        let messages: Vec<&str> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2343)
            .map(|diagnostic| diagnostic.message_text())
            .collect();
        for helper in [
            "__esDecorate",
            "__runInitializers",
            "__setFunctionName",
            "__propKey",
        ] {
            assert!(
                messages.iter().any(|message| message.contains(helper)),
                "missing {helper}: {messages:#?}"
            );
        }
    }

    #[test]
    fn parameter_initializer_ordering_reports_self_and_later_but_not_deferred() {
        assert_eq!(
            codes_of("function f(a = a, b = c, c = 1, d = () => e, e = 1) {}\n")
                .into_iter()
                .filter(|code| matches!(code, 2372 | 2373))
                .collect::<Vec<_>>(),
            [2372, 2373]
        );
    }

    #[test]
    fn parameter_initializer_scope_change_honors_explicit_legacy_class_fields() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "class C {}\n((b = class extends C { static x = 1 }, d = x) => { var C; var x; })();\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
                use_define_for_class_fields: Some(false),
                ..CompilerOptions::default()
            },
        );
        let rows: Vec<(u32, u32, u32)> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2373)
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or_default(),
                    diagnostic.length.unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(rows, [(2373, 31, 1), (2373, 55, 1)]);
    }

    #[test]
    fn missing_import_meta_global_is_public_semantic_diagnostic() {
        assert_eq!(
            codes_of_with_options(
                "const x = import.meta;\n",
                &CompilerOptions {
                    module: Some(99),
                    ..CompilerOptions::default()
                },
            ),
            [2318]
        );
    }

    #[test]
    fn missing_generator_fallback_global_is_public_semantic_diagnostic() {
        assert_eq!(codes_of("function* f() { yield 1; }\n"), [2318]);
    }

    #[test]
    fn ts_nocheck_does_not_publish_missing_generator_globals() {
        let codes = codes_of("// @ts-nocheck\nfunction* f() { yield 1; }\n");
        assert!(codes.is_empty(), "{codes:?}");
    }

    #[test]
    fn check_js_false_does_not_publish_missing_generator_globals() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "function* f() { yield 1; }\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(false),
                ..CompilerOptions::default()
            },
        );
        assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
    }

    #[test]
    fn node16_esm_import_of_commonjs_has_synthetic_default_even_when_option_is_false() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.cts".to_owned(),
                    text: "declare const value: { x: number };\nexport = value;\n".to_owned(),
                },
                InputFile {
                    name: "main.mts".to_owned(),
                    text: "import value from \"./dep.cjs\";\nvalue.x;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                allow_synthetic_default_imports: Some(false),
                es_module_interop: Some(false),
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert!(
            !codes.contains(&1259) && !codes.contains(&1192) && !codes.contains(&1203),
            "native ESM-to-CJS default interop should be accepted: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node16_package_commonjs_target_has_synthetic_default() {
        let result = check_program(
            &[
                InputFile {
                    name: "esm/package.json".to_owned(),
                    text: "{\"type\":\"module\"}\n".to_owned(),
                },
                InputFile {
                    name: "cjs/package.json".to_owned(),
                    text: "{\"type\":\"commonjs\"}\n".to_owned(),
                },
                InputFile {
                    name: "cjs/dep.ts".to_owned(),
                    text: "export const ok = 1;\n".to_owned(),
                },
                InputFile {
                    name: "esm/main.ts".to_owned(),
                    text: "import value from \"../cjs/dep.js\";\nvalue.ok;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                allow_synthetic_default_imports: Some(false),
                es_module_interop: Some(false),
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 1192),
            "package-scoped CommonJS target should have a synthetic default: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node16_mode_mismatch_details_preserve_package_type_evidence() {
        let run = |package_json: &str| {
            check_program(
                &[
                    InputFile {
                        name: "/package.json".to_owned(),
                        text: package_json.to_owned(),
                    },
                    InputFile {
                        name: "/module.mts".to_owned(),
                        text: "export const value = 1;\n".to_owned(),
                    },
                    InputFile {
                        name: "/common.cts".to_owned(),
                        text: "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                    },
                    InputFile {
                        name: "/common.js".to_owned(),
                        text: "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                    },
                    InputFile {
                        name: "/common.ts".to_owned(),
                        text: "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                    },
                    InputFile {
                        name: "/common.tsx".to_owned(),
                        text: "import { value } from \"./module.mjs\";\nvalue;\n".to_owned(),
                    },
                ],
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    module: Some(100),
                    module_resolution: Some(3),
                    target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                    ..CompilerOptions::default()
                },
            )
        };
        let detail_codes = |result: &CheckResult| {
            result
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 1479)
                .map(|diagnostic| {
                    (
                        diagnostic
                            .file_name
                            .as_deref()
                            .expect("mode mismatch is located")
                            .to_owned(),
                        diagnostic.message.next.first().map(|detail| detail.code),
                    )
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(
            detail_codes(&run("{}\n")),
            [
                ("/common.cts".to_owned(), None),
                ("/common.js".to_owned(), Some(1481)),
                ("/common.ts".to_owned(), Some(1481)),
                ("/common.tsx".to_owned(), Some(1482)),
            ]
        );
        assert_eq!(
            detail_codes(&run("{\"type\":\"commonjs\"}\n")),
            [
                ("/common.cts".to_owned(), None),
                ("/common.js".to_owned(), Some(1480)),
                ("/common.ts".to_owned(), Some(1480)),
                ("/common.tsx".to_owned(), Some(1483)),
            ]
        );
    }

    #[test]
    fn node16_mode_mismatch_selects_construct_and_honors_overrides() {
        let result = check_program(
            &[
                InputFile {
                    name: "/module.mts".to_owned(),
                    text: "export type T = number;\n".to_owned(),
                },
                InputFile {
                    name: "/common.cts".to_owned(),
                    text: "import value = require(\"./module.mjs\");\n\
                           import type {} from \"./module.mjs\";\n\
                           import type {} from \"./module.mjs\" with { \"resolution-mode\": \"import\" };\n\
                           type Plain = typeof import(\"./module.mjs\");\n\
                           type Overridden = typeof import(\"./module.mjs\", { with: { \"resolution-mode\": \"import\" } });\n\
                           const dynamic = import(\"./module.mjs\");\n\
                           void value;\nvoid dynamic;\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                ..CompilerOptions::default()
            },
        );
        let mismatch_codes = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .filter(|code| matches!(code, 1471 | 1479 | 1541 | 1542))
            .collect::<Vec<_>>();
        assert_eq!(
            mismatch_codes,
            [1471, 1541, 1542],
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node16_mode_mismatch_resolves_package_conditions_and_patterns_without_publishing_symbols() {
        let result = check_program(
            &[
                InputFile {
                    name: "/node_modules/pkg/package.json".to_owned(),
                    text: "{\"exports\":{\"./exact\":{\"require\":\"./esm.mjs\",\"import\":\"./cjs.cjs\"},\"./pattern/*\":\"./*.mjs\"}}\n".to_owned(),
                },
                InputFile {
                    name: "/node_modules/pkg/esm.mts".to_owned(),
                    text: "export const exact = 1;\n".to_owned(),
                },
                InputFile {
                    name: "/node_modules/pkg/value.mts".to_owned(),
                    text: "export const pattern = 1;\n".to_owned(),
                },
                InputFile {
                    name: "/consumer.cts".to_owned(),
                    text: "import { exact } from \"pkg/exact\";\n\
                           import { pattern } from \"pkg/pattern/value\";\n\
                           exact;\npattern;\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                ..CompilerOptions::default()
            },
        );
        let mismatch_codes = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .filter(|code| *code == 1479)
            .collect::<Vec<_>>();
        assert_eq!(mismatch_codes, [1479, 1479], "{:#?}", result.diagnostics);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic.code(), 2305 | 2551)),
            "diagnostic-only package resolution must not publish target members: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn bundler_does_not_infer_plain_target_format_from_package_scope() {
        let result = check_program(
            &[
                InputFile {
                    name: "/package.json".to_owned(),
                    text: "{\"type\":\"module\"}\n".to_owned(),
                },
                InputFile {
                    name: "/plain.ts".to_owned(),
                    text: "declare const plain: number;\nexport = plain;\n".to_owned(),
                },
                InputFile {
                    name: "/decisive.mts".to_owned(),
                    text: "declare const decisive: number;\nexport = decisive;\n".to_owned(),
                },
                InputFile {
                    name: "/consumer.ts".to_owned(),
                    text: "import plain from \"./plain\";\nimport decisive from \"./decisive.mts\";\nplain;\ndecisive;\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(99),
                module_resolution: Some(100),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                allow_synthetic_default_imports: Some(true),
                ..CompilerOptions::default()
            },
        );
        let rows: Vec<(String, u32, u32, u32)> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1192)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().expect("located diagnostic"),
                    diagnostic.code(),
                    diagnostic.start.expect("located diagnostic"),
                    diagnostic.length.expect("located diagnostic"),
                )
            })
            .collect();
        assert_eq!(rows, [("/consumer.ts".to_owned(), 1192, 36, 8)]);
    }

    #[test]
    fn emit_format_distinguishes_explicit_commonjs_from_missing_package_type() {
        let result = check_program(
            &[
                InputFile {
                    name: "/node_modules/cjs/package.json".to_owned(),
                    text: "{\"type\":\"commonjs\"}\n".to_owned(),
                },
                InputFile {
                    name: "/node_modules/cjs/index.ts".to_owned(),
                    text: "export const value = 1;\n".to_owned(),
                },
                InputFile {
                    name: "/node_modules/other/package.json".to_owned(),
                    text: "{}\n".to_owned(),
                },
                InputFile {
                    name: "/node_modules/other/index.ts".to_owned(),
                    text: "export const value = 1;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(99),
                module_resolution: Some(100),
                target: Some(tsc_types::ScriptTarget::ES2022.bits()),
                verbatim_module_syntax: Some(true),
                ..CompilerOptions::default()
            },
        );
        let rows: Vec<(String, u32, u32, u32)> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 1287)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().expect("located diagnostic"),
                    diagnostic.code(),
                    diagnostic.start.expect("located diagnostic"),
                    diagnostic.length.expect("located diagnostic"),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [("/node_modules/cjs/index.ts".to_owned(), 1287, 0, 6)]
        );
    }

    #[test]
    fn node16_json_declaration_rejects_named_esm_imports() {
        let result = check_program(
            &[
                InputFile {
                    name: "data.d.json.ts".to_owned(),
                    text: "export const x: number;\n".to_owned(),
                },
                InputFile {
                    name: "main.mts".to_owned(),
                    text: "import data, { x } from \"./data.d.json.ts\";\ndata.x;\nx;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                allow_importing_ts_extensions: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1544));
    }

    #[test]
    fn node18_json_default_import_requires_type_attribute() {
        let files = |main: &str| {
            vec![
                InputFile {
                    name: "data.d.json.ts".to_owned(),
                    text: "export const x: number;\n".to_owned(),
                },
                InputFile {
                    name: "main.mts".to_owned(),
                    text: main.to_owned(),
                },
            ]
        };
        let options = CompilerOptions {
            module: Some(101),
            module_resolution: Some(3),
            allow_importing_ts_extensions: Some(true),
            ..CompilerOptions::default()
        };
        let missing = check_program(
            &files("import data from \"./data.d.json.ts\";\ndata.x;\n"),
            &options,
        );
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 1543),
            "Node18 JSON import without an attribute should report 1543: {:#?}",
            missing.diagnostics
        );

        let attributed = check_program(
            &files("import data from \"./data.d.json.ts\" with { type: \"json\" };\ndata.x;\n"),
            &options,
        );
        assert!(
            !attributed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 1543),
            "a type: json attribute should satisfy the Node18 requirement: {:#?}",
            attributed.diagnostics
        );
    }

    #[test]
    fn import_attributes_on_cjs_emit_report_2856_with_priority() {
        // tsc checkImportAttributes: the CommonJS-require row (2856)
        // rides the specifier's emit syntax and takes priority over
        // the type-only (2857) and resolution-mode (1454) rows. The
        // oracle-correction epoch made the row observable corpus-wide
        // (nodeModulesJson loosey.cts and the ImportAttributesMode
        // DeclarationEmit fixtures).
        let files = |main: &str| {
            vec![
                InputFile {
                    name: "data.d.json.ts".to_owned(),
                    text: "declare const _default: {};\nexport default _default;\n".to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: main.to_owned(),
                },
            ]
        };
        let options = CompilerOptions {
            module: Some(101),
            module_resolution: Some(3),
            allow_importing_ts_extensions: Some(true),
            ..CompilerOptions::default()
        };
        let plain = check_program(
            &files("import data from \"./data.d.json.ts\" with { type: \"json\" };\ndata;\n"),
            &options,
        );
        let codes: Vec<u32> = plain
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert!(codes.contains(&2856), "{:#?}", plain.diagnostics);

        let type_only = check_program(
            &files(
                "import type data from \"./data.d.json.ts\" with { type: \"json\" };\nexport type T = typeof data;\n",
            ),
            &options,
        );
        let codes: Vec<u32> = type_only
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert!(
            codes.contains(&2856) && !codes.contains(&2857),
            "the CommonJS-require row outranks the type-only row: {:#?}",
            type_only.diagnostics
        );
    }

    #[test]
    fn node18_actual_json_module_is_resolved_and_typed() {
        let result = check_program(
            &[
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{\"type\":\"module\"}\n".to_owned(),
                },
                InputFile {
                    name: "data.json".to_owned(),
                    text: "{\"count\": 1, \"label\": \"ok\"}\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "import data from \"./data.json\";\n\
                           let count: number;\n\
                           count = data.count;\n\
                           let wrong: string;\n\
                           wrong = data.count;\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(101),
                module_resolution: Some(3),
                resolve_json_module: Some(true),
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert!(codes.contains(&1543), "{:#?}", result.diagnostics);
        assert!(codes.contains(&2322), "{:#?}", result.diagnostics);
        assert!(!codes.contains(&2307), "{:#?}", result.diagnostics);
    }

    #[test]
    fn node20_commonjs_default_import_uses_module_exports_export() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.mts".to_owned(),
                    text: "const value = { a: 1 };\nexport { value as \"module.exports\" };\n"
                        .to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: "import value from \"./dep.mjs\";\nvalue.a;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                es_module_interop: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result.diagnostics.is_empty(),
            "Node20 module.exports interop should resolve the default: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node20_module_exports_default_import_requires_explicit_interop_when_disabled() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.mts".to_owned(),
                    text: "const value = { a: 1 };\nexport { value as \"module.exports\" };\n"
                        .to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: "import value from \"./dep.mjs\";\nvalue.a;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                es_module_interop: Some(false),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [1259],
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node20_module_exports_precedes_syntactic_default() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.mts".to_owned(),
                    text: "export default function actual(x: string): string { return x; }\n\
                           const compat = (x: number) => x;\n\
                           export { compat as \"module.exports\" };\n"
                        .to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: "import fn from \"./dep.mjs\";\nfn(1);\nfn(\"x\");\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        let errors: Vec<&tsc_diagnostics::Diagnostic> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2345)
            .collect();
        assert_eq!(errors.len(), 1, "{:#?}", result.diagnostics);
        assert_eq!(
            errors[0].message_text(),
            "Argument of type 'string' is not assignable to parameter of type 'number'."
        );
    }

    #[test]
    fn checked_cjs_require_of_node20_esm_namespace_is_not_constructable() {
        for es_module_interop in [true, false] {
            let result = check_program(
                &[
                    InputFile {
                        name: "/exporter.mts".to_owned(),
                        text: "export default class Foo {}\n\
                               const oops = \"oops\";\n\
                               export { oops as \"module.exports\" };\n"
                            .to_owned(),
                    },
                    InputFile {
                        name: "/importer.cjs".to_owned(),
                        text: "const Foo = require(\"./exporter.mjs\");\nnew Foo();\n".to_owned(),
                    },
                ],
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    module: Some(102),
                    module_resolution: Some(3),
                    es_module_interop: Some(es_module_interop),
                    ..CompilerOptions::default()
                },
            );
            let errors: Vec<&tsc_diagnostics::Diagnostic> = result
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code() == 2351)
                .collect();
            assert_eq!(
                errors.len(),
                1,
                "diagnostics={:#?}\npartial={:#?}",
                result.diagnostics,
                result.partial_checks
            );
            assert_eq!(
                errors[0].message_text(),
                "This expression is not constructable."
            );
        }
    }

    #[test]
    fn node20_namespace_import_uses_distinct_module_exports_export() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.mts".to_owned(),
                    text: "export default function actual(x: string): string { return x; }\n\
                           const compat = (x: number) => x;\n\
                           export { compat as \"module.exports\" };\n"
                        .to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: "import * as fn from \"./dep.mjs\";\nfn(1);\nfn(\"x\");\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        let errors: Vec<&tsc_diagnostics::Diagnostic> = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2345)
            .collect();
        assert_eq!(errors.len(), 1, "{:#?}", result.diagnostics);
        assert_eq!(
            errors[0].message_text(),
            "Argument of type 'string' is not assignable to parameter of type 'number'."
        );
    }

    #[test]
    fn node20_namespace_import_uses_module_exports_even_when_it_aliases_default() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.mts".to_owned(),
                    text: "const compat = (x: number) => x;\n\
                           export default compat;\n\
                           export { compat as \"module.exports\" };\n"
                        .to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: "import * as fn from \"./dep.mjs\";\nfn(1);\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
    }

    fn js_pair_diagnostics(js: &str, ts: &str) -> Vec<(u32, Option<String>)> {
        check_program(
            &[
                InputFile {
                    name: "a.js".to_owned(),
                    text: js.to_owned(),
                },
                InputFile {
                    name: "b.ts".to_owned(),
                    text: ts.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..strict_options()
            },
        )
        .diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.code(), diagnostic.file_name))
        .collect()
    }

    #[test]
    fn unrelated_destructuring_sibling_guard_keeps_property_miss() {
        assert_eq!(
            codes_of_with_options(
                "function f({a,b}:{a:boolean,b:number}){if(a){b.missing;}}",
                &strict_options(),
            ),
            [2339]
        );
    }

    #[test]
    fn concrete_destructuring_equality_guard_keeps_property_miss() {
        assert_eq!(
            codes_of_with_options(
                "function f({a,b}:{a:boolean,b:number}){if(a===true){b.missing;}}",
                &strict_options(),
            ),
            [2339]
        );
    }

    #[test]
    fn discriminated_destructuring_sibling_still_narrows() {
        assert_eq!(
            codes_of_with_options(
                "type A={kind:'A',payload:{a:number}}|{kind:'B',payload:{b:number}};\
                 function f({kind,payload}:A){if(kind==='A'){payload.a;}}",
                &strict_options(),
            ),
            Vec::<u32>::new()
        );
    }

    fn full_lib_bundle(target_libs: &[&str]) -> Vec<InputFile> {
        let base = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-6.0.3/lib/"
        );
        target_libs
            .iter()
            .map(|name| InputFile {
                name: (*name).to_owned(),
                text: std::fs::read_to_string(format!("{base}{name}")).expect("vendored lib"),
            })
            .collect()
    }

    #[test]
    fn in_operator_missing_key_join_keeps_later_const_key_narrowing() {
        // controlFlowInOperator: the missing-key branch and the later
        // `a in c` branch are independent; the latter narrows to A so
        // `c[a]` remains valid.
        let libs = full_lib_bundle(&[
            "lib.es6.d.ts",
            "lib.es5.d.ts",
            "lib.es2015.d.ts",
            "lib.dom.d.ts",
            "lib.dom.iterable.d.ts",
            "lib.webworker.importscripts.d.ts",
            "lib.scripthost.d.ts",
            "lib.es2015.core.d.ts",
            "lib.es2015.collection.d.ts",
            "lib.es2015.generator.d.ts",
            "lib.es2015.iterable.d.ts",
            "lib.es2015.promise.d.ts",
            "lib.es2015.proxy.d.ts",
            "lib.es2015.reflect.d.ts",
            "lib.es2015.symbol.d.ts",
            "lib.es2015.symbol.wellknown.d.ts",
            "lib.es2018.asynciterable.d.ts",
            "lib.decorators.d.ts",
            "lib.decorators.legacy.d.ts",
        ]);
        let options = CompilerOptions {
            strict: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        let result = check_program_with_libs(
            &libs,
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "const a = 'a';\nconst b = 'b';\nconst d = 'd';\ntype A = { [a]: number; };\ntype B = { [b]: string; };\ndeclare const c: A | B;\nif ('d' in c) {\n    c;\n}\nif (a in c) {\n    c;\n    c[a];\n}\n".to_owned(),
            }],
            &options,
        );
        let rows: Vec<(String, u32)> = result
            .diagnostics
            .iter()
            .filter(|d| d.file_name.as_deref() == Some("a.ts"))
            .map(|d| (d.file_name.clone().unwrap_or_default(), d.code()))
            .collect();
        assert_eq!(rows, Vec::<(String, u32)>::new());
    }

    #[test]
    fn const_key_in_narrowing_indexes_late_bound_members() {
        // `a in c` narrows to A and `c[a]` resolves (oracle-clean).
        let text = "const a = 'a';\nconst b = 'b';\nconst d = 'd';\ntype A = { [a]: number; };\ntype B = { [b]: string; };\ndeclare const c: A | B;\nif (a in c) {\n    c;\n    c[a];\n}\n";
        assert_eq!(
            lib_codes_of_with_options(text, &strict_options()),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn for_in_over_optional_chain_stays_clean() {
        // tsc #51941 (canary FP controlFlowOptionalChain f50): the
        // body's obj.main read must not 18048; the optional-chain
        // condition narrows the body read.
        let text = "type Test5 = {\n  main?: {\n    childs: Record<string, Test5>;\n  };\n};\nfunction f50(obj: Test5) {\n   for (const key in obj.main?.childs) {\n      if (obj.main.childs[key] === obj) {\n        return obj;\n      }\n   }\n   return null;\n}\n";
        assert_eq!(
            lib_codes_of_with_options(text, &strict_options()),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn overload_failure_promise_intersection_awaits_to_never() {
        // The combined overload-failure signature returns the
        // INTERSECTION of candidate returns (tsc 76907); awaiting it
        // unwraps through the intersected structural `then` to never,
        // so the loop-carried assignment stays silent — only the 2769
        // reports (oracle-exact; the un-unwrapped promise was the
        // 6.6f 2322 FP face).
        let libs = full_lib_bundle(&[
            "lib.es6.d.ts",
            "lib.es5.d.ts",
            "lib.es2015.d.ts",
            "lib.es2015.core.d.ts",
            "lib.es2015.collection.d.ts",
            "lib.es2015.generator.d.ts",
            "lib.es2015.iterable.d.ts",
            "lib.es2015.promise.d.ts",
            "lib.es2015.proxy.d.ts",
            "lib.es2015.reflect.d.ts",
            "lib.es2015.symbol.d.ts",
            "lib.es2015.symbol.wellknown.d.ts",
            "lib.es2018.asynciterable.d.ts",
            "lib.decorators.d.ts",
            "lib.decorators.legacy.d.ts",
        ]);
        let options = CompilerOptions {
            strict: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        let result = check_program_with_libs(
            &libs,
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const cond: boolean;\ndeclare function foo(x: string): Promise<number>;\ndeclare function foo(x: number): Promise<string>;\nasync function g1() {\n    let x: string | number | boolean;\n    x = \"\";\n    while (cond) {\n        x = await foo(x);\n        x;\n    }\n    x;\n}\n".to_owned(),
            }],
            &options,
        );
        let rows: Vec<(u32, u32)> = result
            .diagnostics
            .iter()
            .filter(|d| d.file_name.as_deref() == Some("a.ts"))
            .map(|d| (d.code(), d.start.unwrap_or(0)))
            .collect();
        assert_eq!(rows, [(2769, 242)]);
    }

    #[test]
    fn async_iteration_fixture_reports_no_spurious_2322() {
        let libs = full_lib_bundle(&[
            "lib.es6.d.ts",
            "lib.es5.d.ts",
            "lib.es2015.d.ts",
            "lib.es2015.core.d.ts",
            "lib.es2015.collection.d.ts",
            "lib.es2015.generator.d.ts",
            "lib.es2015.iterable.d.ts",
            "lib.es2015.promise.d.ts",
            "lib.es2015.proxy.d.ts",
            "lib.es2015.reflect.d.ts",
            "lib.es2015.symbol.d.ts",
            "lib.es2015.symbol.wellknown.d.ts",
            "lib.es2018.asynciterable.d.ts",
            "lib.decorators.d.ts",
            "lib.decorators.legacy.d.ts",
        ]);
        let options = CompilerOptions {
            strict: Some(true),
            target: Some(tsc_types::ScriptTarget::ES2015.bits()),
            ..CompilerOptions::default()
        };
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ts-tests/tests/cases/conformance/controlFlow/controlFlowIterationErrorsAsync.ts"
        ))
        .expect("fixture")
        .lines()
        .filter(|line| !line.trim_start().starts_with("// @"))
        .collect::<Vec<_>>()
        .join("\n");
        let result = check_program_with_libs(
            &libs,
            &[InputFile {
                name: "a.ts".to_owned(),
                text,
            }],
            &options,
        );
        let rows: Vec<u32> = result
            .diagnostics
            .iter()
            .filter(|d| d.file_name.as_deref() == Some("a.ts"))
            .map(|d| d.code())
            .collect();
        assert_eq!(
            rows.iter().filter(|&&c| c == 2322).count(),
            0,
            "rows: {rows:?}"
        );
    }

    #[test]
    fn computed_key_destructuring_assignment_contains() {
        // The evaluation-order family (tsc PR #41094) defers to M6 —
        // the const-bb rows partial-mark instead of misreporting
        // (controlFlowAssignmentPatternOrder).
        let text = "let a: 0 | 1 = 0;\nlet b: 0 | 1 | 8 | 9;\n[{ [(a = 1)]: b } = [9, a] as const] = [[9, 8] as const];\nconst bb: 0 | 8 = b;\n";
        assert_eq!(
            lib_codes_of_with_options(text, &strict_options()),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn destructuring_assignment_reads_apparent_type_members() {
        // getTypeOfPropertyOfType has no receiver-flags guard (55803;
        // 6.6 review A1) — string.length resolves via the reduced
        // apparent type and the assigned type narrows; tsc is clean.
        assert_eq!(
            lib_codes_of_with_options(
                "let n: number | string = 0;\n({ length: n } = \"abc\");\nconst m: number = n;\n",
                &strict_options()
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn body_predicate_narrows_reference_inside_compound_return() {
        // The inferred predicate narrows `u` before the array literal
        // is checked against the annotated return type.
        assert_eq!(
            lib_codes_of_with_options(
                "function isNum(x: string | number) { return typeof x === \"number\"; }\nfunction g(u: string | number): number[] { if (isNum(u)) { return [u]; } return [0]; }\n",
                &strict_options()
            ),
            Vec::<u32>::new()
        );
    }

    fn lib_codes_of_with_options(source: &str, options: &CompilerOptions) -> Vec<u32> {
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.ts".to_owned(),
                text: source.to_owned(),
            }],
            options,
        );
        result.diagnostics.iter().map(|d| d.code()).collect()
    }

    // The three redeclaration pins below run WITH lib.es5 — the real
    // autoArrayType (6.2) is Array<auto>, which needs the global Array
    // to mint and render (`any[]`). The lib-less env degrades to a
    // display partial, matching tsc --noLib's own no-2403 output.
    #[test]
    fn empty_array_redeclaration_still_reports_incompatible_type() {
        assert_eq!(
            lib_codes_of_with_options("var x = [];\nvar x = 1;\n", &strict_options()),
            [2403]
        );
    }

    #[test]
    fn shadowed_array_function_does_not_trigger_evolving_array_containment() {
        assert_eq!(
            lib_codes_of_with_options(
                "function f(){function Array():number{return 1};var x=[];var x=Array();return x;}",
                &strict_options(),
            ),
            [2403]
        );
    }

    #[test]
    fn array_returning_call_redeclaration_reports_2403() {
        // Pre-6.2 this scenario was CONTAINED (the evolving-array
        // stand-in rendered the wrong first-type face); the real
        // autoArrayType retires the escape and matches the oracle.
        assert_eq!(
            lib_codes_of_with_options(
                "declare function makeArray():number[];var x=[];var x=makeArray();",
                &strict_options(),
            ),
            [2403]
        );
    }

    #[test]
    fn ts_const_function_expression_reads_assignment_members_normally() {
        assert_eq!(
            codes_of(
                "const f = function () { return true; };\n\
                 f.extra = 1;\n\
                 const value: number = f.extra;\n\
                 f.missing;\n"
            ),
            [2339]
        );
    }

    #[test]
    fn expando_member_uses_annotated_parent_property_type() {
        assert_eq!(
            codes_of(
                "interface F { (): boolean; value: 123; }\n\
                 const f: F = () => true;\n\
                 f.value = 123;\n"
            ),
            Vec::<u32>::new()
        );
    }

    fn checked_js_codes(source: &str) -> Vec<u32> {
        check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect()
    }

    fn checked_js_codes_with_function_prototype(source: &str) -> Vec<u32> {
        // getPropertyOfType 59348-59389 augments a callable with the
        // global Function face. The upstream fixture uses the default
        // lib, whose lib.es5.d.ts:299 declares `prototype: any`.
        check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect()
    }

    #[test]
    fn checked_js_bare_prototype_access_type_annotates_the_assignment_symbol() {
        // getWidenedTypeForAssignmentDeclaration 56247-56263 keeps a
        // bare access declaration as the expression, so its @type
        // participates in the earlier constructor assignment.
        assert_eq!(
            checked_js_codes_with_function_prototype(
                "function C() { this.x = false; }\n\
                 /** @type {number} */\n\
                 C.prototype.x;\n\
                 new C().x;\n"
            ),
            [2322]
        );
    }

    #[test]
    fn checked_js_bare_prototype_access_without_type_does_not_constrain_the_assignment() {
        assert_eq!(
            checked_js_codes_with_function_prototype(
                "function C() { this.x = false; }\n\
                 C.prototype.x;\n\
                 new C().x;\n"
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn checked_js_chained_prototype_replacement_uses_the_rightmost_object_literal() {
        // getAssignedJSPrototype 77594-77606 reads
        // getInitializerOfBinaryExpression, so both A and B acquire
        // the object-literal class face.
        assert_eq!(
            checked_js_codes(
                "var A = function A() {};\n\
                 var B = function B() {};\n\
                 A.prototype = B.prototype = {\n\
                   /** @param {number} n */\n\
                   m(n) { return n + 1; }\n\
                 };\n\
                 new A().m('bad');\n\
                 new B().m('bad');\n"
            ),
            [2345, 2345]
        );
    }

    #[test]
    fn checked_js_non_object_chained_prototype_replacement_does_not_invent_members() {
        let codes = checked_js_codes(
            // isJSConstructor 77509-77522 requires an instance member:
            // establish constructability before the primitive prototype
            // assignment, then verify that the assignment neither removes
            // that face nor invents `missing`.
            "var A = function A() { this.a = 1; };\n\
             var B = function B() { this.b = 2; };\n\
             A.prototype = B.prototype = 0;\n\
             new A().missing;\n",
        );
        assert!(codes.contains(&2339), "{codes:?}");
        assert!(!codes.contains(&7009), "{codes:?}");
    }

    #[test]
    fn checked_js_exported_arrow_expando_keeps_its_own_property_annotation() {
        // getTypeOfFuncClassEnumModule 56808-56827 publishes the
        // merged initializer/expando type on both link faces.
        assert_eq!(
            checked_js_codes(
                "/** @type {{ (): boolean; nuo: 789 }} */\n\
                 export const conflicting = () => true;\n\
                 /** @type {1000} */\n\
                 conflicting.nuo = 789;\n"
            ),
            [2322]
        );
    }

    #[test]
    fn checked_js_exported_arrow_matching_expando_annotation_is_clean() {
        assert_eq!(
            checked_js_codes(
                "/** @type {{ (): boolean; nuo: 789 }} */\n\
                 export const matching = () => true;\n\
                 /** @type {789} */\n\
                 matching.nuo = 789;\n"
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn function_return_annotation_is_not_an_expando_parent_annotation() {
        assert_eq!(
            lib_codes_of_with_options(
                "function f(): number { return 1; }\nf.toFixed = \"own\";\n",
                &CompilerOptions::default(),
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn plain_js_object_reference_warning_requires_strict_equality() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "if ({} === {}) {}\nif ({} == {}) {}\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2839]
        );
    }

    #[test]
    fn js_declared_container_property_miss_in_ts_file_reports() {
        assert_eq!(
            js_pair_diagnostics("class C {}", "const c = new C(); c.missing;"),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_assignment_declared_class_member_stays_available() {
        assert!(js_pair_diagnostics("class C {}\nC.extra = 1;", "C.extra;").is_empty());
    }

    #[test]
    fn shadowed_js_class_assignment_does_not_open_outer_class() {
        assert_eq!(
            js_pair_diagnostics(
                "class C {}\nfunction f(){class C {}\nC.extra = 1;}",
                "C.extra;",
            ),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_assignment_declared_function_member_stays_available() {
        assert!(js_pair_diagnostics("function F() {}\nF.extra = 1;", "F.extra;").is_empty());
    }

    #[test]
    fn js_assignment_declared_prototype_member_stays_available() {
        assert!(
            js_pair_diagnostics("class C {}\nC.prototype.extra = 1;", "new C().extra;").is_empty()
        );
    }

    #[test]
    fn js_static_assignment_does_not_open_instance_side() {
        assert_eq!(
            js_pair_diagnostics("class C {}\nC.extra = 1;", "new C().extra;"),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_prototype_assignment_does_not_open_static_side() {
        assert_eq!(
            js_pair_diagnostics("class C {}\nC.prototype.extra = 1;", "C.extra;"),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_static_this_assignment_does_not_open_instance_side() {
        assert_eq!(
            js_pair_diagnostics("class C { static { this.extra = 1; } }", "new C().extra;",),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_instance_this_assignment_does_not_open_static_side() {
        assert_eq!(
            js_pair_diagnostics("class C { constructor() { this.extra = 1; } }", "C.extra;",),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_static_this_assignment_stays_available_on_static_side() {
        assert!(
            js_pair_diagnostics("class C { static { this.extra = 1; } }", "C.extra;",).is_empty()
        );
    }

    #[test]
    fn js_instance_this_assignment_stays_available_on_instance_side() {
        assert!(js_pair_diagnostics(
            "class C { constructor() { this.extra = 1; } }",
            "new C().extra;",
        )
        .is_empty());
    }

    #[test]
    fn nested_non_arrow_function_this_does_not_open_class_instance() {
        let diagnostics = js_pair_diagnostics(
            "class C { method() { function nested() { this.extra = 1; } nested(); } }",
            "new C().extra;",
        );
        assert!(
            diagnostics.contains(&(2339, Some("b.ts".to_owned()))),
            "a nested function owns its `this`: {diagnostics:?}"
        );
    }

    #[test]
    fn nested_js_assignment_does_not_open_direct_static_member() {
        assert_eq!(
            js_pair_diagnostics(
                "class C {}\nC.bucket = {};\nC.bucket.extra = 1;",
                "C.extra;",
            ),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn nested_js_assignment_still_opens_its_actual_receiver() {
        assert!(js_pair_diagnostics(
            "class C {}\nC.bucket = {};\nC.bucket.extra = 1;",
            "C.bucket.extra;",
        )
        .is_empty());
    }

    #[test]
    fn unresolved_module_augmentation_keeps_unrelated_property_miss() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "augmentation.ts".to_owned(),
                    text: "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\n(\"x\").missing;\n"
                        .to_owned(),
                },
                // An unrelated package scope does not make "pkg"
                // resolvable and therefore must not hide 2664.
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{}".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2664, 2339]
        );
    }

    #[test]
    fn unresolved_module_augmentation_does_not_open_same_named_local_type() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.ts".to_owned(),
                    text: "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\ninterface X {}\ndeclare const local: X;\nlocal.missing;\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_bare_augmentation_does_not_claim_same_spelled_workspace_file() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/other/index.d.ts".to_owned(),
                    text: "export {};\n".to_owned(),
                },
                InputFile {
                    name: "pkg.ts".to_owned(),
                    text: "interface X {}\ndeclare const local: X;\nlocal.missing;\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.ts".to_owned(),
                    text:
                        "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\n"
                            .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2664, 2339]
        );
    }

    #[test]
    fn unresolved_module_augmentation_contains_index_signature_property() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace Pkg;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as Pkg from \"pkg\";\ndeclare module \"pkg\" { interface X { [key: string]: unknown } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const value: Pkg.X;\nvalue.anything;\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn unresolved_module_augmentation_contains_computed_property() {
        let result = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace Pkg;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as Pkg from \"pkg\";\ndeclare const member: \"extra\";\ndeclare module \"pkg\" { interface X { [member](): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const value: Pkg.X;\nvalue.extra();\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );
        let diagnostics = result
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
            .collect::<Vec<_>>();
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert!(
            result.partial_checks.is_empty(),
            "{:#?}",
            result.partial_checks
        );
    }

    #[test]
    fn unresolved_module_augmentation_matches_export_equals_namespace_target() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace Pkg;\nexport = Package;\ndeclare namespace Package { class X {} }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as Pkg from \"pkg\";\ndeclare module \"pkg\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const value: Pkg.X;\nvalue.added();\nfunction use<T extends Pkg.X>(item: T) { item.added(); }\ndeclare const mixed: Pkg.X | { added(): void };\nmixed.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn unresolved_module_augmentation_does_not_open_sibling_package_subpath() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/a.d.ts".to_owned(),
                    text: "export as namespace PkgA;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "node_modules/pkg/b.d.ts".to_owned(),
                    text: "export as namespace PkgB;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as PkgA from \"pkg/a\";\ndeclare module \"pkg/a\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const aValue: PkgA.X;\naValue.added();\ndeclare const bValue: PkgB.X;\nbValue.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_module_augmentation_stays_with_nearest_package_instance() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "app1/node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace PkgOne;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "app2/node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace PkgTwo;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "app1/augmentation.d.ts".to_owned(),
                    text: "import * as PkgOne from \"pkg\";\ndeclare module \"pkg\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "app2/use.ts".to_owned(),
                    text: "declare const one: PkgOne.X;\none.added();\ndeclare const two: PkgTwo.X;\ntwo.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_node_core_augmentation_matches_only_its_at_types_node_subpath() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/@types/node/fs.d.ts".to_owned(),
                    text: "export as namespace NodeFs;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "node_modules/@types/node/http.d.ts".to_owned(),
                    text: "export as namespace NodeHttp;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as NodeFs from \"node:fs\";\ndeclare module \"node:fs\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const fsValue: NodeFs.X;\nfsValue.added();\ndeclare const httpValue: NodeHttp.X;\nhttpValue.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
        .collect::<Vec<_>>();
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2591, 2339, 2339]
        );
    }

    #[test]
    fn value_side_member_publication_survives_reentrant_base_resolution() {
        let diagnostics = codes_of(
            "class B {}\nclass A extends A.make() {\n  static make(): typeof B { return B; }\n}\nA.make();\n",
        );
        assert!(
            !diagnostics.contains(&2339),
            "staged exports must stay visible during base resolution: {diagnostics:?}"
        );
    }

    #[test]
    fn truthy_this_guard_keeps_type_query_assignment_error() {
        assert_eq!(
            codes_of_with_options(
                "class C { m() { if (this) { const x: typeof this = 1; } } }",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn tuple_intersection_array_literal_keeps_element_error() {
        assert_eq!(
            codes_of_with_options(
                "const x: [string] & { p: number } = [1];",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn tuple_intersection_unrelated_member_reports_the_intersection_head() {
        // Oracle: one 2322 head with args '[number]' vs
        // '[number] & { p: string; }' (+ the missing-'p' chain in the
        // elided tail). The intersection member is an anonymous
        // object WITH members — rendered by the 9.3b display slice
        // (this pin was containment-until-9.3b after the pre-9.3a
        // syntax bridge retired).
        assert_eq!(
            codes_of_with_options(
                "const x: [number] & { p: string } = [1];",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn contextual_tuple_arity_gap_remains_contained() {
        assert_eq!(
            codes_of_with_options(
                "const x: [...number[]] & { length: 2 } = [0, 0];",
                &strict_options(),
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn satisfies_literal_reports_elaborated_member_error() {
        assert_eq!(
            codes_of_with_options(
                "const x = { a: 1 } satisfies { a: string };",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn invalid_interface_computed_name_reports_resolution_error() {
        assert_eq!(codes_of("interface I { [NotThere.x](): void; }"), [2304]);
        assert_eq!(
            codes_of("declare const ns: {}; interface I { [ns.missing](): void; }"),
            [2339]
        );
    }

    #[test]
    fn computed_object_setter_is_checked_without_a_use_site() {
        assert_eq!(
            codes_of_with_options(
                "declare const k: unique symbol; const o = { set [k](v) {} };",
                &strict_options(),
            ),
            [7032, 7006]
        );
    }

    #[test]
    fn used_expect_error_consuming_a_real_row_stays_silent() {
        // Named for the KEEP-OFF era ("stays silent while checker is
        // incomplete") until the 2026-07-19 B32 amendment: the 2578
        // emitter is LIVE since 5.9d, and this shape is silent
        // because the directive consumes the real straight-line 2454
        // (use before assignment, live since 6.2) — a USED directive
        // reports nothing.
        assert_eq!(
            codes_of("let x: number;\n// @ts-expect-error\nx;\n"),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn eopt_widened_absent_property_takes_the_missing_flavor() {
        // m4-review A13: getUndefinedProperty types the context-added
        // absent property undefinedOrMissingType (tsc 67990). Under
        // exactOptionalPropertyTypes the widened first branch stays
        // assignable to `c?: string` (missing ⊂ string|missing where
        // plain undefined is not), the directive has nothing to
        // consume, and the unused 2578 surfaces — oracle row
        // (2578, 69, 19), probed vs vendored 6.0.3 (eOPT + strict,
        // noLib). The undefined flavor instead made the relation
        // reject, and the display-band containment of that report
        // marked the directive used — silence where the oracle
        // reports.
        let options = CompilerOptions {
            exact_optional_property_types: Some(true),
            ..CompilerOptions::default()
        };
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const b: boolean;\nconst o = b ? { a: 1 } : { a: 2, c: \"x\" };\n// @ts-expect-error\nconst t: { a: number; c?: string } = o;\n".to_owned(),
            }],
            &options,
        );
        let rows: Vec<(u32, Option<u32>, Option<u32>)> = result
            .diagnostics
            .iter()
            .map(|d| (d.code(), d.start, d.length))
            .collect();
        assert_eq!(rows, [(2578, Some(69), Some(19))]);
    }

    #[test]
    fn partial_flow_check_does_not_hide_unrelated_unused_expect_error() {
        // The branch-dependent 2454 is REAL since 6.4b (the condition
        // arm is live and a plain boolean guard narrows nothing) and
        // no longer hides the unrelated 2578.
        assert_eq!(
            codes_of(
                "declare const c: boolean;\nlet x: number;\nif (c) { x = 1; }\nx;\n// @ts-expect-error\nconst y = 1;\n"
            ),
            [2454, 2578]
        );
    }

    #[test]
    fn condition_join_reports_use_before_assignment() {
        // The if-without-else join and condition arm are live, and a
        // plain boolean guard narrows nothing — the join computes
        // number ∪
        // (number | undefined) and the ladder's 2454 fires like
        // tsc's. (The straight-line form reports since 6.2, the
        // condition-free try/catch join since 6.3 — pinned below.)
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const c: boolean;\nlet x: number;\nif (c) { x = 1; }\nx;\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2454]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn const_variable_guard_inlines_into_the_condition() {
        // narrowType's Identifier arm (6.4h): `if (isStr)` narrows x
        // through the const's initializer (`typeof x === "string"`),
        // so the fs(x) argument checks clean — no diagnostic and no
        // containment (pre-6.4h the inline conditions flagged the
        // query and the failed-argument gate partial-marked).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function fs(s: string): void;\ndeclare const x: string | number;\nconst isStr = typeof x === \"string\";\nif (isStr) { fs(x); }\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn destructuring_query_does_not_inline_const_guards() {
        // The synthetic destructuring reference never const-inlines:
        // tsc's isConstantReference reads the factory node's
        // resolvedSymbol — never populated — and its access arm lands
        // on isReadonlySymbol(unknownSymbol) = false (70385). The
        // guard must NOT narrow p to string, so `p === 42` stays a
        // legal overlap (no 2367) exactly like tsc.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const o: { p: string | number };\nconst isStr = typeof o.p === \"string\";\nif (isStr) {\n  const { p } = o;\n  if (p === 42) {}\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(
            result.partial_checks.len(),
            0,
            "{:?}",
            result.partial_checks
        );
    }

    #[test]
    fn empty_string_typeof_case_witnesses_none() {
        // getSwitchClauseTypeOfWitnesses (69955): `case "":` is a
        // FALSY text — the witness is None like a default clause, the
        // clause narrows to never (tsc's `text ? ... : neverType`),
        // and the never-typed assignment checks clean. tsc reports
        // ONLY the case-comparability 2678 (oracle-verified). Pre-fix
        // the "" witness took the host-object fallback and narrowed
        // unknown to object — a 2322 FP alongside.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const x: unknown;\nswitch (typeof x) {\n  case \"\": {\n    const y: never = x;\n    break;\n  }\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2678]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn multi_signature_body_inference_resolves_the_selection() {
        // m6 7.6 flip: getEffectsSignature's some() sweep reaches the
        // LIVE body-inference arm per member — `!!v` infers no
        // predicate (its false branch survives reduction), so the
        // selection resolves to NO effects signature and BOTH uses
        // report their straight-line 2454, unflagged (oracle q2:
        // (2454, 137, 1) + (2454, 152, 1), vendored 6.0.3 strict).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "function f(v: unknown) { return !!v; }\nfunction g(v: unknown) { return !!v; }\ndeclare const h: typeof f & typeof g;\nlet x: number;\nif (h(x)) { x = 1; }\nx;\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2454, 2454]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn body_inference_resolves_the_runtime_trigger() {
        // `!!v` infers no predicate, so the guard call
        // carries no effects, and the trailing use reports its
        // straight-line 2454 for real alongside the argument use
        // (oracle q6: (2454, 60, 1) + (2454, 75, 1), vendored 6.0.3
        // strict). No partial mark remains.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "function f(v: unknown) { return !!v; }\nlet x: number;\nif (f(x)) { x = 1; }\nx;\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2454, 2454]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn join_dependent_auto_type_resolves_without_implicit_any() {
        // The auto-typed join computes number |
        // undefined for real — no implicit-any diagnostic and no
        // partial mark, like tsc.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const c: boolean;\nlet x;\nif (c) { x = 1; }\nx;\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn join_dependent_auto_type_resolves_through_guard_calls() {
        // The guard call resolves through body inference
        // (no predicate from `!!v`), the auto-typed join computes
        // number | undefined for real, and tsc is CLEAN on this
        // shape (oracle q7, vendored 6.0.3 strict) — no rows, no
        // partial mark.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "function f(v: unknown) { return !!v; }\nlet x;\nif (f(x)) { x = 1; }\nx;\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn branch_join_reports_use_before_assignment_across_try_catch() {
        // try/catch joins carry no condition nodes (the try-path
        // antecedent terminates at the x=1 assignment arm; the
        // catch-path runs to Start), so the 6.3 branch label computes
        // the REAL union: number ∪ (number | undefined) → the ladder's
        // 2454 fires like tsc's.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "let x: number;\ntry { x = 1; } catch {}\nx;\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2454]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn loop_fixpoint_converges_across_back_edges() {
        // The 6.3 loop-label fixpoint: `while (true)` binds no
        // condition node (the binder's literal-condition passthrough),
        // so both antecedents resolve through live arms. Entry assigns
        // "a" → string; the back edge re-assigns "b" → string; the
        // fixpoint converges to string and fs(x) is clean.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function fs(s: string): void;\nlet x: string | number = \"a\";\nwhile (true) {\n  fs(x);\n  x = \"b\";\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn loop_fixpoint_accumulates_widening_back_edge_types() {
        // The divergent twin of the pin above: the back edge assigns a
        // NUMBER, so the fixpoint's second pass adds it and the union
        // reaches the declared string | number — fs(x) genuinely fails
        // under tsc (2345). Pins the accumulate-then-break direction
        // (an antecedent equal to the declared type stops the walk) —
        // AND the report surface: with the [FLOW M5] failure-face
        // gates retired at 6.6f, the true positive REPORTS
        // (oracle-exact: 2345 at the argument).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function fs(s: string): void;\nlet x: string | number = \"a\";\nwhile (true) {\n  fs(x);\n  x = 1;\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2345]
        );
        assert_eq!(
            result
                .partial_checks
                .iter()
                .map(|p| p.reason.as_str())
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn speculative_overload_failure_in_fixpoint_leaves_no_signature_memo() {
        // The g2 shape of controlFlowIterationErrorsAsync: the bare
        // `x;` query's back-edge pull speculatively resolves foo(x),
        // whose overload failure stashes a failure-face
        // resolvedSignature (resolveCall 76629). The mid-fixpoint exit
        // must clear that stash (tsc 77505's `: cached`): if it
        // survived, the later assignment-statement check would hit the
        // memo, skip argument checking, and let the failure-face
        // return type reach the assignment relation — a 2322 tsc never
        // emits. Post-6.6f expected (oracle-exact): ONE 2769 (the
        // overload failure at the real call check), no 2322, no
        // partial marks.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function foo(x: string): number;\ndeclare function foo(x: number): string;\ndeclare const cond: boolean;\nlet x: string | number | boolean;\nx = \"\";\nwhile (cond) {\n  x;\n  x = foo(x);\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2769]
        );
        assert_eq!(
            result
                .partial_checks
                .iter()
                .map(|p| p.reason.as_str())
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn loop_fixpoint_joins_evolving_arrays_incomplete_first_pass() {
        // Evolving arrays THROUGH the fixpoint: at tn(a) the loop
        // label joins {entry: evolving[never], back edge:
        // ArrayMutation(push 1)}. The mutation's input walk re-enters
        // this same label mid-back-edge and takes the in-progress arm
        // (the partial union tagged INCOMPLETE); the join then unions
        // element types into evolving[number], finalized to number[]
        // at the use — clean, like tsc.
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function tn(ns: number[]): void;\nlet a = [];\nwhile (true) {\n  tn(a);\n  a.push(1);\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn loop_fixpoint_reports_2454_through_live_conditions() {
        // 6.4b: the fixpoint through a LIVE (non-narrowing) boolean
        // condition computes the real per-use unions — both loop uses
        // report 2454 like tsc, nothing partial-marks, and the
        // second query may legitimately hit flowLoopCaches (same
        // key, unflagged).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare const cond: boolean;\nlet x: number;\nwhile (true) {\n  x;\n  x;\n  if (cond) { x = 1; }\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2454, 2454]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn loop_fixpoint_reports_for_real_through_guard_calls() {
        // The guard call resolves through body inference (no
        // predicate), the loop fixpoint runs, and all
        // THREE uses report their 2454 exactly like tsc (oracle q5:
        // (2454, 71/76/87), vendored 6.0.3 strict).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "function f(v: unknown) { return !!v; }\nlet x: number;\nwhile (true) {\n  x;\n  x;\n  if (f(x)) { x = 1; }\n}\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            [2454, 2454, 2454]
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn arithmetic_face_narrows_through_the_inferred_predicate() {
        // m6 7.6 flip of the M5 post-close D2 pin: isNum's predicate
        // is INFERRED for real, u narrows to number inside the
        // guard, and the arithmetic face is clean like tsc
        // (verify/d2_operator_face.ts + oracle q3).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "function isNum(x: unknown) { return typeof x === \"number\"; }\nfunction f(u: string | number) {\n    if (isNum(u)) {\n        const a = u * 2;\n    }\n}\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Error)
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn assignment_face_relates_through_the_inferred_predicate() {
        // m6 7.6 flip of the M5 post-close D1 pin: isNum's predicate
        // is INFERRED for real, u narrows to number inside the
        // compound RHS, and the assignment face relates cleanly like
        // tsc (verify/d1_assignment_face.ts + oracle q4).
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "function isNum(x: unknown) { return typeof x === \"number\"; }\nfunction g(u: string | number) {\n    let t: { p: number };\n    if (isNum(u)) {\n        t = { p: u };\n        void t;\n    }\n}\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn dependent_parameter_narrowing_types_rest_tuple_slices() {
        // getNarrowedTypeOfSymbol arm 2 (72040-72060) over a CONCRETE
        // union-of-tuples rest type — live since the 6.2 review fix
        // (pre-fix the whole reference stopped at a recovery boundary).
        // kind types as the [0]-slice "a" | "b", so takeAB accepts it.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function f(cb: (...args: [\"a\", number] | [\"b\", string]) => void): void;\ndeclare function takeAB(x: \"a\" | \"b\"): void;\nf((kind, _data) => { takeAB(kind); });\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|d| d.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn dependent_parameter_narrowing_skips_a_non_union_rest_type() {
        // Nearest non-firing side of the 72046 gate: a single tuple is
        // contextually indexed normally, but does not enter the
        // dependent union-of-tuples flow walk.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function f(cb: (...args: [\"a\", number]) => void): void;\n\
                       declare function takeA(x: \"a\"): void;\n\
                       f((kind, _data) => { takeA(kind); });\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            Vec::<u32>::new()
        );
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn dependent_parameter_narrowing_stops_after_parameter_assignment() {
        // getNarrowedTypeOfSymbol 72043-72046: assignment to one of
        // the dependent parameters keeps the union-of-tuples rest
        // type on its non-firing path. The property access therefore
        // retains both tuple payloads and reports tsc 6.0.3's exact
        // chained 2339 rather than narrowing data from kind.
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "declare function f(cb: (...args: [\"a\", { aOnly: 1 }] | [\"b\", { bOnly: 1 }]) => void): void;\nf((kind, data) => { kind = kind; if (kind === \"a\") { data.aOnly; } });\n".to_owned(),
            }],
            &CompilerOptions {
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        let diagnostics = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2339)
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics[0];
        assert_eq!((diagnostic.start, diagnostic.length), (Some(150), Some(5)));
        assert_eq!(
            (diagnostic.message.code, diagnostic.message.text.as_str()),
            (
                2339,
                "Property 'aOnly' does not exist on type '{ aOnly: 1; } | { bOnly: 1; }'.",
            )
        );
        assert_eq!(diagnostic.message.next.len(), 1);
        let child = &diagnostic.message.next[0];
        assert_eq!(
            (child.code, child.text.as_str()),
            (
                2339,
                "Property 'aOnly' does not exist on type '{ bOnly: 1; }'.",
            )
        );
        assert!(child.next.is_empty());
        assert_eq!(result.partial_checks.len(), 0);
    }

    #[test]
    fn unused_expect_error_reports_2578() {
        assert_eq!(codes_of("// @ts-expect-error\nconst x = 1;\n"), [2578]);
    }

    #[test]
    fn suggestion_does_not_consume_or_hide_expect_error() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "export {};\n// @ts-expect-error\nconst dead = 1;\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code(), diagnostic.category()))
                .collect::<Vec<_>>(),
            [
                (2578, DiagnosticCategory::Error),
                (6133, DiagnosticCategory::Suggestion),
            ]
        );
        assert_eq!(
            result
                .semantic_diagnostics
                .iter()
                .map(|diagnostic| { (diagnostic.code(), diagnostic.category()) })
                .collect::<Vec<_>>(),
            [(2578, DiagnosticCategory::Error)]
        );
        assert_eq!(
            result
                .suggestion_diagnostics
                .iter()
                .map(|diagnostic| { (diagnostic.code(), diagnostic.category()) })
                .collect::<Vec<_>>(),
            [(6133, DiagnosticCategory::Suggestion)]
        );
    }

    #[test]
    fn expect_error_inside_contained_object_accessor_body_is_exempt() {
        // m4-review S8 (oracle: vendored tsc 6.0.3, noLib, strict,
        // 2026-07-19): clean — the directive consumes the body's
        // 2322. Since the A2 routing (checkAccessorDeclaration owns
        // the deferred obj-literal accessor) the body is genuinely
        // checked and the suppression marks the directive used —
        // tsc's own mechanism; the S8-era wholly-unchecked-subtree
        // exemption is retired.
        assert_eq!(
            codes_of(
                "const o = {\n    get x() {\n        // @ts-expect-error\n        let a: number = \"s\";\n        return 1;\n    },\n};\n"
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn checked_js_marks_directives_from_the_full_diagnostic_stream() {
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\n// @ts-expect-error\n(1)();\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2578),
            "the suppressed checked-JS diagnostic must mark the directive used: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn contained_expect_error_target_does_not_report_2578() {
        assert_eq!(
            codes_of(
                "// @ts-expect-error\n\
                 const bad = (() => 1) satisfies number;\n"
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn expect_error_on_a_curtained_2507_extends_is_exempt() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23): clean —
        // the directive consumes the 2507. The bigint-literal face
        // curtains the port's 2507, so the drop must mark the report
        // anchor partial or the directive accounting fabricates 2578
        // (9.3b5 review r1).
        assert_eq!(
            codes_of("declare const x: 1n;\n// @ts-expect-error\nclass C extends x {}\n"),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn expect_error_on_a_curtained_2509_base_return_is_exempt() {
        // oracle (vendored 6.0.3, strict, noLib, 2026-07-23): clean —
        // the directive consumes the 2509 (base constructor return
        // type 1n is not an object type). Same containment-marking
        // rule as the 2507 twin above.
        assert_eq!(
            codes_of("declare const x: new () => 1n;\n// @ts-expect-error\nclass C extends x {}\n"),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn directive_inside_a_checked_mapped_type_is_not_blanket_exempted() {
        assert_eq!(
            codes_of(
                "type M<T> = {\n\
                   // @ts-expect-error\n\
                   [K in keyof T]: number;\n\
                 };\n"
            ),
            [2578]
        );
    }

    #[test]
    fn checked_js_exposes_supported_checker_call_diagnostics() {
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\n(1)();\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2349),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_publishes_symbol_free_property_misses() {
        let source = "const n = 1;\nn.missing;\n";
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                source.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_contains_symbol_bearing_expando_property_misses() {
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.js".to_owned(),
                text: "const value = {};\nvalue.added = 1;\nvalue.added;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 2339),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_publishes_jsdoc_symbol_free_property_misses() {
        let source = "/** @type {number} */\nconst n = 1;\nn.missing;\n";
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                source.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_publishes_property_misses_on_non_js_declared_types() {
        let source = "value.missing;\n";
        let result = check_program(
            &[
                InputFile {
                    name: "types.d.ts".to_owned(),
                    text: "interface Declared { known: number }\ndeclare const value: Declared;\n"
                        .to_owned(),
                },
                InputFile {
                    name: "a.js".to_owned(),
                    text: source.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.file_name.as_deref(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                Some("a.js"),
                2339,
                source.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_non_js_declared_prototype_replacement_reports_assignment_type() {
        let source = "C.prototype = {};\nC.bar = 2;\n";
        let result = check_program(
            &[
                InputFile {
                    name: "types.d.ts".to_owned(),
                    text: "declare namespace C { function bar(): void }\n".to_owned(),
                },
                InputFile {
                    name: "a.js".to_owned(),
                    text: source.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2322,
                source.find("C.bar").expect("typed assignment") as u32,
                "C.bar".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_publishes_plain_value_module_property_reads() {
        let source = "exports.missing();\nexports.created = 1;\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                source.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_contains_assignment_bearing_value_module_property_misses() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "function C() { this.p = 1; }\n\
                       C.prototype = { q: 2 };\n\
                       const c = new C();\n\
                       c.q;\n"
                    .to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
    }

    #[test]
    fn checked_js_publishes_assignment_bearing_class_property_reads() {
        let source = "class C { constructor() { this.p = 1; } }\n\
                      C.prototype = { q: 2 };\n\
                      const c = new C();\n\
                      c.q;\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(2339, source.rfind('q').expect("missing property") as u32, 1,)]
        );
    }

    #[test]
    fn checked_js_publishes_direct_this_class_property_reads() {
        let source = "class C { method() { this.missing; } }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                source.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_publishes_imported_class_alias_expando_misses() {
        let source = "import { C, value } from \"./defs\";\n\
                      C.missing = 1;\n\
                      value.added = 1;\n";
        let result = check_program(
            &[
                InputFile {
                    name: "defs.js".to_owned(),
                    text: "export class C {}\nexport const value = {};\n".to_owned(),
                },
                InputFile {
                    name: "main.js".to_owned(),
                    text: source.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        // TS 6.0.3 exact identity: both imported assignment sites are
        // rejected, producing these two 2339 rows.
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.file_name.as_deref(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    Some("main.js"),
                    2339,
                    source.find("missing").expect("missing property") as u32,
                    "missing".len() as u32,
                ),
                (
                    Some("main.js"),
                    2339,
                    source.find("added").expect("added property") as u32,
                    "added".len() as u32,
                ),
            ]
        );
    }

    #[test]
    fn checked_js_publishes_jsdoc_adjacent_private_name_misses() {
        let source = "class C {\n\
                        #known;\n\
                        method() {\n\
                          /** @type {string} */\n\
                          this.#missing;\n\
                          this.#known;\n\
                        }\n\
                      }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    7008,
                    source.find("#known").expect("unused private field") as u32,
                    "#known".len() as u32,
                ),
                (
                    2339,
                    source.find("#missing").expect("missing private name") as u32,
                    "#missing".len() as u32,
                ),
            ]
        );
    }

    #[test]
    fn checked_js_publishes_chained_this_assignment_misses() {
        let source = "this.x = {};\n\
                      this.x.missing = {};\n\
                      /** @constructor */\n\
                      function F() {\n\
                        this.x = {};\n\
                        this.x.alsoMissing = {};\n\
                      }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    2339,
                    source.find("missing").expect("global chained miss") as u32,
                    "missing".len() as u32,
                ),
                (
                    2339,
                    source
                        .find("alsoMissing")
                        .expect("constructor chained miss") as u32,
                    "alsoMissing".len() as u32,
                ),
            ]
        );
    }

    #[test]
    fn checked_js_publishes_chained_identifier_empty_assignment_misses() {
        let source = "let A;\n\
                      A = {};\n\
                      A.prototype.b = {};\n\
                      let B;\n\
                      B = {};\n\
                      B.direct = {};\n\
                      B.direct;\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        // TS 6.0.3 exact identity: prototype plus both direct access
        // sites produce three 2339 rows.
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    2339,
                    source.find("prototype").expect("chained missing property") as u32,
                    "prototype".len() as u32,
                    "Property 'prototype' does not exist on type '{}'.",
                ),
                (
                    2339,
                    source.find("direct").expect("direct assignment miss") as u32,
                    "direct".len() as u32,
                    "Property 'direct' does not exist on type '{}'.",
                ),
                (
                    2339,
                    source.rfind("direct").expect("direct read miss") as u32,
                    "direct".len() as u32,
                    "Property 'direct' does not exist on type '{}'.",
                ),
            ]
        );
    }

    #[test]
    fn checked_js_publishes_prototype_object_property_assignment_misses() {
        let source = "/** @constructor */\n\
                      var Multimap = function() {\n\
                        this._map = {};\n\
                        this._map;\n\
                        this.set;\n\
                        this.get;\n\
                        this.addon;\n\
                      };\n\
                      Multimap.prototype = {\n\
                        set: function() {},\n\
                        get() {}\n\
                      };\n\
                      Multimap.prototype.addon = function() {\n\
                        this._map;\n\
                        this.set;\n\
                        this.get;\n\
                        this.addon;\n\
                      };\n\
                      var Plain = function() {};\n\
                      Plain.prototype = { existing() {} };\n\
                      Plain.prototype.incremental = function() {};\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    2339,
                    (source
                        .find("Multimap.prototype.addon")
                        .expect("missing prototype property")
                        + "Multimap.prototype.".len()) as u32,
                    "addon".len() as u32,
                    "Property 'addon' does not exist on type '{ set: () => void; get(): void; }'.",
                ),
                (
                    2339,
                    source
                        .find("incremental")
                        .expect("plain prototype property") as u32,
                    "incremental".len() as u32,
                    "Property 'incremental' does not exist on type '{ existing(): void; }'.",
                ),
            ]
        );
    }

    #[test]
    fn checked_js_nested_constructor_this_uses_merged_prototype_members() {
        let source = "(function container() {\n\
                        /** @constructor */\n\
                        var Multimap = function() {\n\
                          this._map = {};\n\
                          this._map;\n\
                          this.set;\n\
                          this.get;\n\
                          this.addon;\n\
                        };\n\
                        Multimap.prototype = {\n\
                          set: function() {},\n\
                          get() {}\n\
                        };\n\
                        Multimap.prototype.addon = function() {};\n\
                      })();\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        // Preserve the existing assignment-LHS canary while proving
        // the earlier constructor read sees the inferred JS class's
        // complete prototype member set.
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                (source
                    .find("Multimap.prototype.addon")
                    .expect("missing prototype property")
                    + "Multimap.prototype.".len()) as u32,
                "addon".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_publishes_jsdoc_satisfies_object_literal_property_reads() {
        let source = "const value = /** @satisfies {{ present: number }} */ ({ present: 1 });\n\
                      value.present;\n\
                      value.missing;\n\
                      const asserted = /** @type {{ present: number }} */ ({ present: 1 });\n\
                      asserted.hidden;\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    2339,
                    source.find("missing").expect("satisfies-backed miss") as u32,
                    "missing".len() as u32,
                    "Property 'missing' does not exist on type '{ present: number; }'.",
                ),
                (
                    2339,
                    source.find("hidden").expect("type-assertion-backed miss") as u32,
                    "hidden".len() as u32,
                    "Property 'hidden' does not exist on type '{ present: number; }'.",
                ),
            ]
        );
    }

    #[test]
    fn checked_js_valid_template_nested_prototype_read_is_parse_all_crash_guard() {
        // TypeScript 6.0.3 with ParseAll crashes in
        // typeToString -> lookupSymbolChainWorker while trying to
        // format the otherwise expected 2339 for `missing`. Keep this
        // fixture as a crash-free valid-JSDoc guard; the non-crashing
        // oracle face for the prototype read is pinned separately.
        let source = "/** @template T */\n\
                      class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [(
                6133,
                source.find("@template").expect("unused template tag") as u32,
                12,
                "'T' is declared but its value is never read.",
            )]
        );
        assert!(
            result.partial_checks.is_empty(),
            "oracle-crash control flow is not partial-model audit debt: {:#?}",
            result.partial_checks
        );
    }

    #[test]
    fn checked_js_outer_template_display_crash_does_not_stop_later_errors() {
        let source = "/** @template T */\n\
                      class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n\
                      const later = { present: 1 };\n\
                      later.missing;\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(diagnostic.code(), 2339 | 6133))
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    6133,
                    source.find("@template").expect("unused template tag") as u32,
                    12,
                ),
                (
                    2339,
                    source.rfind("missing").expect("later independent miss") as u32,
                    "missing".len() as u32,
                ),
            ]
        );
        assert!(result.partial_checks.is_empty());
    }

    #[test]
    fn checked_js_outer_template_display_crash_consumes_preceding_expect_error_range_only() {
        let source = "/** @template T */\n\
                      class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              // @ts-expect-error\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [6133],
            "the contained oracle crash must consume the directive without fabricating TS2578"
        );
        assert!(result.partial_checks.is_empty());
    }

    #[test]
    fn checked_js_publishes_this_prototype_class_property_reads() {
        let source = "class Outer {\n\
                        method() {\n\
                          class Inner {\n\
                            static check() {\n\
                              this.prototype.missing;\n\
                            }\n\
                          }\n\
                          Inner;\n\
                        }\n\
                      }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                source.find("missing").expect("class prototype miss") as u32,
                "missing".len() as u32,
                "Property 'missing' does not exist on type 'Inner'.",
            )]
        );
        assert!(result.partial_checks.is_empty());
    }

    #[test]
    fn checked_js_publishes_jsdoc_chained_static_assignment_this_reads() {
        let source = "function A() {\n\
                        this.instanceOnly = 1;\n\
                      }\n\
                      /** @param {number} n */\n\
                      A.s = A.t = function g(n) {\n\
                        return n + this.instanceOnly;\n\
                      };\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [(
                2339,
                source
                    .rfind("instanceOnly")
                    .expect("static-side instance miss") as u32,
                "instanceOnly".len() as u32,
                "Property 'instanceOnly' does not exist on type 'typeof A'.",
            )]
        );
    }

    #[test]
    fn checked_js_publishes_class_this_miss_from_jsdoc_this_annotated_arrow() {
        let source = "/** @typedef {{ fn(a: string): void }} T */\n\
                      class C {\n\
                        /**\n\
                         * @this {T}\n\
                         * @param {string} a\n\
                         */\n\
                        p = (a) => this.missing(a);\n\
                      }\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    2730,
                    source.find("@this").expect("JSDoc this tag") as u32 + 1,
                    "this".len() as u32,
                    "An arrow function cannot have a 'this' parameter.",
                ),
                (
                    2339,
                    source.find("missing").expect("lexical class this miss") as u32,
                    "missing".len() as u32,
                    "Property 'missing' does not exist on type 'C'.",
                ),
            ]
        );
    }

    #[test]
    fn checked_js_publishes_primitive_module_exports_assignment_misses() {
        let primitive = "module.exports = 1;\nmodule.exports.missing = 1;\n";
        let result = check_program(
            &[
                InputFile {
                    name: "requires.d.ts".to_owned(),
                    text: "declare var module: { exports: any };\n".to_owned(),
                },
                InputFile {
                    name: "primitive.js".to_owned(),
                    text: primitive.to_owned(),
                },
                InputFile {
                    name: "object.js".to_owned(),
                    text: "module.exports = {};\nmodule.exports.allowed = 1;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.file_name.as_deref(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                ))
                .collect::<Vec<_>>(),
            [(
                Some("primitive.js"),
                2339,
                primitive.find("missing").expect("missing property") as u32,
                "missing".len() as u32,
            )]
        );
    }

    #[test]
    fn checked_js_common_js_object_replacement_unions_direct_export_members() {
        let source = "const mod1 = require('./mod1');\n\
                      mod1.justExport.toFixed();\n\
                      mod1.bothBefore.toFixed();\n\
                      mod1.bothAfter.toFixed();\n\
                      mod1.justProperty.length;\n";
        let result = check_program_with_libs(
            &[es5_lib()],
            &[
                InputFile {
                    name: "requires.d.ts".to_owned(),
                    text: "declare var module: { exports: any };\n\
                           declare function require(name: string): any;\n"
                        .to_owned(),
                },
                InputFile {
                    name: "mod1.js".to_owned(),
                    text: "module.exports.bothBefore = 'string';\n\
                           module.exports = {\n\
                               justExport: 1,\n\
                               bothBefore: 2,\n\
                               bothAfter: 3,\n\
                           };\n\
                           module.exports.bothAfter = 'string';\n\
                           module.exports.justProperty = 'string';\n"
                        .to_owned(),
                },
                InputFile {
                    name: "a.js".to_owned(),
                    text: source.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.file_name.clone(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text().to_owned(),
                    diagnostic
                        .message
                        .next
                        .first()
                        .map(|message| message.text.clone()),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    Some("a.js".to_owned()),
                    2339,
                    source.find("bothBefore.toFixed").expect("before access") as u32
                        + "bothBefore.".len() as u32,
                    "toFixed".len() as u32,
                    "Property 'toFixed' does not exist on type 'number | \"string\"'.".to_owned(),
                    Some("Property 'toFixed' does not exist on type '\"string\"'.".to_owned()),
                ),
                (
                    Some("a.js".to_owned()),
                    2339,
                    source.find("bothAfter.toFixed").expect("after access") as u32
                        + "bothAfter.".len() as u32,
                    "toFixed".len() as u32,
                    "Property 'toFixed' does not exist on type 'number | \"string\"'.".to_owned(),
                    Some("Property 'toFixed' does not exist on type '\"string\"'.".to_owned()),
                ),
            ]
        );
    }

    #[test]
    fn checked_js_exposes_typed_declaration_arity_diagnostics() {
        let result = check_program(
            &[
                InputFile {
                    name: "defs.d.ts".to_owned(),
                    text: "declare function f1(p: void): void;\n\
                           declare function f2(p: undefined): void;\n\
                           declare function f3(p: unknown): void;\n\
                           declare function f4(p: any): void;\n\
                           interface I<T> { m(p: T): void; }\n\
                           declare const o1: I<void>;\n\
                           declare const o2: I<undefined>;\n\
                           declare const o3: I<unknown>;\n\
                           declare const o4: I<any>;\n"
                        .to_owned(),
                },
                InputFile {
                    name: "a.js".to_owned(),
                    text: "f1();\no1.m();\nf2();\nf3();\nf4();\no2.m();\no3.m();\no4.m();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                strict: Some(true),
                ..CompilerOptions::default()
            },
        );
        let arity_rows = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2554)
            .collect::<Vec<_>>();
        assert_eq!(arity_rows.len(), 6, "{:#?}", result.diagnostics);
        assert!(arity_rows
            .iter()
            .all(|diagnostic| { diagnostic.file_name.as_deref() == Some("a.js") }));
    }

    #[test]
    fn checked_js_publishes_non_jsdoc_readonly_enum_expandos() {
        let source = "lf.Order = {};\nlf.Order.DESC = 0;\nlf.Order.ASC = 1;\n";
        let result = check_program(
            &[
                InputFile {
                    name: "types.d.ts".to_owned(),
                    text: "declare namespace lf { export enum Order { ASC, DESC } }\n".to_owned(),
                },
                InputFile {
                    name: "enums.js".to_owned(),
                    text: source.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(2),
                ..CompilerOptions::default()
            },
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.file_name.as_deref(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                    diagnostic.message_text(),
                ))
                .collect::<Vec<_>>(),
            [
                (
                    Some("enums.js"),
                    2540,
                    source.find("DESC").expect("DESC assignment") as u32,
                    "DESC".len() as u32,
                    "Cannot assign to 'DESC' because it is a read-only property.",
                ),
                (
                    Some("enums.js"),
                    2540,
                    source.find("ASC =").expect("ASC assignment") as u32,
                    "ASC".len() as u32,
                    "Cannot assign to 'ASC' because it is a read-only property.",
                ),
            ]
        );
    }

    #[test]
    fn complex_union_guards_report_across_intersection_template_and_tuple_paths() {
        let ten_objects = |suffix: u8| {
            ('a'..='j')
                .map(|name| format!("{{{name}{suffix}: any}}"))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let source = format!(
            "type U1 = {};\n\
             type U2 = {};\n\
             type U3 = {};\n\
             type U4 = {};\n\
             type U5 = {};\n\
             type U100000 = U1 & U2 & U3 & U4 & U5;\n\
             type D = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;\n\
             type D100000 = `${{D}}${{D}}${{D}}${{D}}${{D}}`;\n\
             type TD = [0] | [1] | [2] | [3] | [4] | [5] | [6] | [7] | [8] | [9];\n\
             type T100000 = [...TD, ...TD, ...TD, ...TD, ...TD];\n\
             type D20 = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20;\n\
             type Spacing = `0` | `${{number}}px` | `${{number}}rem` | `s${{D20}}`;\n\
             type SpacingShorthand = `${{Spacing}} ${{Spacing}} ${{Spacing}} ${{Spacing}}`;\n",
            ten_objects(1),
            ten_objects(2),
            ten_objects(3),
            ten_objects(4),
            ten_objects(5),
        );
        assert_eq!(codes_of(&source), [2590, 2590, 2590, 2590]);
    }

    #[test]
    fn checked_js_jsdoc_type_checks_its_initializer() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\n/** @type {number} */\nlet value = \"wrong\";\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2322),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_does_not_treat_other_jsdoc_tags_as_type() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\n/** @types {number} */\nlet value = \"ok\";\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2322),
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_jsdoc_augments_reports_only_effective_hosts() {
        let source = "/** @extends {A} */\n\
                      /** @constructor */\n\
                      class A {}\n\
                      /** @augments A */\n\
                      function f() {}\n\
                      class B {}\n\
                      /** @augments A */\n\
                      class C extends B {}\n\
                      /** @augments */\n\
                      class D extends A {}\n\
                      /** @extends {A} */\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(99),
                ..CompilerOptions::default()
            },
        );
        let rows = result
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 8022 | 8023))
            .map(|diagnostic| {
                (
                    diagnostic.file_name.as_deref(),
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.code(),
                    diagnostic.message_text(),
                )
            })
            .collect::<Vec<_>>();
        let function_name = source.find("f()").expect("function name") as u32;
        let mismatch_name = (source
            .find("@augments A */\nclass C")
            .expect("mismatch tag")
            + "@augments ".len()) as u32;
        let missing_name =
            (source.find("@augments */").expect("missing tag") + "@augments".len()) as u32;
        assert_eq!(
            rows,
            [
                (
                    None,
                    None,
                    None,
                    8022,
                    "JSDoc '@extends' is not attached to a class.",
                ),
                (
                    Some("a.js"),
                    Some(function_name),
                    Some(1),
                    8022,
                    "JSDoc '@augments' is not attached to a class.",
                ),
                (
                    Some("a.js"),
                    Some(mismatch_name),
                    Some(1),
                    8023,
                    "JSDoc '@augments A' does not match the 'extends B' clause.",
                ),
                (
                    Some("a.js"),
                    Some(missing_name),
                    Some(0),
                    8023,
                    "JSDoc '@augments ' does not match the 'extends A' clause.",
                ),
                (
                    Some("a.js"),
                    Some(source.len() as u32),
                    Some(0),
                    8022,
                    "JSDoc '@extends' is not attached to a class.",
                ),
            ],
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_detached_augments_document_keeps_fileless_8022() {
        let source = "class A {}\n\
                      /** @extends {A} */\n\
                      \n\
                      /** @constructor */\n\
                      class B extends A {}\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(99),
                ..CompilerOptions::default()
            },
        );
        let rows = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 8022)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.as_deref(),
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message_text(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [(
                None,
                None,
                None,
                "JSDoc '@extends' is not attached to a class.",
            )],
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn checked_js_detached_implements_document_keeps_fileless_8022() {
        let source = "class A {}\n\
                      /** @implements {A} */\n\
                      /** @constructor */\n\
                      class B {}\n\
                      /** @implements {A} */\n\
                      class C {}\n";
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: source.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                target: Some(99),
                ..CompilerOptions::default()
            },
        );
        let rows = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 8022)
            .map(|diagnostic| {
                (
                    diagnostic.file_name.as_deref(),
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.message_text(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [(
                None,
                None,
                None,
                "JSDoc '@implements' is not attached to a class.",
            )],
            "{:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn jsdoc_augments_projection_preserves_matching_siblings_and_typescript() {
        let valid_js = "class A {}\n\
                        /** @extends {A} */\n\
                        class B extends A {}\n\
                        /** @extends { A } */\n\
                        class C extends A {}\n\
                        /** @extends {A<{ value: string }>} */\n\
                        class Generic extends A {}\n\
                        /** prose @extends {B} */\n\
                        class D extends B {}\n";
        let options = CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            target: Some(99),
            ..CompilerOptions::default()
        };
        for (name, text) in [
            ("a.js", valid_js),
            (
                "a.ts",
                "/** @augments Wrong */\nclass Typed extends Actual {}\n",
            ),
        ] {
            let result = check_program(
                &[InputFile {
                    name: name.to_owned(),
                    text: text.to_owned(),
                }],
                &options,
            );
            assert!(
                result
                    .diagnostics
                    .iter()
                    .all(|diagnostic| !matches!(diagnostic.code(), 8022 | 8023)),
                "{name}: {:#?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn checked_js_set_only_accessors_use_jsdoc_parameter_annotations() {
        let source = "// @ts-check\n\
                      class C {\n\
                        /** @param {string} value */\n\
                        set instance(value) {}\n\
                        /** @param {number} value */\n\
                        static set stat(value) {}\n\
                      }\n\
                      const c = new C();\n\
                      c.instance = 1;\n\
                      C.stat = \"bad\";\n";
        for target in [1, 2] {
            let result = check_program(
                &[InputFile {
                    name: "a.js".to_owned(),
                    text: source.to_owned(),
                }],
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    strict: Some(true),
                    target: Some(target),
                    ..CompilerOptions::default()
                },
            );
            assert_eq!(
                result
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| matches!(diagnostic.code(), 2322 | 7032))
                    .map(|diagnostic| (
                        diagnostic.code(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                    ))
                    .collect::<Vec<_>>(),
                [
                    (
                        2322,
                        source.find("c.instance").expect("instance assignment") as u32,
                        "c.instance".len() as u32,
                    ),
                    (
                        2322,
                        source.find("C.stat").expect("static assignment") as u32,
                        "C.stat".len() as u32,
                    ),
                ],
                "target {target}: {:#?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn checked_js_super_call_uses_effective_jsdoc_extends_type_arguments() {
        let source = "// @ts-check\n\
                      /** @template T */\n\
                      class Base {\n\
                        /** @param {T} value */\n\
                        constructor(value) {}\n\
                      }\n\
                      /** @template U @extends {Base<U>} */\n\
                      class Derived extends Base {\n\
                        /** @param {U} value */\n\
                        constructor(value) { super(value); }\n\
                      }\n\
                      /** @extends {Base<number>} */\n\
                      class Fixed extends Base {\n\
                        constructor() { super(\"bad\"); }\n\
                      }\n";
        for target in [1, 2] {
            let result = check_program(
                &[InputFile {
                    name: "a.js".to_owned(),
                    text: source.to_owned(),
                }],
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    strict: Some(true),
                    target: Some(target),
                    ..CompilerOptions::default()
                },
            );
            assert_eq!(
                result
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| matches!(diagnostic.code(), 2345 | 2346))
                    .map(|diagnostic| (
                        diagnostic.code(),
                        diagnostic.start.unwrap_or(u32::MAX),
                        diagnostic.length.unwrap_or(u32::MAX),
                    ))
                    .collect::<Vec<_>>(),
                [(
                    2345,
                    source.find("\"bad\"").expect("invalid super argument") as u32,
                    "\"bad\"".len() as u32,
                )],
                "target {target}: {:#?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn single_line_directive_suppresses_through_comment_lines() {
        // Walk crosses blank and `//` lines, exactly like tsc.
        assert_eq!(
            codes_of("// @ts-ignore\n// note\n\nlet x;\nlet x;\n"),
            [2451]
        );
    }

    #[test]
    fn block_comment_shell_stops_the_directive_walk() {
        // tsc's markPrecedingCommentDirectiveLine stops at any line
        // that is non-empty and not a `//` comment — a block-comment
        // line between directive and diagnostic KEEPS the diagnostic
        // (the retired interim filter walked through these).
        assert_eq!(
            codes_of("// @ts-ignore\n/* shell */\nlet x;\nlet x;\n"),
            [2451, 2451]
        );
    }

    #[test]
    fn trailing_comment_directive_suppresses_the_next_line() {
        // Scanner-collected: the directive comment trails code on its
        // own line, so a line-start scan would miss it.
        assert_eq!(
            codes_of("let a = 1; // @ts-ignore\nlet x;\nlet x;\n"),
            [2451]
        );
    }

    #[test]
    fn multi_line_directive_keys_on_its_closing_line() {
        // Directive on the closing line: suppresses the next line.
        assert_eq!(
            codes_of("/*\n@ts-expect-error */\nlet x;\nlet x;\n"),
            [2451]
        );
        // Directive on an interior line is no directive at all.
        assert_eq!(
            codes_of("/*\n@ts-expect-error\n*/\nlet x;\nlet x;\n"),
            [2451, 2451]
        );
    }

    #[test]
    fn template_literal_fake_directive_does_not_suppress() {
        // The `// @ts-ignore` line sits INSIDE a template literal: the
        // scanner collects nothing, and the walk treats the line as a
        // `//` comment and keeps climbing past it.
        assert_eq!(
            codes_of("const s = `\n// @ts-ignore\n`;\nlet x;\nlet x;\n"),
            [2451, 2451]
        );
    }

    #[test]
    fn directive_on_the_diagnostic_line_itself_does_not_suppress() {
        // The walk starts one line ABOVE the diagnostic.
        assert_eq!(codes_of("let x;\nlet x; // @ts-ignore\n"), [2451, 2451]);
    }

    #[test]
    fn ts_nocheck_suppresses_checked_js_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-nocheck\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn jsdoc_parse_diagnostics_publish_only_for_checked_js_semantics() {
        let text = "/**\n * @typedef Name\n * @type {string}\n * @type {Oops}\n */";
        let checked = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: text.to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 8033),
            "{:#?}",
            checked.diagnostics
        );
        assert!(
            checked
                .syntactic_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code() != 8033),
            "{:#?}",
            checked.syntactic_diagnostics
        );

        for (source, check_js) in [
            (text.to_owned(), false),
            (format!("// @ts-nocheck\n{text}"), true),
        ] {
            let result = check_program(
                &[InputFile {
                    name: "a.js".to_owned(),
                    text: source,
                }],
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(check_js),
                    ..CompilerOptions::default()
                },
            );
            assert!(
                result
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code() != 8033),
                "{:#?}",
                result.diagnostics
            );
        }
    }

    #[test]
    fn ts_check_overrides_explicit_check_js_false() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(false),
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                )
            })
            .collect();

        assert_eq!(pins, [(2451, 17, 1), (2451, 24, 1)]);
    }

    #[test]
    fn checked_js_uses_comment_directives() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\n// @ts-ignore\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();

        assert_eq!(codes, [2451]);
    }

    #[test]
    fn check_js_option_uses_comment_directives() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-ignore\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();

        assert_eq!(codes, [2451]);
    }

    #[test]
    fn check_directive_matches_shebang_bom_and_unicode_line_breaks() {
        assert_eq!(
            check_directive("#!/usr/bin/env node\n// @ts-nocheck\n"),
            Some(CheckDirective::NoCheck)
        );
        assert_eq!(
            check_directive("\u{FEFF}// @ts-nocheck\n"),
            Some(CheckDirective::NoCheck)
        );
        assert_eq!(
            check_directive("\u{FEFF}#!/usr/bin/env node\n// @ts-nocheck\n"),
            None
        );
        assert_eq!(
            check_directive("// @ts-nocheck\u{2028}// @ts-check\u{2029}"),
            Some(CheckDirective::Check)
        );
        assert_eq!(
            check_directive("// @ts-check\u{2028}// @ts-nocheck\u{2029}"),
            Some(CheckDirective::NoCheck)
        );
    }

    #[test]
    fn unicode_line_break_last_ts_check_restores_semantic_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "// @ts-nocheck\u{2028}// @ts-check\u{2028}const value: string = 1;"
                    .to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2322),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn bom_before_shebang_does_not_enable_following_ts_nocheck() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "\u{FEFF}#!/usr/bin/env node\n// @ts-nocheck\nconst value: string = 1;\n"
                    .to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2322),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn ts_nocheck_after_shebang_suppresses_semantic_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "#!/usr/bin/env node\n// @ts-nocheck\nconst value: string = 1;\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn skip_lib_check_preserves_syntax_errors_and_skips_semantic_errors() {
        let result = check_program(
            &[
                InputFile {
                    name: "bad-syntax.d.ts".to_owned(),
                    text: "declare const x: ;\n".to_owned(),
                },
                InputFile {
                    name: "bad-semantic.d.ts".to_owned(),
                    text: "declare const y: Missing;\n".to_owned(),
                },
                InputFile {
                    name: "merge-a.d.ts".to_owned(),
                    text: "declare let merged: number;\n".to_owned(),
                },
                InputFile {
                    name: "merge-b.d.ts".to_owned(),
                    text: "declare let merged: string;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                skip_lib_check: Some(true),
                ..CompilerOptions::default()
            },
        );

        let pins: Vec<(String, u32, u32)> = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().unwrap_or_default(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                )
            })
            .collect();
        assert_eq!(pins, [("bad-syntax.d.ts".to_owned(), 1110, 17)]);
    }

    // ---- lib-loading L2: lib-backed programs (oracle-pinned) ----

    fn es5_lib() -> InputFile {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-6.0.3/lib/lib.es5.d.ts"
        );
        InputFile {
            name: "lib.es5.d.ts".to_owned(),
            text: std::fs::read_to_string(path).expect("vendored lib.es5.d.ts"),
        }
    }

    fn lib_backed_diags(text: &str) -> Vec<(u32, u32, u32, String)> {
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.ts".to_owned(),
                text: text.to_owned(),
            }],
            &CompilerOptions::default(),
        );
        result
            .diagnostics
            .iter()
            .map(|d| {
                (
                    d.code(),
                    d.start.unwrap_or(u32::MAX),
                    d.length.unwrap_or(u32::MAX),
                    d.message_text().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn lib_names_resolve_through_the_loaded_lib() {
        assert_eq!(
            lib_backed_diags(
                "interface I<T extends Date> { x: T }
"
            ),
            []
        );
    }

    #[test]
    fn restricted_lib_set_reports_2583_with_the_lib_argument() {
        // Map is not in es5: the failure is GENUINE under this lib set
        // (the lib_globals gate stands down for lib-loaded programs)
        // and the suggested-lib arm supplies tsc's exact argument.
        let diags = lib_backed_diags(
            "interface I<T extends Map> { x: T }
",
        );
        assert_eq!(
            diags,
            [(
                2583,
                22,
                3,
                "Cannot find name 'Map'. Do you need to change your target library? Try changing the 'lib' compiler option to 'es2015' or later."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn lib_array_members_drive_variance_measurement() {
        // Mutable method parameters are bivariant, so es5 Array
        // measures covariant and `out` holds (oracle-pinned clean)...
        assert_eq!(
            lib_backed_diags(
                "interface Wrap<out T> { xs: T[] }
"
            ),
            []
        );
        // ...including when a fixture declaration MERGES into the lib
        // interface (both member sets resolve; oracle-pinned clean).
        assert_eq!(
            lib_backed_diags(
                "interface Array<T> { fixtureExtra: T }
interface Wrap<out T> { xs: T[] }
"
            ),
            []
        );
        assert_eq!(
            lib_backed_diags(
                "interface Array<T> { sink: (x: T) => void }
interface Wrap<out T> { xs: T[] }
"
            ),
            []
        );
        assert_eq!(
            lib_backed_diags(
                "interface Wrap<out T> { xs: ReadonlyArray<T> }
"
            ),
            []
        );
    }

    #[test]
    fn lib_types_render_in_constraint_failure_args() {
        // Named object types print their symbol name in the 2344 args
        // (type_to_string_slice's named-object arm; oracle-pinned).
        let diags =
            lib_backed_diags("interface Foo<T extends number> { x: T }\ntype X = Foo<Date>;\n");
        assert_eq!(
            diags,
            [(
                2344,
                54,
                4,
                "Type 'Date' does not satisfy the constraint 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn lib_array_in_parameter_position_reports_2636() {
        let diags = lib_backed_diags(
            "interface Wrap<out T> { f: (xs: T[]) => void }
",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2636, 15, 5));
        assert!(
            diags[0]
                .3
                .starts_with("Type 'Wrap<sub-T>' is not assignable to type 'Wrap<super-T>'"),
            "{}",
            diags[0].3
        );
    }

    #[test]
    fn check_program_includes_parse_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "\"unterminated".to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code(), 1002);
    }

    /// Promise<T> is declared in BOTH es2015.promise and
    /// es2015.symbol.wellknown; the merged symbol must expose ONE T
    /// (getSymbolOfDeclaration's getMergedSymbol chase inside
    /// appendTypeParameters) — without the chase the declared type
    /// read `Promise<T, T>` and every `Promise<X>` reference tripped
    /// a spurious 2314 (lib-loading L2 find: the async-fixture FPs).
    #[test]
    fn merged_lib_interface_type_parameters_unify() {
        let vendor = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-6.0.3/lib/"
        );
        let lib = |name: &str| InputFile {
            name: name.to_owned(),
            text: std::fs::read_to_string(format!("{vendor}{name}")).expect("vendored lib"),
        };
        let result = check_program_with_libs(
            &[
                lib("lib.es5.d.ts"),
                lib("lib.es2015.promise.d.ts"),
                lib("lib.es2015.symbol.wellknown.d.ts"),
            ],
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "type X = Promise<number>;\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(result.diagnostics, []);
    }
}

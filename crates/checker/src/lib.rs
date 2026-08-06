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

use std::sync::Arc;

use tsc_diagnostics::{
    Diagnostic, DiagnosticCategory, DiagnosticList, DocumentVersion, TextSnapshot,
};

pub use tsc_types::CompilerOptions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputFile {
    pub name: String,
    snapshot: Arc<TextSnapshot>,
}

impl InputFile {
    /// tsrs-native: construct a one-shot L0 snapshot at the checker
    /// compatibility edge.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            snapshot: TextSnapshot::new(text.into(), DocumentVersion::default()),
        }
    }

    /// tsrs-native: retain the exact producer-owned L0 snapshot Arc at the
    /// checker compatibility edge.
    pub fn from_snapshot(name: impl Into<String>, snapshot: Arc<TextSnapshot>) -> Self {
        Self {
            name: name.into(),
            snapshot,
        }
    }

    /// tsrs-native: expose the shared L0 snapshot owner without its private
    /// store lineage.
    pub fn snapshot(&self) -> &Arc<TextSnapshot> {
        &self.snapshot
    }

    /// tsrs-native: borrow contiguous parser text from the shared L0 snapshot.
    pub fn text(&self) -> &str {
        self.snapshot.text()
    }
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
    /// The exact resolver-selected identity. This can differ from the target
    /// source's file name when createProgram redirects an equal package ID.
    pub resolved_file_name: String,
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

#[derive(Clone, Debug, Default)]
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
    /// Coarse document work performed by this invocation. The counters are
    /// updated only at parse/bind entry boundaries; they never add a branch
    /// to node, symbol, or type hot loops. Operational work is intentionally
    /// excluded from result equality; callers compare it explicitly through
    /// this field or its accessors.
    pub work_counters: CheckWorkCounters,
}

impl PartialEq for CheckResult {
    fn eq(&self, other: &Self) -> bool {
        self.diagnostics == other.diagnostics
            && self.syntactic_diagnostics == other.syntactic_diagnostics
            && self.semantic_diagnostics == other.semantic_diagnostics
            && self.global_diagnostics == other.global_diagnostics
            && self.suggestion_diagnostics == other.suggestion_diagnostics
            && self.file_diagnostics == other.file_diagnostics
            && self.partial_checks == other.partial_checks
    }
}

impl Eq for CheckResult {}

/// Parse/bind and full-text-copy observations for one checker invocation.
///
/// Text snapshots are shared across checker boundaries, so a fresh parse no
/// longer contributes a full-text projection. The copy counters remain in
/// the evidence schema as a zero-valued compatibility observation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckWorkCounters {
    parsed_documents: u64,
    bound_documents: u64,
    full_text_copies: u64,
    full_text_bytes_copied: u64,
}

impl CheckWorkCounters {
    /// tsrs-native: expose the L0 parse-work observation without changing the
    /// pinned checker algorithm.
    pub const fn parsed_documents(self) -> u64 {
        self.parsed_documents
    }

    /// tsrs-native: expose the L0 bind-work observation without changing the
    /// pinned binder algorithm.
    pub const fn bound_documents(self) -> u64 {
        self.bound_documents
    }

    /// tsrs-native: expose the Rust ownership projection count used by the L0
    /// resource contract.
    pub const fn full_text_copies(self) -> u64 {
        self.full_text_copies
    }

    /// tsrs-native: expose the Rust ownership projection bytes used by the L0
    /// resource contract.
    pub const fn full_text_bytes_copied(self) -> u64 {
        self.full_text_bytes_copied
    }

    fn record_parse(&mut self, text_bytes: usize) {
        self.parsed_documents += 1;
        let _ = text_bytes;
    }

    fn record_bind(&mut self) {
        self.bound_documents += 1;
    }

    fn for_fresh_inputs(inputs: &[&InputFile]) -> Self {
        Self {
            parsed_documents: inputs.len() as u64,
            bound_documents: inputs.len() as u64,
            full_text_copies: 0,
            full_text_bytes_copied: 0,
        }
    }
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
    positions: &tsc_diagnostics::PositionIndex,
    diagnostic_start: u32,
) -> Option<usize> {
    let diagnostic_line = positions.line_and_character_utf16(diagnostic_start)?.line as usize;
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
    let text = source.text();
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
            source.positions(),
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
    let text = source.text();
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
            .positions()
            .byte_to_utf16((start) as u32)
            .unwrap_or(start as u32);
        if let Some(line) = preceding_comment_directive_line(
            text,
            &byte_line_starts,
            &directive_lines,
            source.positions(),
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
    let byte_line_starts = compute_byte_line_starts(source.text());
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
                .positions()
                .byte_to_utf16((directive.pos as usize) as u32)
                .unwrap_or(directive.pos);
            let end = source
                .positions()
                .byte_to_utf16((directive.end as usize) as u32)
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
            CheckWorkCounters::for_fresh_inputs(&effective_libs),
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
        CheckWorkCounters::default(),
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
        CheckWorkCounters::for_fresh_inputs(&effective_libs),
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
            CheckWorkCounters::default(),
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
            CheckWorkCounters::for_fresh_inputs(&effective_libs),
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
    mut work_counters: CheckWorkCounters,
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
            let module_type = parse_host_package_json(file.text())
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
            let source_file = tsc_syntax::parse_json_text_from_snapshot_with_bases(
                file.name.clone(),
                Arc::clone(file.snapshot()),
                node_id_base,
                node_array_id_base,
            );
            work_counters.record_parse(file.text().len());
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
        let source_file = tsc_syntax::parse_source_file_from_snapshot(
            file.name.clone(),
            Arc::clone(file.snapshot()),
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
        work_counters.record_parse(file.text().len());
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
        .map(|source| (source.file_name.as_str(), check_directive(source.text())))
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
        work_counters.record_bind();
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
                let value = parse_host_package_json(file.text())?;
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
            work_counters,
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
            && self.sources.iter().zip(libs).all(|(source, lib)| {
                source.file_name == lib.name && Arc::ptr_eq(source.snapshot(), lib.snapshot())
            })
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
            .map(|lib| (lib.name.clone(), fingerprint(lib.text())))
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
        sources.push(tsc_syntax::parse_source_file_from_snapshot(
            lib.name.clone(),
            Arc::clone(lib.snapshot()),
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
#[path = "../tests/unit/lib/tests.rs"]
mod tests;

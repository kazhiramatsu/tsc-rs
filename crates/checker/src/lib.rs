#![forbid(unsafe_code)]

/// Join JavaScript values without converting through a scalar display string.
/// tsrs-native: JsString Array.join utility without scalar round-trip
pub(crate) fn join_js_strings<'a>(
    parts: impl IntoIterator<Item = tsc_types::JsStr<'a>>,
    separator: &str,
) -> tsc_types::JsString {
    let mut result = tsc_types::JsString::new();
    for (index, part) in parts.into_iter().enumerate() {
        if index != 0 {
            result.push_str(separator);
        }
        result.push_js(part);
    }
    result
}

/// Explicit interpolation for diagnostic/source text. Arbitrary JS values
/// append their code units; only numeric counters use scalar formatting.
pub(crate) trait JsTextPart {
    fn append_to(&self, result: &mut tsc_types::JsString);
}

impl<T: JsTextPart + ?Sized> JsTextPart for &T {
    fn append_to(&self, result: &mut tsc_types::JsString) {
        (*self).append_to(result);
    }
}
impl JsTextPart for str {
    fn append_to(&self, result: &mut tsc_types::JsString) {
        result.push_str(self);
    }
}
impl JsTextPart for String {
    fn append_to(&self, result: &mut tsc_types::JsString) {
        result.push_str(self);
    }
}
impl JsTextPart for tsc_types::JsString {
    fn append_to(&self, result: &mut tsc_types::JsString) {
        result.push_js(self.as_js());
    }
}
impl JsTextPart for tsc_types::JsStr<'_> {
    fn append_to(&self, result: &mut tsc_types::JsString) {
        result.push_js(*self);
    }
}
impl JsTextPart for char {
    fn append_to(&self, result: &mut tsc_types::JsString) {
        result.push(*self);
    }
}
macro_rules! numeric_text_part {
    ($($number:ty),* $(,)?) => { $(
        impl JsTextPart for $number {
            fn append_to(&self, result: &mut tsc_types::JsString) {
                result.push_str(&self.to_string());
            }
        }
    )* };
}
numeric_text_part!(usize, u32, i32, u64, i64, f64);

/// tsrs-native: JsString concatenation utility (JS + on strings)
pub(crate) fn concat_js(parts: &[&dyn JsTextPart]) -> tsc_types::JsString {
    let mut result = tsc_types::JsString::new();
    for part in parts {
        part.append_to(&mut result);
    }
    result
}

/// tsrs-native: JsTextPart Array.join utility
pub(crate) fn join_js_texts<T: JsTextPart>(parts: &[T], separator: &str) -> tsc_types::JsString {
    let mut result = tsc_types::JsString::new();
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            result.push_str(separator);
        }
        part.append_to(&mut result);
    }
    result
}

pub mod access;
pub mod annotate;
pub mod calls;
pub mod check;
pub mod class;
pub mod conditional;
pub mod constraints;
pub mod contextual;
mod declaration_emit;
mod diagnostic_sink;
mod display_clone;
mod display_clone_body;
mod display_clone_module;
pub mod elaboration;
pub mod emit;
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
pub mod line_profile;
pub mod links;
pub mod literals;
pub mod mapped;
pub mod member_table;
pub mod merge;
pub mod modules;
pub mod narrow;
mod node_builder;
pub mod operators;
pub(crate) mod order_guard;
mod plain_js_errors;
pub mod program;
mod pseudochecker;
pub mod relate;
#[doc(hidden)]
pub mod relpin;
pub mod resolve;
pub mod shard;
pub mod speculate;
pub mod spell;
pub mod state;
pub mod statements;
pub mod structural;
mod syntactic_type_node_builder;
pub(crate) mod type_order;
pub mod unions;
mod unused;
pub mod variance;
pub mod widen;

use std::sync::Arc;

use tsc_binder::BindData;
use tsc_diagnostics::{
    Diagnostic, DiagnosticCategory, DiagnosticList, DocumentVersion, TextSnapshot,
};
use tsc_program::WorkerBudget;

pub use crate::shard::{order_replay_requested, CheckerBudget, MAX_CHECKERS};

/// Whether checkSourceFile runs the unused-identifier pass when neither
/// `noUnusedLocals` nor `noUnusedParameters` turns its rows into errors.
/// tsc computes those rows only for getSuggestionDiagnostics, which the
/// command line never requests; the API keeps them on for the harnesses that
/// compare suggestion rows.
static UNUSED_IDENTIFIER_SUGGESTIONS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// Process-wide switch for the unused-identifier suggestion pass (see
/// [`unused_identifier_suggestions`]). A command-line process sets it once
/// before checking; library consumers leave it on.
pub fn set_unused_identifier_suggestions(enabled: bool) {
    UNUSED_IDENTIFIER_SUGGESTIONS.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn unused_identifier_suggestions() -> bool {
    UNUSED_IDENTIFIER_SUGGESTIONS.load(std::sync::atomic::Ordering::Relaxed)
}
use tsc_types::perf::{self, PerfCounter};
use tsc_types::{IdentityDomain, IdentityLease, JsStr, JsString};

use crate::emit::CheckerSession;

pub use crate::program::{
    BoundDocument, DocumentAddress, DocumentLease, DocumentRegistry, DocumentRegistryError,
    DocumentScriptKind, EphemeralDocumentStore, EphemeralDocumentStoreError,
    IncrementalDocumentOptions, IncrementalDocumentUpdate, ParsedDocument, ProgramFileFacts,
    ProgramFileId, ProgramSnapshot,
};

pub use tsc_syntax::JSDocParsingMode;
pub use tsc_types::CompilerOptions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputFile {
    pub name: tsc_types::JsString,
    snapshot: Arc<TextSnapshot>,
    host_only: bool,
    /// API-supplied `SourceFile.moduleName` (transpileWorker,
    /// typescript.js:146100); overrides the parsed `amd-module` pragma.
    module_name: Option<tsc_types::JsString>,
    /// API-supplied `SourceFile.renamedDependencies` (typescript.js:146101).
    renamed_dependencies: Vec<(tsc_types::JsString, tsc_types::JsString)>,
    /// Per-file createSourceFile `jsDocParsingMode`; None keeps the
    /// Program's ParseAll default.
    js_doc_parsing_mode: Option<JSDocParsingMode>,
    /// The Program loader's request-planning parse of this exact snapshot
    /// (an empty slot for API-built inputs). The checker adopts it only after
    /// proving it would have parsed with identical options; otherwise it
    /// parses as before. The slot compares equal in every state, so it never
    /// participates in `InputFile` equality.
    preparsed_syntax: tsc_program::PreparsedSyntax,
}

impl InputFile {
    /// tsrs-native: construct a one-shot L0 snapshot at the checker
    /// compatibility edge.
    pub fn new(name: impl Into<tsc_types::JsString>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            snapshot: TextSnapshot::new(text.into(), DocumentVersion::default()),
            host_only: false,
            module_name: None,
            renamed_dependencies: Vec::new(),
            js_doc_parsing_mode: None,
            preparsed_syntax: tsc_program::PreparsedSyntax::empty(),
        }
    }

    /// tsrs-native: retain the exact producer-owned L0 snapshot Arc at the
    /// checker compatibility edge.
    pub fn from_snapshot(
        name: impl Into<tsc_types::JsString>,
        snapshot: Arc<TextSnapshot>,
    ) -> Self {
        Self {
            name: name.into(),
            snapshot,
            host_only: false,
            module_name: None,
            renamed_dependencies: Vec::new(),
            js_doc_parsing_mode: None,
            preparsed_syntax: tsc_program::PreparsedSyntax::empty(),
        }
    }

    /// tsrs-native: offer the loader's planning parse of this snapshot for
    /// adoption. Only the program crate's planner produces a filled slot, and
    /// adoption re-verifies snapshot identity, file name and parse options.
    pub fn with_preparsed_syntax(mut self, syntax: tsc_program::PreparsedSyntax) -> Self {
        self.preparsed_syntax = syntax;
        self
    }

    /// tsc `sourceFile.moduleName = transpileOptions.moduleName`
    /// (typescript.js:146099-146101).
    /// tsrs-native: builder setter for transpile moduleName (typescript.js:146099-146101)
    pub fn with_module_name(mut self, module_name: Option<tsc_types::JsString>) -> Self {
        self.module_name = module_name;
        self
    }

    /// tsc `sourceFile.renamedDependencies = new Map(...)`
    /// (typescript.js:146102-146104).
    /// tsrs-native: builder setter for renamedDependencies (typescript.js:146102-146104)
    pub fn with_renamed_dependencies(
        mut self,
        renamed_dependencies: Vec<(tsc_types::JsString, tsc_types::JsString)>,
    ) -> Self {
        self.renamed_dependencies = renamed_dependencies;
        self
    }

    /// tsc createSourceFile `jsDocParsingMode` for this file only
    /// (typescript.js:146097).
    /// tsrs-native: builder setter for transpile jsDocParsingMode (typescript.js:146097 as
    /// cited)
    pub fn with_js_doc_parsing_mode(mut self, mode: Option<JSDocParsingMode>) -> Self {
        self.js_doc_parsing_mode = mode;
        self
    }

    /// tsrs-native: retain a host-readable input that is not a source in the
    /// checker Program. Prepared package manifests use this path so package
    /// metadata remains available to module-specifier generation without
    /// shadowing an imported JSON SourceFile at the same path.
    pub fn host_only_from_snapshot(
        name: impl Into<tsc_types::JsString>,
        snapshot: Arc<TextSnapshot>,
    ) -> Self {
        Self {
            name: name.into(),
            snapshot,
            host_only: true,
            module_name: None,
            renamed_dependencies: Vec::new(),
            js_doc_parsing_mode: None,
            preparsed_syntax: tsc_program::PreparsedSyntax::empty(),
        }
    }

    /// Adopt the loader's parse of this input when it is provably the parse
    /// the checker would perform: same snapshot identity, same file name and
    /// the same [`tsc_syntax::ParseOptions`] (identity bases excluded). The
    /// adopted tree is relocated into `identity_domain` exactly as a fresh
    /// base-0 parse would be. Any other case returns `None` and the caller
    /// parses.
    /// The loader's parse of this exact snapshot at local identities, when
    /// it is provably the parse this session would perform. The caller
    /// leases its identities in program order and relocates it.
    fn take_preparsed_source(
        &self,
        options: &tsc_syntax::ParseOptions,
    ) -> Option<tsc_syntax::SourceFile> {
        let preparsed = self.preparsed_syntax.take()?;
        let expected = tsc_syntax::ParseOptions {
            node_id_base: 0,
            node_array_id_base: 0,
            ..options.clone()
        };
        if *preparsed.parse_options() != expected
            || preparsed.source().file_name != self.name
            || !Arc::ptr_eq(preparsed.source().snapshot(), &self.snapshot)
        {
            return None;
        }
        Some(preparsed.into_source())
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
    pub file_name: JsString,
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
    pub containing_file: JsStr<'a>,
    pub specifier: JsStr<'a>,
    pub mode: AuthoritativeResolutionMode,
}

/// Package identity attached by the authoritative resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativePackageId {
    pub name: JsString,
    pub submodule_name: JsString,
    pub version: JsString,
    pub peer_dependencies: Option<JsString>,
}

/// A loaded source selected by the authoritative host table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeResolvedModule {
    pub target_token: AuthoritativeSourceToken,
    /// The exact resolver-selected identity. This can differ from the target
    /// source's file name when createProgram redirects an equal package ID.
    pub resolved_file_name: JsString,
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
    pub alternate_result: Option<JsString>,
    pub types_package_exists: bool,
    pub package_bundles_types: bool,
}

/// A successfully resolved untyped implementation that was deliberately not
/// loaded into the source program, together with the exact TS7016 facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeUntypedModule {
    pub resolved_file_name: JsString,
    pub package_name: Option<JsString>,
    pub alternate_result: Option<JsString>,
    pub types_package_exists: bool,
    pub package_bundles_types: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoritativeModuleResolutionDiagnostic {
    JsxWithoutJsxOption,
    ArbitraryExtensionWithoutOption,
}

/// A successful resolution kept only for its resolution diagnostic. The
/// target is intentionally absent from source membership, so this cannot be
/// represented as either a loaded module or an untyped implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeResolutionDiagnosticModule {
    pub resolved_file_name: JsString,
    pub diagnostic: AuthoritativeModuleResolutionDiagnostic,
}

/// An unsuccessful authoritative lookup together with host facts that remain
/// observable in the module-not-found diagnostic chain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoritativeNotFoundModule {
    pub alternate_result: Option<JsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthoritativeModuleResolution {
    Resolved(AuthoritativeResolvedModule),
    Untyped(AuthoritativeUntypedModule),
    ResolutionDiagnostic(AuthoritativeResolutionDiagnosticModule),
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
pub trait AuthoritativeModuleProvider: Sync {
    fn resolve_module(
        &self,
        request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure>;

    /// Immutable Program-owned fields from TypeScript's effective compiler
    /// options used when synthesizing a declaration module specifier. These
    /// accompany the separate `CompilerOptions` bag without reparsing config
    /// or losing the declaring directory of inherited paths.
    /// Legacy providers without a Program option bag retain the absent view.
    fn program_options_for_module_specifiers(&self) -> Option<&tsc_program::ProgramOptions> {
        None
    }

    /// tsgo `GetIncludeProcessorDiagnostics` before its per-file filter
    /// (compiler/program.go:840-846): the Program's rows located in one of
    /// its source files (unresolved references, file-include explanations,
    /// resolution diagnostics), each with the token of that source. A row
    /// joins its file's semantic diagnostics unless the file skips type
    /// checking or a comment directive precedes the row. Providers without
    /// Program rows report none.
    fn include_processor_diagnostics(&self) -> Vec<(AuthoritativeSourceToken, Diagnostic)> {
        Vec::new()
    }
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
        containing_file: JsString,
        specifier: JsString,
        mode: AuthoritativeResolutionMode,
        failure: AuthoritativeModuleLookupFailure,
    },
    UnknownSourceToken {
        file_index: usize,
        containing_file: JsString,
    },
    UnknownTargetToken {
        source_token: AuthoritativeSourceToken,
        containing_file: JsString,
        specifier: JsString,
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
                "authoritative module lookup failed for ({}, {specifier:?}, {mode:?}): {failure:?}", containing_file.to_string_lossy()
            ),
            Self::UnknownSourceToken {
                file_index,
                containing_file,
            } => write!(
                formatter,
                "authoritative checker file {file_index} ({}) has no source token", containing_file.to_string_lossy()
            ),
            Self::UnknownTargetToken {
                containing_file,
                specifier,
                mode,
                target_token,
                ..
            } => write!(
                formatter,
                "authoritative module lookup for ({}, {specifier:?}, {mode:?}) selected unavailable source token {}",
                containing_file.to_string_lossy(), target_token.0
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
    /// `program.getSemanticDiagnostics(undefined)` for production Program
    /// sessions. This is distinct from [`Self::semantic_diagnostics`]: the
    /// source-file getter surface intentionally does not query default
    /// libraries, while the whole-Program getter does (subject to
    /// `skipDefaultLibCheck`, `skipLibCheck`, and the remaining
    /// `skipTypeCheckingWorker` policy).
    ///
    /// Legacy fixture-only adapters leave this as `None`; authoritative
    /// compiler sessions always publish `Some`, including for an empty
    /// diagnostic list. Keeping availability explicit prevents a compiler
    /// consumer from accidentally falling back to the fixture projection.
    pub program_semantic_diagnostics: Option<DiagnosticList>,
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
            && self.program_semantic_diagnostics == other.program_semantic_diagnostics
            && self.global_diagnostics == other.global_diagnostics
            && self.suggestion_diagnostics == other.suggestion_diagnostics
            && self.file_diagnostics == other.file_diagnostics
            && self.partial_checks == other.partial_checks
    }
}

impl Eq for CheckResult {}

/// Parse/bind and full-text-copy observations for one checker invocation.
///
/// `parsed_documents` counts documents this invocation parsed itself (the L0
/// parse-work observation). `adopted_documents` counts documents whose
/// syntax tree this invocation took over from the Program loader's
/// request-planning parse ([`tsc_program::PreparsedSyntax`]) and relocated
/// into its identity domain instead of parsing; the loader's parse happened
/// outside this invocation and is not counted here. Every materialized
/// document is either parsed or adopted, so
/// `parsed_documents + adopted_documents == bound_documents` for an owned
/// library prefix plus program files; a cached library prefix contributes to
/// none of the three. A repeated session over the same prepared program, or
/// an input whose parse options differ from the planner's, parses again and
/// therefore reports more parse work: the counters describe work performed,
/// not a fixed expectation.
///
/// Text snapshots are shared across checker boundaries, so a fresh parse no
/// longer contributes a full-text projection. The copy counters remain in
/// the evidence schema as a zero-valued compatibility observation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckWorkCounters {
    parsed_documents: u64,
    adopted_documents: u64,
    bound_documents: u64,
    full_text_copies: u64,
    full_text_bytes_copied: u64,
    checker_shards: u64,
    checker_threads: u64,
    checker_serial_replay: u64,
    checker_replay_reasons: u64,
}

/// Parse work performed while materializing an owned library prefix.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LibParseWork {
    parsed: u64,
    adopted: u64,
}

impl CheckWorkCounters {
    /// tsrs-native: expose the L0 parse-work observation without changing the
    /// pinned checker algorithm.
    pub const fn parsed_documents(self) -> u64 {
        self.parsed_documents
    }

    /// tsrs-native: documents whose loader-planned syntax tree this
    /// invocation adopted (relocated) instead of parsing.
    pub const fn adopted_documents(self) -> u64 {
        self.adopted_documents
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

    /// tsrs-native: checker states constructed by this invocation (one for
    /// the serial driver, the effective shard count for the sharded driver).
    /// Zero means no checker state was constructed (an empty Program).
    pub const fn checker_shards(self) -> u64 {
        self.checker_shards
    }

    /// tsrs-native: distinct threads that ran those checker states; equal
    /// to `checker_shards` when every shard got its own thread, smaller when
    /// the coordinator ran a shard after a refused spawn.
    pub const fn checker_threads(self) -> u64 {
        self.checker_threads
    }

    fn record_parse(&mut self, text_bytes: usize) {
        self.parsed_documents += 1;
        let _ = text_bytes;
    }

    fn record_checker_shards(&mut self, shards: u64, threads: u64) {
        self.checker_shards = shards;
        self.checker_threads = threads;
    }

    /// tsrs-native: 1 when a sharded check was discarded and replayed by
    /// the serial checker because the order-sensitivity guard fired
    /// (slice W2c); 0 otherwise.
    pub const fn checker_serial_replay(self) -> u64 {
        self.checker_serial_replay
    }

    /// tsrs-native: the union of the guard's reason bits over all shards
    /// (see `order_guard::OrderReason`); 0 when nothing fired.
    pub const fn checker_replay_reasons(self) -> u64 {
        self.checker_replay_reasons
    }

    fn record_serial_replay(&mut self, reasons: u32) {
        self.checker_serial_replay = 1;
        self.checker_replay_reasons = u64::from(reasons);
    }

    /// Order-guard reasons a merged (not replayed) sharded run recorded.
    fn record_order_reasons(&mut self, reasons: u32) {
        self.checker_replay_reasons = u64::from(reasons);
    }

    fn record_adoption(&mut self) {
        self.adopted_documents += 1;
    }

    fn record_bind(&mut self) {
        self.bound_documents += 1;
    }

    /// Counters for an owned (uncached) library prefix, from the parse work
    /// actually performed by `parse_lib_sources`; every owned library is
    /// bound by this invocation.
    fn for_owned_libs(work: LibParseWork) -> Self {
        Self {
            parsed_documents: work.parsed,
            adopted_documents: work.adopted,
            bound_documents: work.parsed + work.adopted,
            full_text_copies: 0,
            full_text_bytes_copied: 0,
            checker_shards: 0,
            checker_threads: 0,
            checker_serial_replay: 0,
            checker_replay_reasons: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileDiagnosticPasses {
    pub file_name: tsc_types::JsString,
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
    pub file_name: tsc_types::JsString,
    /// UTF-16 offset, matching diagnostic and oracle coordinates.
    pub start: u32,
    pub length: u32,
    pub reason: String,
}

/// tsc getSupportedExtensions: JS roots only join the program with allowJs.
fn is_supported_source_file_name<'n>(name: impl Into<JsStr<'n>>, allow_js: bool) -> bool {
    let name = name.into();
    let ts_like = [".ts", ".tsx", ".mts", ".cts", ".json"];
    ts_like.iter().any(|extension| name.ends_with(extension)) || (allow_js && is_js_file_name(name))
}

/// tsc-port: hasJSFileExtension @6.0.3
/// tsc-hash: 26f2de10186fd7377e0fc90d254165421f27320a1b95dca68e43ee8f2f71128d
/// tsc-span: _tsc.js:18654-18656
pub(crate) fn is_js_file_name<'n>(name: impl Into<JsStr<'n>>) -> bool {
    let name = name.into();
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
pub(crate) enum CheckDirective {
    Check,
    NoCheck,
}

/// tsrs-native: lexical projection of leading-comment @ts-check/@ts-nocheck
/// (processCommentPragmas 36215 + processPragmasIntoFields checkJsDirective ~36288, last
/// wins); not a structural port
pub(crate) fn check_directive(text: &str) -> Option<CheckDirective> {
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

/// tsc-port: canIncludeBindAndCheckDiagnostics @6.0.3
/// tsc-hash: e833101f7d0b7e59d1247180868406c7e65ac869387a07face9965d430e98204
/// tsc-span: _tsc.js:18898-18905
pub(crate) fn can_include_bind_and_check_diagnostics(
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

/// tsc-port: skipTypeCheckingWorker @6.0.3
/// tsc-hash: 1c3be6d0ff15f3752237bd0bd2d0ee0543b4cfd10c150652d0c6e94b2890f103
/// tsc-span: _tsc.js:18895-18897
///
/// One Program-aware implementation of tsc's `skipTypeCheckingWorker` policy.
///
/// Default-library status is supplied by the Program builder rather than
/// inferred from a path or stored on a reusable document. Project-reference
/// redirects are not yet represented by this Program model; when they are,
/// they belong in [`ProgramFileFacts`] beside the default-library bit.
pub(crate) fn should_skip_type_checking_file(
    source: &tsc_syntax::SourceFile,
    facts: ProgramFileFacts,
    options: &CompilerOptions,
) -> bool {
    options.skip_lib_check == Some(true) && source.is_declaration_file
        || options.skip_default_lib_check == Some(true) && facts.is_default_library()
        || options.no_check == Some(true)
        // tsgo canIncludeBindAndCheckDiagnostics (compiler/program.go:856-873)
        // checks TypeScript, plain JS and checked JS files only: a JSON
        // source file is never checked.
        || source.file_name.as_js().ends_with(".json")
        || !can_include_bind_and_check_diagnostics(
            is_js_file_name(&source.file_name),
            check_directive(source.text()),
            options,
        )
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
    directive_lines: &rustc_hash::FxHashSet<usize>,
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
        let start = positions.line_start_byte(line as u32)? as usize;
        let end = positions
            .line_start_byte(line as u32 + 1)
            .map_or(text.len(), |end| end as usize);
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
    mut used_directive_lines: Option<&mut rustc_hash::FxHashSet<usize>>,
) -> Vec<tsc_diagnostics::Diagnostic> {
    // getMergedBindAndCheckDiagnostics (123744): no directives, no
    // filtering.
    if source.comment_directives.is_empty() {
        return diagnostics.collect();
    }
    let text = source.text();
    let directive_lines = comment_directive_lines(source);
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
        if let Some(line) =
            preceding_comment_directive_line(text, &directive_lines, source.positions(), start)
        {
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
    used_directive_lines: &mut rustc_hash::FxHashSet<usize>,
) {
    if source.comment_directives.is_empty() || partial_ranges.is_empty() {
        return;
    }
    let text = source.text();
    let directive_lines = comment_directive_lines(source);
    for &(start, _) in partial_ranges {
        let start = tsc_syntax::skip_trivia(text, start as usize);
        let start_utf16 = source
            .positions()
            .byte_to_utf16((start) as u32)
            .unwrap_or(start as u32);
        if let Some(line) = preceding_comment_directive_line(
            text,
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
    used_directive_lines: &rustc_hash::FxHashSet<usize>,
) -> Vec<tsc_diagnostics::Diagnostic> {
    use tsc_syntax::CommentDirectiveKind;

    if source.comment_directives.is_empty() {
        return Vec::new();
    }
    // createCommentDirectivesMap uses Map construction, so the last
    // directive ending on a line replaces earlier directives there.
    let mut directives_by_line = std::collections::BTreeMap::new();
    for directive in &source.comment_directives {
        directives_by_line.insert(comment_directive_line(source, directive), *directive);
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
            Some(tsc_diagnostics::Diagnostic::new_js(
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

/// The line a comment directive ends on (its line in tsc's
/// createCommentDirectivesMap), from the source's own line starts.
fn comment_directive_line(
    source: &tsc_syntax::SourceFile,
    directive: &tsc_syntax::CommentDirective,
) -> usize {
    source
        .positions()
        .line_of_byte(directive.end)
        .expect("a comment directive ends inside its source") as usize
}

fn comment_directive_lines(source: &tsc_syntax::SourceFile) -> rustc_hash::FxHashSet<usize> {
    source
        .comment_directives
        .iter()
        .map(|directive| comment_directive_line(source, directive))
        .collect()
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
fn resolve_host_current_directory<'cwd>(current_directory: impl Into<JsStr<'cwd>>) -> JsString {
    let current_directory = current_directory.into();
    let raw_path = if current_directory.starts_with("/") {
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
        concat_js(&[&process_cwd, &"/", &current_directory])
    };

    let absolute = raw_path.starts_with("/");
    let mut segments: Vec<JsStr<'_>> = Vec::new();
    for segment in raw_path.as_js().split_ascii(b'/') {
        match segment.as_str() {
            Some("" | ".") => {}
            Some("..") => {
                if segments.last().is_some_and(|last| *last != "..") {
                    segments.pop();
                } else if !absolute {
                    segments.push(segment);
                }
            }
            _ => segments.push(segment),
        }
    }
    let normalized = join_js_texts(&segments, "/");
    let resolved = if absolute {
        concat_js(&[&"/", &normalized])
    } else if normalized.is_empty() {
        JsString::from(".")
    } else {
        normalized
    };
    node_builder::specifier::normalized_slashes(&resolved)
}

fn is_supported_path_reference<'n>(
    file_name: impl Into<JsStr<'n>>,
    options: &CompilerOptions,
) -> bool {
    let file_name = file_name.into();
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
fn missing_path_reference_diagnostics<'cwd, 'a>(
    sources: impl IntoIterator<Item = &'a tsc_syntax::SourceFile>,
    host_files: impl Iterator<Item = JsString>,
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
) -> DiagnosticList {
    let current_directory = current_directory.into();
    // tsc's processReferencedFiles/processTypeReferenceDirectives gate is
    // outside getSourceFileFromReferenceWorker. Keep this bridge on the same
    // side of that noResolve boundary.
    if options.no_resolve == Some(true) {
        return Vec::new();
    }
    let known_paths: rustc_hash::FxHashSet<JsString> = host_files.collect();
    let mut diagnostics = Vec::new();
    for source in sources {
        let source_path =
            state::CheckerState::normalize_program_path(&source.file_name, current_directory);
        let source_directory = source_path
            .as_js()
            .rsplit_once("/")
            .map_or("/".into(), |(directory, _)| directory);
        for reference in &source.referenced_files {
            if !is_supported_path_reference(&reference.file_name, options) {
                continue;
            }
            let resolved =
                state::CheckerState::normalize_program_path(&reference.file_name, source_directory);
            if known_paths.contains(&resolved) {
                continue;
            }
            // The reference as written, like the program loader's row
            // (tsgo's `diagnosticFileName`, compiler/fileloader.go:697).
            diagnostics.push(Diagnostic::new_js(
                Some(source.file_name.clone()),
                Some(reference.pos),
                Some(reference.end.saturating_sub(reference.pos)),
                tsc_diagnostics::MessageChain::new_js(
                    &tsc_diagnostics::gen::File_0_not_found,
                    &[node_builder::specifier::normalized_slashes(
                        &reference.file_name,
                    )],
                ),
            ));
        }
    }
    diagnostics
}

fn parse_host_package_json(file: &InputFile) -> tsc_program::JsonValue {
    tsc_program::JsonValue::Object(tsc_program::read_package_json_object_from_snapshot(
        file.name.as_js(),
        file.snapshot(),
    ))
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
pub fn check_program_with_libs_at<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
) -> CheckResult {
    let current_directory = current_directory.into();
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

/// Prepare an owned authoritative library prefix for a bounded harness scope.
///
/// The fixture-name filter mirrors `createProgram`: a root that shadows a
/// library name removes that library from the effective prefix. Callers may
/// reuse the returned bundle for isolated sessions over the same inputs; it is
/// intentionally not inserted into the process-lifetime cache.
/// tsrs-native: caller-owned harness library-prefix preparation seam.
#[doc(hidden)]
pub fn prepare_authoritative_harness_lib_bundle(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
) -> Option<OwnedHarnessLibBundle> {
    if std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_some_and(|value| value == "0") {
        return None;
    }
    let fixture_names: rustc_hash::FxHashSet<JsStr<'_>> = files
        .iter()
        .filter(|file| !file.host_only)
        .map(|file| file.name.as_js())
        .collect();
    let effective_libs = libs
        .iter()
        .filter(|lib| !fixture_names.contains(&lib.name.as_js()))
        .collect::<Vec<_>>();
    (!effective_libs.is_empty()).then(|| build_owned_lib_bundle(&effective_libs, options))
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
pub fn check_program_with_prepared_harness_libs_at<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    prepared: PreparedHarnessLibBundle,
) -> CheckResult {
    let current_directory = current_directory.into();
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
pub fn check_program_with_libs_at_observed<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    mut observe_phase: impl FnMut(CheckPhase),
) -> CheckResult {
    let current_directory = current_directory.into();
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

fn check_program_with_libs_at_observed_cache_mode<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    cache_enabled: bool,
    observe_phase: &mut impl FnMut(CheckPhase),
) -> CheckResult {
    let current_directory = current_directory.into();
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

fn check_program_with_libs_at_observed_cache_mode_prepared<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    cache_enabled: bool,
    prepared: Option<PreparedHarnessLibBundle>,
    observe_phase: &mut impl FnMut(CheckPhase),
) -> CheckResult {
    let current_directory = current_directory.into();
    observe_phase(CheckPhase::Parse);

    let fixture_names: rustc_hash::FxHashSet<JsStr<'_>> = files
        .iter()
        .filter(|file| !file.host_only)
        .map(|file| file.name.as_js())
        .collect();
    let effective_libs: Vec<&InputFile> = libs
        .iter()
        .filter(|lib| !fixture_names.contains(&lib.name.as_js()))
        .collect();

    if !effective_libs.is_empty() && !cache_enabled {
        // Cache-off is the L3 A/B path. Keep the parsed and bound prefix local
        // so repeated disabled-cache calls do not leak one bundle each.
        let bundle_options = lib_bundle_options(options);
        let identity_domain = IdentityDomain::ephemeral();
        let (lib_sources, lib_work) = parse_lib_sources(
            &effective_libs,
            &bundle_options,
            &identity_domain,
            WorkerBudget::serial(),
        );
        let lib_binders = bind_lib_sources(
            &lib_sources,
            &bundle_options,
            &identity_domain,
            WorkerBudget::serial(),
        );
        let lib_data = binders_into_data(lib_binders);
        let lib_documents = publish_bound_documents(lib_sources, lib_data);
        return check_program_with_prebound_libs_at_observed(
            libs,
            files,
            options,
            current_directory,
            &lib_documents,
            &identity_domain,
            CheckWorkCounters::for_owned_libs(lib_work),
            false,
            observe_phase,
            None,
            None,
            ProgramFileFacts::ORDINARY,
            WorkerBudget::serial(),
        )
        .result;
    }

    let bundle = (!effective_libs.is_empty()).then(|| {
        let bundle_options = lib_bundle_options(options);
        prepared
            .and_then(|prepared| prepared.validated(&effective_libs, &bundle_options))
            .unwrap_or_else(|| lib_bundle(&effective_libs, options))
    });
    let lib_documents: &[Arc<BoundDocument>] = match bundle {
        Some(bundle) => bundle.documents,
        None => &[],
    };
    let identity_domain = bundle
        .map(|bundle| bundle.identity_domain.clone())
        .unwrap_or_else(IdentityDomain::ephemeral);

    check_program_with_prebound_libs_at_observed(
        libs,
        files,
        options,
        current_directory,
        lib_documents,
        &identity_domain,
        CheckWorkCounters::default(),
        false,
        observe_phase,
        None,
        None,
        ProgramFileFacts::ORDINARY,
        WorkerBudget::serial(),
    )
    .result
}

/// tsrs-native: run one owned-lib batch for the no-emit program session.
///
/// Execute one owned batch program without entering the process-lifetime lib
/// bundle cache. Library sources, binders, and all checker borrows are local
/// to this call and are dropped before it returns.
pub fn check_program_with_owned_libs_at<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
) -> CheckResult {
    let current_directory = current_directory.into();
    let fixture_names: rustc_hash::FxHashSet<JsStr<'_>> = files
        .iter()
        .filter(|file| !file.host_only)
        .map(|file| file.name.as_js())
        .collect();
    let effective_libs: Vec<&InputFile> = libs
        .iter()
        .filter(|lib| !fixture_names.contains(&lib.name.as_js()))
        .collect();
    let bundle_options = lib_bundle_options(options);
    let identity_domain = IdentityDomain::ephemeral();
    let (lib_sources, lib_work) = parse_lib_sources(
        &effective_libs,
        &bundle_options,
        &identity_domain,
        WorkerBudget::serial(),
    );
    let lib_binders = bind_lib_sources(
        &lib_sources,
        &bundle_options,
        &identity_domain,
        WorkerBudget::serial(),
    );
    let lib_data = binders_into_data(lib_binders);
    let lib_documents = publish_bound_documents(lib_sources, lib_data);
    let mut observe_phase = |_| {};

    check_program_with_prebound_libs_at_observed(
        libs,
        files,
        options,
        current_directory,
        &lib_documents,
        &identity_domain,
        CheckWorkCounters::for_owned_libs(lib_work),
        true,
        &mut observe_phase,
        None,
        None,
        ProgramFileFacts::DEFAULT_LIBRARY,
        WorkerBudget::serial(),
    )
    .result
}

/// How an authoritative session completes the whole-Program semantic view
/// (`program.getSemanticDiagnostics(undefined)`).
///
/// tsc checks every Program source the skip predicate admits — the default
/// library prefix included — before that getter returns. `Complete` is that
/// production CLI surface. `FixtureObservedOnly` assembles the same
/// whole-Program view purely from state already observed while checking the
/// fixture sources: no library-prefix check pass runs, so library-owned rows
/// that only a library check would publish stay absent from the assembled
/// view. It exists for harness sessions whose consumers read only the
/// per-file getter projections (the conformance runner compares
/// `conformance_diagnostics`/`syntactic_diagnostics`, which are assembled
/// before this pass and therefore cannot observe it); any consumer of the
/// whole-Program surface itself must use `Complete`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryPrefixCompletion {
    Complete,
    FixtureObservedOnly,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DiagnosticSchedule {
    Eager,
    OnDemand,
    /// The eager schedule after one call of the scoped operation over the
    /// initialized checker, before any source is checked: the order of the
    /// native compiler runner's second Program, which emits and then asks
    /// for the diagnostics.
    EagerAfterEmit,
}

/// Constructs one [`AuthoritativeModuleProvider`] per checker state.
///
/// Sharded checking runs several checker states on scoped threads; a provider
/// may keep small mutable caches (the compiler's request plans live in a
/// `RefCell`), so instead of requiring `Sync` on the provider trait each
/// shard constructs its own provider through this factory inside its thread.
/// The factory itself is shared read-only across shards, hence `Sync`.
/// tsrs-native: per-checker construction seam for the sharded driver.
pub trait AuthoritativeModuleProviderFactory: Sync {
    fn provider(&self) -> Box<dyn AuthoritativeModuleProvider + '_>;
}

/// Where a checker run obtains its module provider.
#[derive(Clone, Copy)]
enum AuthoritativeProviderSource<'a> {
    /// One caller-owned provider used by the single serial checker state.
    Shared(&'a dyn AuthoritativeModuleProvider),
    /// One provider constructed per checker state (serial or sharded).
    PerChecker(&'a dyn AuthoritativeModuleProviderFactory),
}

struct AuthoritativeRun<'a> {
    provider: AuthoritativeProviderSource<'a>,
    lib_metadata: Vec<AuthoritativeSourceMetadata>,
    file_metadata: Vec<AuthoritativeSourceMetadata>,
    library_prefix: LibraryPrefixCompletion,
    diagnostic_schedule: DiagnosticSchedule,
}

struct CheckExecution {
    result: CheckResult,
    authoritative_failure: Option<AuthoritativeModuleFailure>,
}

type CheckedEmitOperation<'operation> =
    dyn FnMut(&ProgramSnapshot, &CheckerSession<'_>, &CheckResult) + 'operation;

/// tsrs-native: run one owned checker batch whose module lookups are supplied exclusively
/// by an exact caller-owned table. The legacy in-memory resolver is never a
/// fallback while `provider` is installed.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    check_program_with_authoritative_modules_at_with_workers(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        WorkerBudget::serial(),
    )
}

/// [`check_program_with_authoritative_modules_at`] with an explicit
/// [`WorkerBudget`] for the scoped per-file binding step. Every budget
/// publishes the same identities and diagnostics; the serial entry above is
/// the default and the reproducible control.
/// tsrs-native: worker control for the production program session.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_with_workers<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    workers: WorkerBudget,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        false,
        None,
        None,
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::Eager,
        workers,
    )
}

/// Run the production authoritative checker and lend its live semantic state
/// to one emit operation after all public diagnostic getters have completed.
/// The callback cannot retain the snapshot or resolver beyond this call.
/// tsrs-native: scoped callback seam that keeps checked state alive for H1 emit.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_for_emit<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    operation: impl FnMut(&ProgramSnapshot, &CheckerSession<'_>, &CheckResult),
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    check_program_with_authoritative_modules_at_for_emit_with_workers(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        WorkerBudget::serial(),
        operation,
    )
}

/// [`check_program_with_authoritative_modules_at_for_emit`] with an explicit
/// [`WorkerBudget`] for the scoped per-file binding step.
/// tsrs-native: worker control for the production emit session.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_for_emit_with_workers<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    workers: WorkerBudget,
    mut operation: impl FnMut(&ProgramSnapshot, &CheckerSession<'_>, &CheckResult),
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        false,
        Some(&mut operation),
        None,
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::Eager,
        workers,
    )
}

/// [`check_program_with_authoritative_modules_at_for_emit_with_workers`] in
/// the order of the native compiler runner's second Program
/// (`compileFilesWithHost` in `tsc/internal/testutil/harnessutil`:
/// `program.Emit`, then the diagnostic getters). `operation` runs twice over
/// the one checker session: first before any source is checked, with a
/// result that holds no semantic diagnostics, and again after the eager
/// schedule with the checked result. What the first call asks of the
/// session's emit resolver is resolved in emit order, and a diagnostic that
/// depends on the order of resolution is reported where emit first reached
/// it.
/// tsrs-native: harness execution order; the command line checks first.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_emit_first_with_workers<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    workers: WorkerBudget,
    mut operation: impl FnMut(&ProgramSnapshot, &CheckerSession<'_>, &CheckResult),
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        false,
        Some(&mut operation),
        None,
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::EagerAfterEmit,
        workers,
    )
}

/// Run one authoritative harness emit over a caller-owned immutable library
/// prefix. The bundle is scoped by the caller (normally one repeated case),
/// so this path avoids both per-repetition parsing and process-lifetime
/// retention of every distinct `ts-tests` library set.
/// tsrs-native: bundle-scoped variant of the emit callback seam above.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_for_emit_with_harness_lib_bundle<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    bundle: &OwnedHarnessLibBundle,
    mut operation: impl FnMut(&ProgramSnapshot, &CheckerSession<'_>, &CheckResult),
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        true,
        Some(&mut operation),
        Some(bundle),
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::Eager,
        WorkerBudget::serial(),
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
pub fn check_program_with_authoritative_modules_at_harness_cached<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    library_prefix: LibraryPrefixCompletion,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
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
        None,
        None,
        library_prefix,
        DiagnosticSchedule::Eager,
        WorkerBudget::serial(),
    )
}

/// Initialize one authoritative checker for source-by-source declaration APIs.
/// Semantic checking is scheduled by the scoped consumer; compiler options and
/// the ordinary diagnostic/emit entrypoints retain their existing behavior.
#[allow(clippy::too_many_arguments)]
/// tsrs-native: Rust API entry configuring one authoritative checker for per-source
/// declaration APIs
pub fn with_authoritative_modules_at_for_declarations<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    mut operation: impl FnMut(&ProgramSnapshot, &CheckerSession<'_>, &CheckResult),
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        provider,
        false,
        Some(&mut operation),
        None,
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::OnDemand,
        WorkerBudget::serial(),
    )
}

/// Run one authoritative no-emit check with an explicit checker budget.
///
/// With [`CheckerBudget::serial`] this is exactly
/// [`check_program_with_authoritative_modules_at_with_workers`] over one
/// provider constructed by `factory`. With a sharded budget the program files
/// (library prefix included) are partitioned deterministically over up to
/// `checkers` checker states, each constructed on its own scoped thread over
/// the one shared immutable [`ProgramSnapshot`] with its own provider,
/// type-id domain and caches; only diagnostics and audit records cross
/// threads and the merged result is assembled in Program order. The
/// on-demand declaration schedule and the emit callback entries stay serial.
/// tsrs-native: sharded checker driver (tsgo `--checkers`); tsc has one checker.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_with_checkers<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    factory: &dyn AuthoritativeModuleProviderFactory,
    workers: WorkerBudget,
    checkers: CheckerBudget,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode_with_source(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        AuthoritativeProviderSource::PerChecker(factory),
        false,
        None,
        None,
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::Eager,
        workers,
        checkers,
        None,
    )
}

/// [`check_program_with_authoritative_modules_at_with_checkers`] for an
/// emitting session: every shard emits the files it checked with its own
/// checker once the coordinator has gated the merged diagnostics; the caller
/// receives the products through `sharded_emit.emissions` and writes them in
/// Program order. A flagged order-sensitive run replays check and emit
/// serially with one checker.
/// tsrs-native: tsgo's per-checker emit over the shared immutable snapshot.
#[allow(clippy::too_many_arguments)]
pub fn check_program_with_authoritative_modules_at_for_emit_with_checkers<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    factory: &dyn AuthoritativeModuleProviderFactory,
    workers: WorkerBudget,
    checkers: CheckerBudget,
    sharded_emit: &mut ShardedEmit<'_>,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    check_program_with_authoritative_modules_at_cache_mode_with_source(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        AuthoritativeProviderSource::PerChecker(factory),
        false,
        None,
        None,
        LibraryPrefixCompletion::Complete,
        DiagnosticSchedule::Eager,
        workers,
        checkers,
        Some(sharded_emit),
    )
}

#[allow(clippy::too_many_arguments)]
fn check_program_with_authoritative_modules_at_cache_mode<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: &dyn AuthoritativeModuleProvider,
    cache_enabled: bool,
    emit_operation: Option<&mut CheckedEmitOperation<'_>>,
    prepared_owned_bundle: Option<&OwnedHarnessLibBundle>,
    library_prefix: LibraryPrefixCompletion,
    diagnostic_schedule: DiagnosticSchedule,
    workers: WorkerBudget,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    check_program_with_authoritative_modules_at_cache_mode_with_source(
        libs,
        files,
        lib_metadata,
        file_metadata,
        options,
        current_directory,
        AuthoritativeProviderSource::Shared(provider),
        cache_enabled,
        emit_operation,
        prepared_owned_bundle,
        library_prefix,
        diagnostic_schedule,
        workers,
        CheckerBudget::serial(),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn check_program_with_authoritative_modules_at_cache_mode_with_source<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    lib_metadata: &[AuthoritativeSourceMetadata],
    file_metadata: &[AuthoritativeSourceMetadata],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    provider: AuthoritativeProviderSource<'_>,
    cache_enabled: bool,
    emit_operation: Option<&mut CheckedEmitOperation<'_>>,
    prepared_owned_bundle: Option<&OwnedHarnessLibBundle>,
    library_prefix: LibraryPrefixCompletion,
    diagnostic_schedule: DiagnosticSchedule,
    workers: WorkerBudget,
    checkers: CheckerBudget,
    sharded_emit: Option<&mut ShardedEmit<'_>>,
) -> Result<CheckResult, AuthoritativeModuleFailure> {
    let current_directory = current_directory.into();
    perf::add(
        PerfCounter::CheckerShardsRequested,
        checkers.checkers() as u64,
    );
    validate_authoritative_metadata(libs, lib_metadata, "library")?;
    validate_authoritative_metadata(files, file_metadata, "program")?;
    let mut seen_tokens = rustc_hash::FxHashSet::default();
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

    let fixture_names: rustc_hash::FxHashSet<JsStr<'_>> = files
        .iter()
        .filter(|file| !file.host_only)
        .map(|file| file.name.as_js())
        .collect();
    let mut effective_libs = Vec::new();
    let mut effective_lib_metadata = Vec::new();
    for (lib, metadata) in libs.iter().zip(lib_metadata) {
        if !fixture_names.contains(&lib.name.as_js()) {
            effective_libs.push(lib);
            effective_lib_metadata.push(metadata.clone());
        }
    }
    let run = AuthoritativeRun {
        provider,
        lib_metadata: effective_lib_metadata,
        file_metadata: file_metadata.to_vec(),
        library_prefix,
        diagnostic_schedule,
    };
    let mut observe_phase = |_| {};
    let execution = if cache_enabled {
        let prepared = prepared_owned_bundle
            .filter(|bundle| bundle.exactly_matches(&effective_libs, &lib_bundle_options(options)));
        if let Some(bundle) = prepared {
            check_program_with_prebound_libs_at_observed(
                libs,
                files,
                options,
                current_directory,
                &bundle.documents,
                &bundle.identity_domain,
                CheckWorkCounters::default(),
                true,
                &mut observe_phase,
                Some(&run),
                emit_operation,
                ProgramFileFacts::DEFAULT_LIBRARY,
                workers,
            )
        } else {
            let bundle = (!effective_libs.is_empty()).then(|| lib_bundle(&effective_libs, options));
            let lib_documents: &[Arc<BoundDocument>] = match bundle {
                Some(bundle) => bundle.documents,
                None => &[],
            };
            let identity_domain = bundle
                .map(|bundle| bundle.identity_domain.clone())
                .unwrap_or_else(IdentityDomain::ephemeral);
            check_program_with_prebound_libs_at_observed(
                libs,
                files,
                options,
                current_directory,
                lib_documents,
                &identity_domain,
                CheckWorkCounters::default(),
                true,
                &mut observe_phase,
                Some(&run),
                emit_operation,
                ProgramFileFacts::DEFAULT_LIBRARY,
                workers,
            )
        }
    } else {
        let bundle_options = lib_bundle_options(options);
        let identity_domain = IdentityDomain::ephemeral();
        let (lib_sources, lib_work) =
            parse_lib_sources(&effective_libs, &bundle_options, &identity_domain, workers);
        let lib_binders =
            bind_lib_sources(&lib_sources, &bundle_options, &identity_domain, workers);
        let lib_data = binders_into_data(lib_binders);
        let lib_documents = publish_bound_documents(lib_sources, lib_data);
        // Sharding is a property of the eager whole-Program schedule with
        // per-checker providers; the on-demand schedule and emit callbacks
        // keep the serial driver.
        let sharded_factory = match run.provider {
            AuthoritativeProviderSource::PerChecker(factory)
                if checkers.is_sharded()
                    && diagnostic_schedule == DiagnosticSchedule::Eager
                    && emit_operation.is_none() =>
            {
                Some(factory)
            }
            _ => None,
        };
        if let Some(factory) = sharded_factory {
            check_program_with_prebound_libs_sharded(
                libs,
                files,
                options,
                current_directory,
                &lib_documents,
                &identity_domain,
                CheckWorkCounters::for_owned_libs(lib_work),
                true,
                &mut observe_phase,
                &run,
                factory,
                ProgramFileFacts::DEFAULT_LIBRARY,
                workers,
                checkers,
                sharded_emit,
            )
        } else {
            check_program_with_prebound_libs_at_observed(
                libs,
                files,
                options,
                current_directory,
                &lib_documents,
                &identity_domain,
                CheckWorkCounters::for_owned_libs(lib_work),
                true,
                &mut observe_phase,
                Some(&run),
                emit_operation,
                ProgramFileFacts::DEFAULT_LIBRARY,
                workers,
            )
        }
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
    let source_inputs = inputs.iter().filter(|input| !input.host_only);
    let source_input_count = source_inputs.clone().count();
    if source_input_count != metadata.len() {
        return Err(AuthoritativeModuleFailure::InvalidMetadata {
            detail: format!(
                "authoritative {kind} metadata has {} rows for {} inputs",
                metadata.len(),
                source_input_count
            ),
        });
    }
    for (index, (input, source)) in source_inputs.zip(metadata).enumerate() {
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

/// Host facts projected once from the input list: immutable data with no
/// checker identity. The serial driver moves them into its one checker state;
/// a sharded driver (W2) clones them once per additional shard, so that a
/// shard constructs its state from the shared immutable snapshot plus this
/// value alone. The tables are shared, not copied (eight copies of Next.js's
/// parsed package manifests were 5 ms of the coordinator's setup), and so
/// are the source ASTs: every state shares the snapshot's
/// `Arc<BoundDocument>` handles.
/// tsrs-native: the resolver's host view; tsc reads its host lazily.
#[derive(Clone)]
struct HostFacts {
    current_directory: JsString,
    file_paths: Arc<rustc_hash::FxHashSet<JsString>>,
    input_snapshots: Arc<rustc_hash::FxHashMap<JsString, Arc<TextSnapshot>>>,
    package_json_module_types: Arc<rustc_hash::FxHashMap<JsString, state::PackageJsonModuleType>>,
    package_json_values: Arc<rustc_hash::FxHashMap<JsString, tsc_program::JsonValue>>,
    package_json_names: Arc<rustc_hash::FxHashMap<JsString, JsString>>,
}

/// Stage-1 output of the check driver: parsed (or adopted) fixture sources
/// plus the owned facts every checker state needs. No checker identity or
/// bind result is created here.
struct ParsedProgramInputs {
    program_sources: Vec<Arc<tsc_syntax::SourceFile>>,
    authoritative_program_metadata: Vec<AuthoritativeSourceMetadata>,
    program_diagnostics: Vec<Diagnostic>,
    host: HostFacts,
}

/// Stage 1: fixture shadowing, root admission, JSON/TS parsing or adoption of
/// the loader's parse, missing-path-reference diagnostics and the host facts.
/// tsrs-native: extracted from the one-shot driver so that W2 can run one
/// parse/bind and many checker states over the same snapshot.
/// A program source between its identity lease (taken in program order on
/// the calling thread) and its rewrite into the leased ranges (on a worker).
enum PendingProgramSource {
    Ready(tsc_syntax::SourceFile),
    Leased(tsc_syntax::SourceFile, IdentityLease, IdentityLease),
}

impl PendingProgramSource {
    fn weight(&self) -> usize {
        match self {
            Self::Ready(_) => 0,
            Self::Leased(source, _, _) => source.arena.nodes().len(),
        }
    }

    fn source_mut(&mut self) -> &mut tsc_syntax::SourceFile {
        match self {
            Self::Ready(source) | Self::Leased(source, _, _) => source,
        }
    }

    fn relocate(self) -> tsc_syntax::SourceFile {
        match self {
            Self::Ready(source) => source,
            Self::Leased(mut source, node_lease, array_lease) => {
                source
                    .relocate_with_leases(node_lease, array_lease)
                    .expect("source identity relocation failed");
                source
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_program_inputs(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: JsStr<'_>,
    identity_domain: &IdentityDomain,
    authoritative_run: Option<&AuthoritativeRun<'_>>,
    work_counters: &mut CheckWorkCounters,
    workers: WorkerBudget,
) -> ParsedProgramInputs {
    // Every host package.json, parsed once (a later input with the same
    // path replaces an earlier one).
    let package_json_values: rustc_hash::FxHashMap<JsString, tsc_program::JsonValue> = files
        .iter()
        .filter(|file| {
            file.name
                .as_js()
                .split_ascii(b'/')
                .next_back()
                .and_then(|name| name.split_ascii(b'\\').next_back())
                .is_some_and(|name| name == "package.json")
        })
        .map(|file| {
            (
                state::CheckerState::normalize_program_path(&file.name, ""),
                parse_host_package_json(file),
            )
        })
        .collect();
    // getImpliedNodeFormatForFileWorker's package-scope input. Build it
    // before parsing because getSetExternalModuleIndicator's Auto mode
    // consults the implied format while SourceFiles are created.
    let host_package_json_module_types: rustc_hash::FxHashMap<
        tsc_types::JsString,
        state::PackageJsonModuleType,
    > = package_json_values
        .iter()
        .map(|(path, value)| {
            let module_type = tsc_program::package_json_property(value, "type")
                .and_then(tsc_program::JsonValue::as_js)
                .map(|value| match value.as_str() {
                    Some("module") => state::PackageJsonModuleType::Module,
                    Some("commonjs") => state::PackageJsonModuleType::CommonJs,
                    _ => state::PackageJsonModuleType::Other,
                })
                .unwrap_or(state::PackageJsonModuleType::Missing);
            (path.clone(), module_type)
        })
        .collect();
    // Fixture-file shadowing (unchanged from the libless world): a
    // later file with the same name shadows an earlier one entirely.
    let mut last_index_by_name = rustc_hash::FxHashMap::default();
    for (index, file) in files.iter().enumerate() {
        if file.host_only {
            continue;
        }
        last_index_by_name.insert(file.name.as_js(), index);
    }

    // Fixture parse pass: every published source receives exact node/array
    // leases from the same domain as the library prefix. JSON files remain in
    // that same program: the binder publishes their root value as the
    // module's default/export= property.
    let serial_started = std::time::Instant::now();
    let mut pending_sources: Vec<PendingProgramSource> = Vec::new();
    let mut authoritative_program_metadata = Vec::new();
    let mut authoritative_file_index = 0;
    for (index, file) in files.iter().enumerate() {
        if file.host_only {
            continue;
        }
        let authoritative_metadata = authoritative_run.and_then(|run| {
            let metadata = run.file_metadata.get(authoritative_file_index);
            authoritative_file_index += 1;
            metadata
        });
        if last_index_by_name.get(&file.name.as_js()) != Some(&index) {
            continue;
        }
        // tsc createProgram only loads roots with supported extensions;
        // anything else (.txt, extensionless, .js without allowJs) never
        // yields syntactic diagnostics. allowNonTsExtensions (transpile
        // routes) admits every root the Program loader accepted.
        if !is_supported_source_file_name(&file.name, options.allow_js)
            && options.allow_non_ts_extensions != Some(true)
        {
            continue;
        }
        let authoritative_implied_node_format =
            authoritative_metadata.and_then(|source| source.implied_node_format);
        if let Some(metadata) = authoritative_metadata {
            authoritative_program_metadata.push(metadata.clone());
        }
        // tsc ensureScriptKind: .json programs parse as JSON values.
        if file.name.ends_with(".json") {
            let source_file = tsc_syntax::parse_json_text_from_snapshot_in_identity_domain(
                file.name.clone(),
                Arc::clone(file.snapshot()),
                identity_domain,
            )
            .expect("JSON source identity allocation failed");
            work_counters.record_parse(file.text().len());
            pending_sources.push(PendingProgramSource::Ready(source_file));
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
                                .as_js()
                                .split_ascii(b'/')
                                .any(|segment| segment == "node_modules");
                        let package_eligible = [".ts", ".tsx", ".js", ".jsx"]
                            .iter()
                            .any(|extension| file.name.ends_with(extension));
                        let package_scope_is_module = if authoritative_run.is_some() {
                            authoritative_implied_node_format
                                == Some(AuthoritativeResolutionMode::EsNext)
                        } else if package_lookup_enabled && package_eligible {
                            let mut directory = normalized
                                .as_js()
                                .rsplit_once("/")
                                .map(|(directory, _)| directory)
                                .unwrap_or("".into());
                            loop {
                                let package_json = if directory.is_empty() {
                                    JsString::from("/package.json")
                                } else {
                                    concat_js(&[&directory, &"/package.json"])
                                };
                                if let Some(&module_type) =
                                    host_package_json_module_types.get(package_json.as_bytes())
                                {
                                    break module_type == state::PackageJsonModuleType::Module;
                                }
                                let Some((parent, _)) = directory.rsplit_once("/") else {
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
        let parse_options = tsc_syntax::ParseOptions {
            script_target: options.emit_script_target(),
            language_variant,
            javascript_file,
            force_external_module,
            detect_external_module_from_jsx,
            node_id_base: 0,
            node_array_id_base: 0,
            js_doc_parsing_mode: file
                .js_doc_parsing_mode
                .unwrap_or(tsc_syntax::JSDocParsingMode::ParseAll),
        };
        // An adopted tree takes its identity lease here, in program order,
        // and is rewritten on a worker below; a fresh parse allocates in
        // the domain directly, in the same order.
        let mut pending = match file.take_preparsed_source(&parse_options) {
            Some(source_file) => {
                work_counters.record_adoption();
                let (node_lease, array_lease) = source_file
                    .lease_identities(identity_domain)
                    .expect("source identity allocation failed");
                PendingProgramSource::Leased(source_file, node_lease, array_lease)
            }
            None => {
                work_counters.record_parse(file.text().len());
                PendingProgramSource::Ready(
                    tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                        file.name.clone(),
                        Arc::clone(file.snapshot()),
                        parse_options,
                        None,
                        identity_domain,
                    )
                    .expect("source identity allocation failed"),
                )
            }
        };
        // transpileWorker (typescript.js:146099-146104) assigns the API
        // moduleName / renamedDependencies to the created SourceFile before
        // createProgram; the parsed pragma value is overridden.
        let source_file = pending.source_mut();
        if let Some(module_name) = &file.module_name {
            source_file.module_name = Some(module_name.clone());
        }
        if !file.renamed_dependencies.is_empty() {
            source_file.renamed_dependencies = file.renamed_dependencies.clone();
        }
        pending_sources.push(pending);
    }
    tsc_types::trace::mark(
        "checker: adopt (serial: parse options, leases)",
        serial_started,
    );
    let rewrite_started = std::time::Instant::now();
    let program_sources: Vec<Arc<tsc_syntax::SourceFile>> = workers
        .map_ordered(
            pending_sources,
            PendingProgramSource::weight,
            PendingProgramSource::relocate,
        )
        .into_iter()
        .map(Arc::new)
        .collect();
    tsc_types::trace::mark("checker: adopt (parallel rewrite)", rewrite_started);

    let host_current_directory = resolve_host_current_directory(current_directory);
    let mut program_diagnostics = missing_path_reference_diagnostics(
        program_sources.iter().map(Arc::as_ref),
        libs.iter().chain(files.iter()).map(|file| {
            state::CheckerState::normalize_program_path(&file.name, &host_current_directory)
        }),
        options,
        &host_current_directory,
    );
    // The Program's include-processor rows join the semantic diagnostics of
    // the source they belong to, under that source's checker name.
    if let Some(run) = authoritative_run {
        let rows = match run.provider {
            AuthoritativeProviderSource::Shared(provider) => {
                provider.include_processor_diagnostics()
            }
            AuthoritativeProviderSource::PerChecker(factory) => {
                factory.provider().include_processor_diagnostics()
            }
        };
        for (token, mut row) in rows {
            if let Some(metadata) = run
                .lib_metadata
                .iter()
                .chain(&authoritative_program_metadata)
                .find(|metadata| metadata.token == token)
            {
                row.file_name = Some(metadata.file_name.clone());
                program_diagnostics.push(row);
            }
        }
    }

    let file_paths = files
        .iter()
        .map(|file| state::CheckerState::normalize_program_path(&file.name, ""))
        .collect();
    let input_snapshots = files
        .iter()
        .map(|file| {
            (
                state::CheckerState::normalize_program_path(&file.name, ""),
                Arc::clone(file.snapshot()),
            )
        })
        .collect();
    let package_json_names = package_json_values
        .iter()
        .filter_map(|(path, value)| {
            // Package self-name resolution consumes the original string;
            // getPathComponents does not trim it (_tsc.js:41454–41458).
            let name = tsc_program::package_json_property(value, "name")?.as_js()?;
            if name.is_empty() {
                return None;
            }
            Some((path.clone(), name.to_owned()))
        })
        .collect();
    ParsedProgramInputs {
        program_sources,
        authoritative_program_metadata,
        program_diagnostics,
        host: HostFacts {
            current_directory: host_current_directory,
            file_paths: Arc::new(file_paths),
            input_snapshots: Arc::new(input_snapshots),
            package_json_module_types: Arc::new(host_package_json_module_types),
            package_json_values: Arc::new(package_json_values),
            package_json_names: Arc::new(package_json_names),
        },
    }
}

/// Stage 3: construct one checker state over the shared immutable snapshot.
/// This is the per-checker constructor: it reads only the shared snapshot and
/// options (never writing into the shared source ASTs) and owns everything
/// else it creates, so W2 can call it inside each shard's thread (the provider
/// is constructed by the caller and outlives the state; the host facts are
/// moved in).
/// tsrs-native: initializeTypeChecker's ordered init as one constructor.
fn init_checker_state<'a>(
    snapshot: &'a ProgramSnapshot,
    options: &'a CompilerOptions,
    authoritative: Option<(
        &'a dyn AuthoritativeModuleProvider,
        &[AuthoritativeSourceMetadata],
    )>,
    host: HostFacts,
) -> state::CheckerState<'a> {
    let mut state = state::CheckerState::from_snapshot_deferring_globals(snapshot, options);
    if let Some((provider, metadata)) = authoritative {
        if let Err(failure) = state.install_authoritative_module_provider(provider, metadata) {
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
    state.host_current_directory = host.current_directory;
    // The resolver's host view (M4 5.8d): every INPUT path, incl.
    // files the program dropped (.json bodies, .js without
    // allowJs) — the suppression probes need them to keep 2307
    // FP-free.
    state.host_file_paths = host.file_paths;
    state.host_input_snapshots = host.input_snapshots;
    state.host_package_json_module_types = host.package_json_module_types;
    state.host_package_json_values = host.package_json_values;
    state.host_package_json_names = host.package_json_names;
    // The globals merge runs once the resolver's host view exists: tsgo's
    // mergeSymbol resolves an alias it merges into (checker.go mergeSymbol),
    // which can load a module of the program.
    state.initialize_deferred_program_globals();
    // initializeTypeChecker's augmentation passes (88769/88874)
    // run here — AFTER the resolver's host view exists (pass 2
    // resolves module names), BEFORE any file checks.
    state.merge_module_augmentations();
    // Type construction is unconditional in tsc. In particular, the
    // eager array singleton roots establish the type-id order consumed by
    // getUnionType when stableTypeOrdering is off. Requesting the public
    // global-diagnostics bucket controls only observation of the rows.
    state.materialize_init_global_diagnostics();
    state
}

/// Stage 4: check `files` in the given order on one state. getDiagnosticsWorker
/// snapshots the file-less bucket around each requested source, so only the
/// rows published while checking a file are attributed to that file.
/// tsrs-native: shared by the fixture pass and the library-completion pass.
fn check_files_in_order(
    state: &mut state::CheckerState<'_>,
    files: &[ProgramFileId],
    globals_by_file: &mut [Vec<Diagnostic>],
) {
    for &file in files {
        check_program_file(state, file, globals_by_file);
    }
}

/// Check one Program file (unless the options skip it) and attribute the
/// global rows its check produced to it.
fn check_program_file(
    state: &mut state::CheckerState<'_>,
    file: ProgramFileId,
    globals_by_file: &mut [Vec<Diagnostic>],
) {
    if state.skip_type_checking_file(file) {
        return;
    }
    let global_start = state.visible_global_diagnostics.len();
    state.check_source_file(file.index());
    globals_by_file[file.index()].extend(
        state.visible_global_diagnostics[global_start..]
            .iter()
            .cloned(),
    );
}

fn program_file_id(file: usize) -> ProgramFileId {
    ProgramFileId::from_raw(u32::try_from(file).expect("program file index"))
}

/// Syntactic rows for every fixture file of a snapshot, in Program order.
/// tsc getSyntacticDiagnosticsForFile: JS files prepend the
/// TypeScript-only-syntax walker output to parser diagnostics.
fn syntactic_file_rows(
    snapshot: &ProgramSnapshot,
    lib_count: usize,
    options: &CompilerOptions,
) -> Vec<FileDiagnosticPasses> {
    snapshot
        .documents()
        .iter()
        .skip(lib_count)
        .map(|document| {
            let source = document.source();
            let mut syntactic = if is_js_file_name(&source.file_name) {
                let mut rows = js_grammar::get_js_syntactic_diagnostics(source);
                // tsgo GetSyntacticDiagnostics (compiler/program.go:743-754):
                // the option-dependent rows of a JavaScript file the checker
                // does not check (IsCheckJSEnabledForFile).
                let check_js = match check_directive(source.text()) {
                    Some(directive) => directive == CheckDirective::Check,
                    None => options.check_js == Some(true),
                };
                if !check_js && !options.experimental_decorators {
                    rows.extend(js_grammar::get_additional_js_syntactic_diagnostics(source));
                }
                rows
            } else {
                Vec::new()
            };
            syntactic.extend(source.parse_diagnostics.iter().cloned());
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut syntactic);
            FileDiagnosticPasses {
                file_name: source.file_name.clone(),
                syntactic,
                semantic: Vec::new(),
                suggestion: Vec::new(),
            }
        })
        .collect()
}

/// The diagnostic ledger of one checker state at one moment, plus the
/// file-attributed rows and partially-checked ranges recorded up to then.
/// Rows are cloned, not indexed by length: later passes may still edit
/// earlier rows (category/related-information updates), so the serial
/// driver's projection boundary needs the rows as they were.
#[derive(Default)]
struct LedgerSnapshot {
    rows: Vec<Diagnostic>,
    globals_by_file: Vec<Vec<Diagnostic>>,
    partially_checked_ranges: rustc_hash::FxHashMap<usize, Vec<(u32, u32)>>,
}

impl LedgerSnapshot {
    fn take(state: &state::CheckerState<'_>, globals_by_file: &[Vec<Diagnostic>]) -> Self {
        Self {
            rows: state.diagnostics.iter().cloned().collect(),
            globals_by_file: globals_by_file.to_vec(),
            partially_checked_ranges: state.partially_checked_ranges.clone(),
        }
    }
}

/// Everything one checker shard hands back to the coordinating thread.
/// Owned values only: diagnostics, attribution vectors and audit records.
/// No `TypeId`, `SymbolId`, signature or mapper identity leaves the shard.
struct ShardOutput {
    /// The ledger after the shard's fixture pass (the serial driver's
    /// per-file projection boundary), before any library-completion check.
    fixture: LedgerSnapshot,
    /// The ledger after every pass.
    complete: LedgerSnapshot,
    /// The file-less bucket right after checker initialization; identical
    /// across shards by construction (same snapshot, same init sequence).
    init_globals: Vec<Diagnostic>,
    partial_check_records: Vec<PartialCheck>,
    failure: Option<AuthoritativeModuleFailure>,
    /// Order-sensitivity guard reasons recorded by this shard (W2c); any
    /// non-zero value makes the driver replay the whole check serially.
    order_reasons: u32,
    /// Type count after this shard's initialization: identical across shards
    /// by construction (deterministic init over the shared snapshot); the
    /// driver verifies it and replays on divergence.
    init_boundary: u32,
    /// Display-class observations (rendered member lists, order-chosen
    /// elaborations) of this shard; they matter only if a published
    /// diagnostic contains them (W2e).
    display_marks: crate::order_guard::DisplayMarks,
    /// The thread that ran this shard (participation evidence for the work
    /// counters and the native controls).
    thread: std::thread::ThreadId,
    /// The Program files this shard checked (fixtures and, for a complete
    /// library prefix, library files), in Program order: the driver hands
    /// them to the emit gate and the per-shard emit.
    files: Vec<usize>,
    /// Files whose checkSourceFileWorker body ran in this shard: ownership
    /// evidence for the debug assertion in the merge (debug builds only).
    #[cfg(debug_assertions)]
    checked_files: Vec<usize>,
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<ShardOutput>();
    assert_send::<HostFacts>();
};

/// One checker shard's emit products plus the evidence the driver merges.
/// Produced on the shard's thread by the caller's emit closure.
pub struct ShardEmission {
    pub units: Vec<tsc_emitter::UnitEmission>,
    /// H2.8c evidence: source files whose checkSourceFileWorker body ran in
    /// this shard by the end of its emit.
    pub checked_source_files: u32,
}

/// The per-shard emit protocol of the sharded driver.
/// tsrs-native: tsgo shape — every checker emits the files it checked, and
/// the coordinator publishes the outputs in Program order. The coordinator
/// decides once, after the merged diagnostics exist, whether any shard emits
/// (handleNoEmitOptions and driver-level refusals); each shard then runs the
/// caller's closure with its own live checker session. Outputs are handed
/// back through [`emissions`](Self::emissions); nothing is written by the
/// shards themselves.
pub struct ShardedEmit<'op> {
    /// Coordinator decision after the merged diagnostics are known, given
    /// every shard's checker session and the Program file indices each
    /// checked (as `emit` receives them) so a noEmitOnError declaration gate
    /// can query each shard's resolver: `true` runs the per-shard emit,
    /// `false` releases the shards without one.
    pub gate: ShardGateClosure<'op>,
    /// Runs once after every shard has checked, with every shard's checker
    /// session and the Program file indices each shard checked (Program
    /// order, index-aligned with the sessions). The caller schedules each
    /// planned unit on its worker budget against the session of the shard
    /// that checked the unit's source.
    pub emit: ShardEmitClosure<'op>,
    /// Filled by the driver: `None` when the gate refused (or no checker
    /// ran), otherwise every shard's products (a serial replay yields one).
    pub emissions: Option<Result<Vec<ShardEmission>, tsc_emitter::UnitEmitError>>,
    /// Runs on a shard's own thread once it has checked its files, over the
    /// session of its checked state (its diagnostics already taken), before
    /// the shards are merged: work the gate may use if it admits, such as
    /// the --noEmit declaration diagnostics of the shard's files, then
    /// overlaps the shards still checking instead of following the slowest.
    /// The gate decides whether the work counts; it must be able to redo it.
    pub eager: Option<ShardEagerClosure<'op>>,
    /// Runs on its own thread over the snapshot while the shards check:
    /// work that depends on the parsed syntax only, such as preparing the
    /// emit copies of the largest sources ahead of their emit.
    pub prelude: Option<ShardPreludeClosure<'op>>,
}

/// The syntax-only work a [`ShardedEmit`] request runs beside the shards.
pub type ShardPreludeClosure<'op> = &'op (dyn Fn(&ProgramSnapshot) + Sync);

/// The per-shard eager work of a [`ShardedEmit`] request: the shard index,
/// the snapshot, the shard's checked session, its Program file indices and
/// the number of shards still checking (the closure may stop its eager work
/// once that reaches zero and leave the rest to the coordinator's pool).
pub type ShardEagerClosure<'op> = &'op (dyn Fn(
    usize,
    &ProgramSnapshot,
    &CheckerSession<'_>,
    &[usize],
    &std::sync::atomic::AtomicUsize,
) + Sync);

type ShardGateClosure<'op> = &'op mut dyn FnMut(
    &ProgramSnapshot,
    &CheckResult,
    &[CheckerSession<'_>],
    &[Vec<usize>],
) -> bool;

type ShardEmitClosure<'op> = &'op (dyn Fn(
    &ProgramSnapshot,
    &[CheckerSession<'_>],
    &[Vec<usize>],
) -> Result<Vec<ShardEmission>, tsc_emitter::UnitEmitError>
          + Sync);

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<ShardEmission>();
};

/// Size a checker's arenas from the syntax it will check, so one reservation
/// replaces the doubling copies of a growing arena. The unused capacity is
/// never touched, so it costs address space, not memory. Purely an allocation
/// hint: no identity depends on it.
///
/// The fractions cover the largest shard of the benchmark programs (per node
/// of the reserved count): types up to 0.8 (hono 1.9), transient symbols up
/// to 1.8 (hono 2.8), mappers up to 1.2 (hono 1.5), resolved members up to
/// 0.3 and signatures up to 0.2.
fn reserve_type_tables(state: &mut state::CheckerState<'_>, node_count: usize) {
    const MIN_RESERVED: usize = 1 << 12;
    const MAX_RESERVED: usize = 1 << 20;
    let reserve =
        |per_node: f64| ((node_count as f64 * per_node) as usize).clamp(MIN_RESERVED, MAX_RESERVED);
    state.tables.reserve_types(reserve(1.0));
    // Transient symbols outgrow the shared cap on the largest programs (VS
    // Code's shards by 3-7 %, its serial check three times over), and each
    // doubling copies the arena while the old buffer is still resident;
    // untouched capacity stays virtual, so this arena takes a higher cap.
    const MAX_RESERVED_TRANSIENT_SYMBOLS: usize = 1 << 22;
    state.binder.reserve_transient_symbols(
        ((node_count as f64 * 2.0) as usize).clamp(MIN_RESERVED, MAX_RESERVED_TRANSIENT_SYMBOLS),
    );
    state.mappers.reserve(reserve(1.5));
    state.mapper_lists.reserve(reserve(2.0));
    state.members.reserve(reserve(0.5));
    state.signatures.reserve(reserve(0.25));
}

/// With `TSRS_MEMORY_REPORT` set, print what the parsed and bound documents
/// of `snapshot` allocate, by structure, to stderr.
fn report_program_memory(snapshot: &ProgramSnapshot) {
    if std::env::var_os("TSRS_MEMORY_REPORT").is_none() {
        return;
    }
    let mut syntax = tsc_syntax::SyntaxMemory::default();
    let mut binder = tsc_binder::BinderMemory::default();
    let mut text_bytes = 0usize;
    for document in snapshot.documents() {
        let source = document.source();
        text_bytes += source.text().len();
        source.arena.add_memory_usage(&mut syntax);
        document.data.add_memory_usage(&mut binder);
    }
    let mib = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
    let row = |name: &str, count: usize, bytes: usize| {
        eprintln!("[memory] {name:<34} {count:>11} {:>10.1} MiB", mib(bytes));
    };
    eprintln!(
        "[memory] {} documents, source text {:.1} MiB",
        snapshot.documents().len(),
        mib(text_bytes)
    );
    row("syntax: nodes (Node)", syntax.nodes, syntax.node_bytes);
    row(
        "syntax: node arrays (NodeArray)",
        syntax.arrays,
        syntax.array_bytes,
    );
    row(
        "syntax: node array items",
        syntax.array_items,
        syntax.array_item_bytes,
    );
    let (names, name_bytes) = tsc_types::EscapedName::interned_names();
    row("syntax: identifiers (interned)", syntax.identifiers, 0);
    row("names: interned name texts", names, name_bytes);
    row(
        "syntax: other literal strings",
        syntax.string_nodes,
        syntax.string_bytes,
    );
    let syntax_total =
        syntax.node_bytes + syntax.array_bytes + syntax.array_item_bytes + syntax.string_bytes;
    row("syntax: total", syntax.nodes, syntax_total);
    let table_bytes: usize = binder.tables.values().sum();
    row(
        "binder: symbols (Symbol)",
        binder.symbols,
        binder.symbol_bytes,
    );
    row(
        "binder: symbol names/declarations",
        binder.symbols,
        binder.symbol_owned_bytes,
    );
    row(
        "binder: member/export tables",
        binder.tables.len(),
        table_bytes,
    );
    row("binder: locals tables", binder.locals, binder.local_bytes);
    row(
        "binder: node_symbol (per node)",
        syntax.nodes,
        binder.node_symbol_bytes,
    );
    row(
        "binder: node_flow (per node)",
        syntax.nodes,
        binder.node_flow_bytes,
    );
    row(
        "binder: node_flags_mut (per node)",
        syntax.nodes,
        binder.node_flags_bytes,
    );
    row(
        "binder: flow nodes (FlowNode)",
        binder.flow_nodes,
        binder.flow_bytes,
    );
    row(
        "binder: flow antecedent lists",
        binder.flow_nodes,
        binder.flow_antecedent_bytes,
    );
    row("binder: node-keyed hash maps", 0, binder.map_bytes);
    let binder_total = binder.symbol_bytes
        + binder.symbol_owned_bytes
        + table_bytes
        + binder.local_bytes
        + binder.node_symbol_bytes
        + binder.node_flow_bytes
        + binder.node_flags_bytes
        + binder.flow_bytes
        + binder.flow_antecedent_bytes
        + binder.map_bytes;
    row("binder: total", binder.symbols, binder_total);
    let mut kinds: Vec<_> = syntax.kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1));
    for (kind, count) in kinds.into_iter().take(12) {
        let name = format!("{kind:?}");
        eprintln!("[memory]   kind {name:<30} {count:>11}");
    }
}

fn snapshot_node_count(snapshot: &ProgramSnapshot) -> usize {
    snapshot
        .documents()
        .iter()
        .map(|document| document.source().arena.nodes().len())
        .sum()
}

/// Run one checker shard on the calling thread: construct its `ProgramBinder`
/// and `CheckerState` over the shared immutable snapshot with the provider the
/// coordinator created for it, initialize, check the shard's files, and
/// report the ledger snapshots. With `keep_state` the checked state is
/// returned for the coordinator's emit pool; otherwise it is dropped (or
/// leaked for a one-shot process).
#[allow(clippy::too_many_arguments)]
fn run_checker_shard<'a>(
    shard_index: usize,
    snapshot: &'a ProgramSnapshot,
    options: &'a CompilerOptions,
    provider: &'a dyn AuthoritativeModuleProvider,
    metadata: &[AuthoritativeSourceMetadata],
    host: HostFacts,
    queue: &shard::ShardFileQueue,
    reserved_nodes: usize,
    complete_library_prefix: bool,
    keep_state: bool,
    leak_state: bool,
    replay_on_order: bool,
    eager: Option<ShardEagerClosure<'_>>,
    checking: &std::sync::atomic::AtomicUsize,
) -> (ShardOutput, Option<state::CheckerState<'a>>) {
    let shard_started = std::time::Instant::now();
    let mut state = init_checker_state(snapshot, options, Some((provider, metadata)), host);
    reserve_type_tables(&mut state, reserved_nodes);
    // W2c: every type created from here on is shard-local. In the exact mode
    // the guard records order-consuming operations over two or more of them
    // so the driver can replay the check serially; the default mode keeps
    // the sharded result and records nothing.
    let init_boundary = state.tables.len();
    // With stableTypeOrdering the member order no longer follows the
    // shard-local ids, so there is no order to guard.
    if replay_on_order && !state.stable_type_ordering {
        state.order_guard.arm(init_boundary);
    }
    if tsc_types::trace::enabled() {
        tsc_types::trace::mark(
            &format!("shard {shard_index}: init ({init_boundary} types)"),
            shard_started,
        );
    }
    let shard_started = std::time::Instant::now();
    let init_globals = state.visible_global_diagnostics.clone();
    let mut globals_by_file = vec![Vec::new(); state.binder.file_count()];
    // Fixtures first, then (for a complete library prefix) the library
    // files, each pulled from the shared Program-order queue as this shard
    // becomes free; both passes run in increasing Program order within the
    // shard, as the serial completion pass checks them.
    let mut files = Vec::new();
    // `TSRS_FILE_TRACE=1` prints one line per checked file with the static
    // sizes a partition cost model can use and the measured check time.
    let file_trace = std::env::var_os("TSRS_FILE_TRACE").is_some();
    while let Some(file) = queue.next_fixture(shard_index) {
        // Shared-AST invariant: the shard's binder borrows the snapshot's
        // documents (pointer-identical sources); it never copies a tree.
        debug_assert!(std::ptr::eq(
            state.binder.source(file),
            snapshot.document(file).source()
        ));
        let file_started = file_trace.then(std::time::Instant::now);
        check_program_file(&mut state, program_file_id(file), &mut globals_by_file);
        if let Some(started) = file_started {
            let document = snapshot.document(file);
            eprintln!(
                "[file] shard={shard_index} file={file} nodes={} symbols={} flow={} ms={:.3} {}",
                document.source().arena.len(),
                document.data.symbols.len(),
                document.data.flow.len(),
                started.elapsed().as_secs_f64() * 1e3,
                document.source().file_name.as_js().to_string_lossy()
            );
        }
        files.push(file);
        if state.order_guard.reasons() != 0 && replay_on_order {
            // The replay is certain: release the other shards' remaining
            // files too (a real program consumes shard-local type order in
            // its first files; without this the whole parallel phase ran to
            // completion before being discarded).
            queue.abort();
            break;
        }
    }
    let fixture = LedgerSnapshot::take(&state, &globals_by_file);
    if complete_library_prefix {
        while let Some(file) = queue.next_library(shard_index) {
            debug_assert!(std::ptr::eq(
                state.binder.source(file),
                snapshot.document(file).source()
            ));
            check_program_file(&mut state, program_file_id(file), &mut globals_by_file);
            files.push(file);
            if state.order_guard.reasons() != 0 && replay_on_order {
                queue.abort();
                break;
            }
        }
    }
    files.sort_unstable();
    // This shard has checked its files: the eager work of every shard reads
    // the count of shards still checking.
    checking.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    #[cfg(debug_assertions)]
    let checked_files = files
        .iter()
        .copied()
        .filter(|&file| {
            state
                .links
                .read_node(state.binder.source(file).root, |links| links.check_flags)
                .intersects(tsc_types::NodeCheckFlags::TYPE_CHECKED)
        })
        .collect();
    let failure = state.take_authoritative_module_failure();
    let complete = LedgerSnapshot::take(&state, &globals_by_file);
    if tsc_types::trace::enabled() {
        tsc_types::trace::mark(
            &format!(
                "shard {shard_index}: check ({} types, {} symbol links, {} files)",
                state.tables.len(),
                state.links.symbol_len(),
                files.len()
            ),
            shard_started,
        );
    }
    state.report_memory(&format!("shard {shard_index}"));
    state.line_profile.flush();
    let mut output = ShardOutput {
        fixture,
        complete,
        init_globals,
        partial_check_records: std::mem::take(&mut state.partial_check_records),
        failure,
        order_reasons: state.order_guard.reasons(),
        init_boundary: u32::try_from(init_boundary).expect("type count fits u32"),
        display_marks: state.order_guard.marks().clone(),
        thread: std::thread::current().id(),
        files,
        #[cfg(debug_assertions)]
        checked_files,
    };
    if keep_state {
        let state = match eager {
            Some(eager) => {
                // Over the session of the checked state, whose ledgers were
                // taken above: what the eager work resolves cannot reach the
                // shard's diagnostics, and the type order it consumes counts
                // like the check's own.
                let session = CheckerSession::from_checked_state(state);
                eager(shard_index, snapshot, &session, &output.files, checking);
                let state = session.into_state();
                output.order_reasons = state.order_guard.reasons();
                output.display_marks = state.order_guard.marks().clone();
                state
            }
            None => state,
        };
        return (output, Some(state));
    }
    if leak_state {
        std::mem::forget(state);
    }
    (output, None)
}

/// Merge the shards' ledgers in Program order into the same observations the
/// serial driver publishes: the per-file fixture projection (rows of every
/// shard at its fixture boundary), the whole-Program list (rows of every
/// shard after completion), init globals, partial-check records and the
/// checker work counters. Rows for a file published by ANY shard — including
/// library completion rows on merged declarations and partially-checked
/// ranges recorded for another shard's file — are merged BEFORE directive
/// filtering and unused-directive synthesis.
#[allow(clippy::too_many_arguments)]
fn merge_shard_outputs(
    snapshot: &ProgramSnapshot,
    lib_count: usize,
    options: &CompilerOptions,
    program_diagnostics: &[Diagnostic],
    mut file_diagnostics: Vec<FileDiagnosticPasses>,
    mut outputs: Vec<ShardOutput>,
    collect_global_diagnostics: bool,
    work_counters: CheckWorkCounters,
    replay_on_display_marks: bool,
) -> Result<CheckExecution, u32> {
    let authoritative_failure = outputs.iter_mut().find_map(|output| output.failure.take());
    debug_assert!(
        outputs
            .iter()
            .all(|output| output.init_globals == outputs[0].init_globals),
        "checker initialization published different global rows in different shards"
    );
    #[cfg(debug_assertions)]
    {
        let mut owners = vec![0u8; snapshot.documents().len()];
        for output in &outputs {
            for &file in &output.checked_files {
                owners[file] += 1;
            }
        }
        debug_assert!(
            owners.iter().all(|&count| count <= 1),
            "a program file was checked by more than one shard"
        );
    }
    let global_diagnostics = if collect_global_diagnostics {
        let mut rows = outputs[0].init_globals.clone();
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut rows);
        rows
    } else {
        Vec::new()
    };

    // Rows by owning file, in (shard, publication) order.
    fn rows_by_file<'o>(
        ledgers: impl Iterator<Item = &'o LedgerSnapshot>,
    ) -> rustc_hash::FxHashMap<&'o JsString, Vec<&'o Diagnostic>> {
        let mut by_file: rustc_hash::FxHashMap<&JsString, Vec<&Diagnostic>> =
            rustc_hash::FxHashMap::default();
        for ledger in ledgers {
            for row in &ledger.rows {
                if let Some(file_name) = row.file_name.as_ref() {
                    by_file.entry(file_name).or_default().push(row);
                }
            }
        }
        by_file
    }
    let fixture_rows = rows_by_file(outputs.iter().map(|output| &output.fixture));
    let complete_rows = rows_by_file(outputs.iter().map(|output| &output.complete));
    let file_count = snapshot.documents().len();
    let skip = |file: usize| {
        should_skip_type_checking_file(
            snapshot.document(file).source(),
            snapshot.file_facts(ProgramFileId::from_raw(
                u32::try_from(file).expect("program file index"),
            )),
            options,
        )
    };
    let assemble = |file: usize,
                    rows: &rustc_hash::FxHashMap<&JsString, Vec<&Diagnostic>>,
                    ledger: fn(&ShardOutput) -> &LedgerSnapshot|
     -> DiagnosticList {
        let document = snapshot.document(file);
        let source = document.source();
        let empty = Vec::new();
        let checker_for_file = rows.get(&source.file_name).unwrap_or(&empty);
        let globals = outputs
            .iter()
            .flat_map(|output| ledger(output).globals_by_file[file].iter().cloned())
            .collect::<Vec<_>>();
        let ranges = outputs
            .iter()
            .flat_map(|output| {
                ledger(output)
                    .partially_checked_ranges
                    .get(&file)
                    .into_iter()
                    .flatten()
                    .copied()
            })
            .collect::<Vec<_>>();
        semantic_diagnostics_for_file_rows(
            source,
            &document.data.bind_diagnostics,
            checker_for_file,
            (!ranges.is_empty()).then_some(ranges.as_slice()),
            &globals,
            program_diagnostics,
            options,
        )
    };

    // Fixture projection: the public per-file getters as observed after the
    // fixture pass and before library completion.
    for file in lib_count..file_count {
        if skip(file) {
            continue;
        }
        let source = snapshot.document(file).source();
        let result_index = file - lib_count;
        if let Some(rows) = fixture_rows.get(&source.file_name) {
            file_diagnostics[result_index].suggestion.extend(
                rows.iter()
                    .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Suggestion)
                    .map(|diagnostic| (*diagnostic).clone()),
            );
        }
        file_diagnostics[result_index].semantic =
            assemble(file, &fixture_rows, |output| &output.fixture);
    }

    // Whole-Program getter: every file after completion, one stable
    // sort/dedupe after flattening (getDiagnosticsHelper's undefined arm).
    let mut diagnostics = Vec::new();
    for file in 0..file_count {
        if skip(file) {
            continue;
        }
        diagnostics.extend(assemble(file, &complete_rows, |output| &output.complete));
    }
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
    // W2e: a display-class observation counts only when the diagnostic it
    // formatted is actually published (whole-Program list or a per-file
    // projection); a discarded elaboration marks nothing.
    let mut marks = crate::order_guard::DisplayMarks::default();
    for output in &outputs {
        marks.extend(&output.display_marks);
    }
    if !marks.is_empty() {
        let published_marked = diagnostics
            .iter()
            .chain(
                file_diagnostics
                    .iter()
                    .flat_map(|file| file.semantic.iter().chain(file.suggestion.iter())),
            )
            .any(|row| marks.is_marked(row));
        if published_marked && replay_on_display_marks {
            return Err(marks.reasons());
        }
    }
    let partial_checks = outputs
        .iter()
        .flat_map(|output| output.partial_check_records.iter().cloned())
        .collect::<Vec<_>>();
    Ok(CheckExecution {
        result: assemble_check_result(
            &file_diagnostics,
            Some(&diagnostics),
            &global_diagnostics,
            &partial_checks,
            work_counters,
        ),
        authoritative_failure,
    })
}

/// Whether the run writes declaration files: `declaration` or `composite`
/// (which implies it), including `emitDeclarationOnly`. Type printing in
/// `.d.ts` output observes the shard-local type-id order, so such a run keeps
/// a scheduling-independent file-to-shard assignment.
pub fn declaration_output_requested(options: &CompilerOptions) -> bool {
    options.declaration == Some(true)
        || options.composite == Some(true)
        || options.emit_declaration_only == Some(true)
}

/// The sharded driver: stages 1–2 once (parse/adopt, bind, snapshot), then
/// one checker state per shard on scoped threads over the shared immutable
/// snapshot, then the Program-order merge. Equivalent to the serial driver's
/// fixture projection and whole-Program assembly. With `sharded_emit`, every
/// shard keeps its checker alive after reporting and emits its own files once
/// the coordinator has gated the merged diagnostics (tsgo's per-checker emit).
/// tsrs-native: tsgo checker pool shape; tsc has one checker.
#[allow(clippy::too_many_arguments)]
fn check_program_with_prebound_libs_sharded<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    lib_documents: &[Arc<BoundDocument>],
    identity_domain: &IdentityDomain,
    mut work_counters: CheckWorkCounters,
    collect_global_diagnostics: bool,
    observe_phase: &mut impl FnMut(CheckPhase),
    run: &AuthoritativeRun<'_>,
    factory: &dyn AuthoritativeModuleProviderFactory,
    lib_facts: ProgramFileFacts,
    workers: WorkerBudget,
    checkers: CheckerBudget,
    mut sharded_emit: Option<&mut ShardedEmit<'_>>,
) -> CheckExecution {
    let current_directory = current_directory.into();
    let phase_started = std::time::Instant::now();
    let ParsedProgramInputs {
        program_sources,
        authoritative_program_metadata,
        program_diagnostics,
        host,
    } = parse_program_inputs(
        libs,
        files,
        options,
        current_directory,
        identity_domain,
        Some(run),
        &mut work_counters,
        workers,
    );
    tsc_types::trace::mark("checker: parse/adopt program sources", phase_started);

    let lib_count = lib_documents.len();
    let mut document_store = EphemeralDocumentStore::with_documents(
        identity_domain.clone(),
        lib_documents.iter().cloned(),
    );
    observe_phase(CheckPhase::Bind);
    let phase_started = std::time::Instant::now();
    let bind_data =
        bind_sources_in_program_order(&program_sources, options, identity_domain, workers);
    for (source_file, data) in program_sources.iter().zip(bind_data) {
        work_counters.record_bind();
        document_store
            .publish(Arc::clone(source_file), data)
            .expect("completed bind must belong to the ephemeral document domain");
    }
    tsc_types::trace::mark("checker: bind program sources", phase_started);
    observe_phase(CheckPhase::Check);

    if lib_documents.is_empty() && program_sources.is_empty() {
        // Same observable result as the serial driver for an empty Program:
        // the whole-Program getter exists and is empty.
        let global_diagnostics = if collect_global_diagnostics {
            globals::missing_init_global_type_diagnostics(options)
        } else {
            Vec::new()
        };
        return CheckExecution {
            result: assemble_check_result(&[], Some(&[]), &global_diagnostics, &[], work_counters),
            authoritative_failure: None,
        };
    }
    let phase_started = std::time::Instant::now();

    let mut file_facts = vec![lib_facts; lib_count];
    file_facts.resize(
        lib_count + program_sources.len(),
        ProgramFileFacts::ORDINARY,
    );
    let snapshot = document_store
        .into_snapshot_with_file_facts(file_facts)
        .expect("program snapshot identity allocation failed");
    report_program_memory(&snapshot);
    let file_diagnostics = syntactic_file_rows(&snapshot, lib_count, options);
    let mut metadata = run.lib_metadata.clone();
    metadata.extend(authoritative_program_metadata.iter().cloned());
    let complete_library_prefix = run.library_prefix == LibraryPrefixCompletion::Complete;

    // Every Program file (library prefix included) goes through the shard
    // file queue. Shared Program-order pulling (tsgo's checker pool) lets a
    // shard on a slower core take fewer files instead of finishing last; but
    // the phases after the check — the declaration diagnostics gate and the
    // emit — run each file on the checker that checked it, so an uneven
    // split lengthens them, and with declaration emit they outweigh the
    // check. With fewer than four fixtures per shard there is little to
    // balance, and the node-count partition spreads the library pass from
    // the start instead of after the last fixture.
    // Declaration output prints types in shard-local type-id order, so a run
    // that writes `.d.ts` files keeps a static file-to-shard assignment that
    // nothing rebalances later (see `declaration_output_requested`).
    let static_declaration_partition =
        sharded_emit.is_some() && declaration_output_requested(options);
    let weights = snapshot
        .documents()
        .iter()
        .enumerate()
        .map(|(index, document)| {
            let source = document.source();
            // A JSON source is admitted for its module shape and checked in
            // constant time; its node count would otherwise claim a share.
            // The static declaration partition also weighs a file whose
            // check is skipped (`skipLibCheck` on a declaration file, the
            // default libraries under `skipDefaultLibCheck`) as constant:
            // the shard neither checks nor emits it, and zod's node-count
            // split gave one shard 165 such files and no checking while
            // another checked for 842 ms. A stealing run keeps the
            // node-count weights of round 9, whose measured assignment the
            // skipped files' weights are part of.
            let skipped = static_declaration_partition
                && should_skip_type_checking_file(
                    source,
                    snapshot.file_facts(ProgramFileId::from_raw(
                        u32::try_from(index).expect("program file index"),
                    )),
                    options,
                );
            if skipped || source.file_name.as_js().ends_with(".json") {
                1
            } else {
                source.arena.len()
            }
        })
        .collect::<Vec<_>>();
    // A large --noEmit check pulls from the shared queue in chunks: the
    // split between shards then follows the measured check times (mixed
    // cores, cost the node count does not predict) while a chunk keeps a
    // directory's files together. Otherwise the deterministic partition
    // (least load by node count with a directory preference): a run's
    // shard-local type order then repeats run to run, as tsgo's does, and
    // the emit phases run each file on the checker that checked it.
    // `TSRS_SHARD_QUEUE` overrides the choice.
    let checker_count = if static_declaration_partition {
        checkers.checkers_for_static_partition()
    } else {
        checkers.checkers()
    };
    // A program dominated by one heavy file (the TypeScript compiler's
    // checker.ts) gains nothing from shards beyond `total / heaviest`: the
    // check ends with that file's check either way, and every extra shard
    // repeats the lazy library-type work and contends with the critical
    // shard. An explicit `TSRS_CHECKERS` count is kept as requested.
    let checker_count = if checkers.is_automatic() {
        let checked_weight = |index: usize, document: &Arc<BoundDocument>| {
            let source = document.source();
            if index < lib_count
                || should_skip_type_checking_file(
                    source,
                    snapshot.file_facts(ProgramFileId::from_raw(
                        u32::try_from(index).expect("program file index"),
                    )),
                    options,
                )
            {
                0
            } else {
                source.arena.len()
            }
        };
        let (total, heaviest) = snapshot
            .documents()
            .iter()
            .enumerate()
            .map(|(index, document)| checked_weight(index, document))
            .fold((0usize, 0usize), |(total, heaviest), weight| {
                (total + weight, heaviest.max(weight))
            });
        checker_count
            .min(shard::dominant_file_shard_cap(total, heaviest))
            .max(1)
    } else {
        checker_count
    };
    // Each file's directory, interned, for the directory-preferring
    // partition (files of one directory share their imports).
    let mut directory_ids: rustc_hash::FxHashMap<&[u8], u32> = Default::default();
    let directories = snapshot
        .documents()
        .iter()
        .map(|document| {
            let name = document.source().file_name.as_js();
            let bytes = name.as_bytes();
            let end = bytes.iter().rposition(|&byte| byte == b'/').unwrap_or(0);
            let next = directory_ids.len() as u32;
            *directory_ids.entry(&bytes[..end]).or_insert(next)
        })
        .collect::<Vec<u32>>();
    let symbols = snapshot
        .documents()
        .iter()
        .map(|document| document.data.symbols.len())
        .collect::<Vec<usize>>();
    let fixtures = weights.len().saturating_sub(lib_count);
    let shared_chunk = match shard::queue_mode_requested() {
        Some(shard::QueueMode::Shared(chunk)) => Some(chunk),
        Some(shard::QueueMode::Partition) => None,
        None => (sharded_emit.is_none() && fixtures >= shard::SHARED_QUEUE_MIN_FIXTURES)
            .then_some(shard::DEFAULT_SHARED_CHUNK),
    };
    let queue = if let Some(chunk) = shared_chunk {
        shard::ShardFileQueue::shared_chunked(lib_count, &weights, checker_count, chunk)
    } else {
        // A large program balances its shares by stealing; a small one
        // keeps the static shares, so its run-to-run type order (the
        // relaxed sharded mode's determinism) does not depend on timing.
        // Declaration output prints types in type-id order, so a run
        // that writes `.d.ts` files keeps the static shares at any size:
        // its output then repeats run to run, as tsgo's does.
        let steal = shard::stealing_enabled()
            && fixtures >= shard::STEAL_MIN_FIXTURES
            && !static_declaration_partition;
        // The declaration files a stealing lane checks first.
        let checked_declarations = if steal {
            snapshot
                .documents()
                .iter()
                .enumerate()
                .map(|(index, document)| {
                    let source = document.source();
                    source.is_declaration_file
                        && !should_skip_type_checking_file(
                            source,
                            snapshot.file_facts(ProgramFileId::from_raw(
                                u32::try_from(index).expect("program file index"),
                            )),
                            options,
                        )
                })
                .collect::<Vec<bool>>()
        } else {
            Vec::new()
        };
        shard::ShardFileQueue::partitioned_with_directories(
            lib_count,
            &weights,
            &symbols,
            &directories,
            checker_count,
            steal,
            &checked_declarations,
        )
    };
    let shard_count = queue.shard_count();

    // An owned copy for the serial replay (W2c), taken before any shard slot
    // exists: the replay never reconstructs host facts from admitted sources
    // (ignored inputs and package.json manifests matter).
    let replay_host = host.clone();
    // One HostFacts per shard in take-once slots: a shard takes its slot on
    // whichever thread runs it, so a refused spawn leaves the facts for the
    // coordinator's fallback. The original moves into slot 0.
    let mut hosts = Vec::with_capacity(shard_count);
    for _ in 1..shard_count {
        hosts.push(std::sync::Mutex::new(Some(host.clone())));
    }
    hosts.insert(0, std::sync::Mutex::new(Some(host)));
    let emit_closure: Option<ShardEmitClosure<'_>> = sharded_emit.as_ref().map(|emit| emit.emit);
    let eager_closure: Option<ShardEagerClosure<'_>> =
        sharded_emit.as_ref().and_then(|emit| emit.eager);
    let prelude_closure: Option<ShardPreludeClosure<'_>> =
        sharded_emit.as_ref().and_then(|emit| emit.prelude);
    let coordinate = emit_closure.is_some();
    // One provider per shard, owned here so a checked state (which borrows
    // its provider) can outlive its shard's thread for the emit pool.
    let providers = (0..shard_count)
        .map(|_| factory.provider())
        .collect::<Vec<_>>();
    // Shards still checking, read by every shard's eager emit.
    let checking = std::sync::atomic::AtomicUsize::new(shard_count);
    let run_shard = |shard_index: usize| {
        let host = hosts[shard_index]
            .lock()
            .expect("host facts slot")
            .take()
            .expect("each shard takes its host facts once");
        run_checker_shard(
            shard_index,
            &snapshot,
            options,
            &*providers[shard_index],
            &metadata,
            host,
            &queue,
            queue.reserved_nodes(shard_index),
            complete_library_prefix,
            coordinate,
            checkers.leaks_states(),
            checkers.order_replay(),
            eager_closure,
            &checking,
        )
    };
    #[allow(clippy::large_enum_variant)]
    enum ShardedRun {
        Merged(CheckExecution),
        Replay(u32),
    }
    let sharded = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(shard_count);
        let mut results: Vec<Option<(ShardOutput, Option<state::CheckerState<'_>>)>> =
            (0..shard_count).map(|_| None).collect();
        // The syntax-only prelude (emit copies of the largest sources) runs
        // beside the shards; a refused thread just leaves the emit to prepare
        // its copies itself.
        if let Some(prelude) = prelude_closure {
            let snapshot = &snapshot;
            let _ = std::thread::Builder::new()
                .name("tsc-rs-emit-prelude".to_owned())
                .stack_size(tsc_program::WORKER_STACK_BYTES)
                .spawn_scoped(scope, move || {
                    tsc_program::run_thread_start_hook();
                    prelude(snapshot);
                });
        }
        // The coordinator runs shard 0 itself; the others run on scoped
        // threads. A refused thread is not an error: the coordinator runs
        // that shard too, with the untouched host facts of its slot.
        for (shard_index, slot) in results.iter_mut().enumerate().skip(1) {
            match std::thread::Builder::new()
                .name(format!("tsc-rs-checker-{shard_index}"))
                .stack_size(tsc_program::WORKER_STACK_BYTES)
                .spawn_scoped(scope, move || {
                    tsc_program::run_thread_start_hook();
                    run_shard(shard_index)
                }) {
                Ok(handle) => handles.push((shard_index, handle)),
                Err(_) => *slot = Some(run_shard(shard_index)),
            }
        }
        results[0] = Some(run_shard(0));
        for (shard_index, handle) in handles {
            results[shard_index] = Some(
                handle
                    .join()
                    .unwrap_or_else(|payload| std::panic::resume_unwind(payload)),
            );
        }
        let mut outputs = Vec::with_capacity(shard_count);
        let mut states = Vec::with_capacity(shard_count);
        for result in results {
            let (output, state) = result.expect("every shard reports exactly once");
            outputs.push(output);
            states.extend(state);
        }
        // The files each shard actually checked, in Program order: the emit
        // gate and the per-shard emit query the resolver of the shard that
        // checked a file.
        let assignment = outputs
            .iter()
            .map(|output| output.files.clone())
            .collect::<Vec<_>>();
        tsc_types::trace::mark("checker: shards checked", phase_started);
        let phase_started = std::time::Instant::now();
        let dispose_states = |states: Vec<state::CheckerState<'_>>| {
            if checkers.leaks_states() {
                // The one-shot process exits right after publishing.
                for state in states {
                    std::mem::forget(state);
                }
            }
        };
        // W2c: a shard that consumed the order of two shard-local types may
        // have diverged from the serial checker (text or semantics); discard
        // every shard result and replay the whole check serially over the
        // same snapshot (parse and bind are not repeated).
        let mut order_reasons = outputs
            .iter()
            .fold(0u32, |acc, output| acc | output.order_reasons);
        // The guard's exemption for pre-guard ids assumes every shard ran the
        // same deterministic initialization; verify it instead of assuming.
        if outputs
            .iter()
            .any(|output| output.init_boundary != outputs[0].init_boundary)
        {
            debug_assert!(false, "shard initialization type counts differ");
            order_reasons |= crate::order_guard::OrderReason::INIT_DIVERGENCE.bits();
        }
        let threads = outputs
            .iter()
            .map(|output| output.thread)
            .collect::<rustc_hash::FxHashSet<_>>();
        work_counters.record_checker_shards(outputs.len() as u64, threads.len() as u64);
        perf::add(PerfCounter::CheckerShardsRun, outputs.len() as u64);
        perf::add(PerfCounter::CheckerShardThreads, threads.len() as u64);
        let replay_on_order = checkers.order_replay();
        if order_reasons != 0 {
            work_counters.record_order_reasons(order_reasons);
            if replay_on_order {
                drop(outputs);
                dispose_states(states);
                return ShardedRun::Replay(order_reasons);
            }
        }
        // W2e: the merge itself decides whether a display-class mark reached
        // a published row; only then is the sharded result discarded.
        let execution = match merge_shard_outputs(
            &snapshot,
            lib_count,
            options,
            &program_diagnostics,
            file_diagnostics.clone(),
            outputs,
            collect_global_diagnostics,
            work_counters,
            replay_on_order,
        ) {
            Ok(execution) => execution,
            Err(marked_reasons) => {
                dispose_states(states);
                return ShardedRun::Replay(marked_reasons);
            }
        };
        tsc_types::trace::mark("checker: merge shard diagnostics", phase_started);
        let Some(sharded_emit) = sharded_emit.as_deref_mut() else {
            dispose_states(states);
            return ShardedRun::Merged(execution);
        };
        if execution.authoritative_failure.is_some() {
            dispose_states(states);
            return ShardedRun::Merged(execution);
        }
        if states.len() != shard_count {
            dispose_states(states);
            return ShardedRun::Replay(crate::order_guard::OrderReason::INIT_DIVERGENCE.bits());
        }
        // Every checked state becomes a session before the gate: the
        // coordinator's noEmitOnError declaration gate queries each shard's
        // resolver, and its emit pool then runs each planned unit against the
        // resolver of the shard that checked it. Both may create shard-local
        // types (declaration rendering, lazy resolver queries): an
        // order-consuming operation or a new display mark discards the
        // products in favour of the serial replay.
        let marks_before = states
            .iter()
            .map(|state| state.order_guard.marks().len())
            .collect::<Vec<_>>();
        let sessions = states
            .into_iter()
            .map(|state| {
                CheckerSession::from_checked_state(state).with_program_diagnostics(
                    program_diagnostics.clone(),
                    execution.result.program_semantic_diagnostics.clone(),
                )
            })
            .collect::<Vec<_>>();
        let phase_started = std::time::Instant::now();
        let admitted = (sharded_emit.gate)(&snapshot, &execution.result, &sessions, &assignment);
        tsc_types::trace::mark("checker: emit gate", phase_started);
        let phase_started = std::time::Instant::now();
        let emitted = admitted.then(|| (sharded_emit.emit)(&snapshot, &sessions, &assignment));
        if admitted {
            tsc_types::trace::mark("checker: shards emitted", phase_started);
        }
        let mut emit_reasons = 0u32;
        let mut states = Vec::with_capacity(sessions.len());
        for (session, before) in sessions.into_iter().zip(marks_before) {
            let state = session.into_state();
            emit_reasons |= state.order_guard.reasons();
            if state.order_guard.marks().len() > before {
                emit_reasons |= crate::order_guard::OrderReason::INIT_DIVERGENCE.bits();
            }
            states.push(state);
        }
        dispose_states(states);
        if emit_reasons != 0 && replay_on_order {
            return ShardedRun::Replay(emit_reasons);
        }
        if let Some(emitted) = emitted {
            sharded_emit.emissions = Some(emitted);
        }
        ShardedRun::Merged(execution)
    });
    let replay_reasons = match sharded {
        ShardedRun::Merged(execution) => {
            if checkers.leaks_states() {
                // The one-shot process exits right after publishing: the
                // shared documents go with the leaked checker states.
                std::mem::forget(snapshot);
            }
            return execution;
        }
        ShardedRun::Replay(reasons) => reasons,
    };
    work_counters.record_serial_replay(replay_reasons);
    perf::add(PerfCounter::CheckerSerialReplays, 1);
    crate::order_guard::count_replay_reasons(replay_reasons);
    let replay_started = std::time::Instant::now();
    if tsc_types::trace::enabled() {
        tsc_types::trace::mark(
            &format!("checker: serial replay selected (order guard reasons {replay_reasons:#x})"),
            replay_started,
        );
    }
    drop(hosts);
    let provider = factory.provider();
    let execution = check_snapshot_serially(
        &snapshot,
        lib_count,
        options,
        &program_diagnostics,
        file_diagnostics,
        Some((&*provider, metadata.as_slice())),
        replay_host,
        collect_global_diagnostics,
        complete_library_prefix,
        work_counters,
        sharded_emit,
        checkers.leaks_states(),
    );
    drop(provider);
    if checkers.leaks_states() {
        std::mem::forget(snapshot);
    }
    execution
}

/// The serial check over an existing snapshot (stages 3–5 of the serial
/// driver, eager schedule). The serial driver delegates its authoritative
/// eager no-emit path here and the sharded driver replays a flagged run
/// through it, so the two cannot drift in check order or assembly; the legacy
/// fixture-only, on-demand and emit-callback paths keep the serial driver's
/// inline sequence. A replayed emitting run emits every file with this one
/// checker through the same per-shard protocol.
#[allow(clippy::too_many_arguments)]
fn check_snapshot_serially(
    snapshot: &ProgramSnapshot,
    lib_count: usize,
    options: &CompilerOptions,
    program_diagnostics: &[Diagnostic],
    mut file_diagnostics: Vec<FileDiagnosticPasses>,
    authoritative: Option<(
        &dyn AuthoritativeModuleProvider,
        &[AuthoritativeSourceMetadata],
    )>,
    host: HostFacts,
    collect_global_diagnostics: bool,
    complete_library_prefix: bool,
    work_counters: CheckWorkCounters,
    sharded_emit: Option<&mut ShardedEmit<'_>>,
    leak_state: bool,
) -> CheckExecution {
    let mut state = init_checker_state(snapshot, options, authoritative, host);
    reserve_type_tables(&mut state, snapshot_node_count(snapshot));
    let global_diagnostics = if collect_global_diagnostics {
        let mut rows = state.visible_global_diagnostics.clone();
        tsc_diagnostics::sort_and_dedupe_diagnostics(&mut rows);
        rows
    } else {
        Vec::new()
    };
    let program_file_ids = state.binder.file_ids().skip(lib_count).collect::<Vec<_>>();
    let mut global_checker_diagnostics_by_file = vec![Vec::new(); state.binder.file_count()];
    let check_started = std::time::Instant::now();
    check_files_in_order(
        &mut state,
        &program_file_ids,
        &mut global_checker_diagnostics_by_file,
    );
    if tsc_types::trace::enabled() {
        tsc_types::trace::mark(
            &format!(
                "serial: check ({} types, {} symbol links, {} files)",
                state.tables.len(),
                state.links.symbol_len(),
                program_file_ids.len()
            ),
            check_started,
        );
    }
    state.report_memory("serial");
    for &file in &program_file_ids {
        if state.skip_type_checking_file(file) {
            continue;
        }
        let source_index = file.index();
        let result_index = source_index - lib_count;
        let source = state.binder.source(source_index);
        file_diagnostics[result_index].suggestion.extend(
            state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.as_ref().map(JsString::as_js)
                        == Some(source.file_name.as_js())
                        && diagnostic.category() == DiagnosticCategory::Suggestion
                })
                .cloned(),
        );
        file_diagnostics[result_index].semantic = semantic_diagnostics_for_program_file(
            &state,
            source_index,
            &global_checker_diagnostics_by_file[source_index],
            program_diagnostics,
            options,
        );
    }
    let all_program_file_ids = state.binder.file_ids().collect::<Vec<_>>();
    if complete_library_prefix {
        check_files_in_order(
            &mut state,
            &all_program_file_ids,
            &mut global_checker_diagnostics_by_file,
        );
    }
    let mut diagnostics = Vec::new();
    for &file in &all_program_file_ids {
        if state.skip_type_checking_file(file) {
            continue;
        }
        diagnostics.extend(semantic_diagnostics_for_program_file(
            &state,
            file.index(),
            &global_checker_diagnostics_by_file[file.index()],
            program_diagnostics,
            options,
        ));
    }
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
    let partial_checks = state.partial_check_records.clone();
    let authoritative_failure = state.take_authoritative_module_failure();
    state.line_profile.flush();
    let result = assemble_check_result(
        &file_diagnostics,
        Some(&diagnostics),
        &global_diagnostics,
        &partial_checks,
        work_counters,
    );
    let state = match sharded_emit {
        Some(sharded_emit) if authoritative_failure.is_none() => {
            let every_file = (0..all_program_file_ids.len()).collect::<Vec<_>>();
            let session = CheckerSession::from_checked_state(state).with_program_diagnostics(
                program_diagnostics.to_vec(),
                result.program_semantic_diagnostics.clone(),
            );
            if (sharded_emit.gate)(
                snapshot,
                &result,
                std::slice::from_ref(&session),
                std::slice::from_ref(&every_file),
            ) {
                let emitted = (sharded_emit.emit)(
                    snapshot,
                    std::slice::from_ref(&session),
                    std::slice::from_ref(&every_file),
                );
                sharded_emit.emissions = Some(emitted);
            }
            session.into_state()
        }
        _ => state,
    };
    if leak_state {
        std::mem::forget(state);
    }
    CheckExecution {
        result,
        authoritative_failure,
    }
}

#[allow(clippy::too_many_arguments)]
fn check_program_with_prebound_libs_at_observed<'cwd>(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
    current_directory: impl Into<JsStr<'cwd>>,
    lib_documents: &[Arc<BoundDocument>],
    identity_domain: &IdentityDomain,
    mut work_counters: CheckWorkCounters,
    collect_global_diagnostics: bool,
    observe_phase: &mut impl FnMut(CheckPhase),
    authoritative_run: Option<&AuthoritativeRun<'_>>,
    emit_operation: Option<&mut CheckedEmitOperation<'_>>,
    lib_facts: ProgramFileFacts,
    workers: WorkerBudget,
) -> CheckExecution {
    let current_directory = current_directory.into();
    let mut file_diagnostics = Vec::new();
    // An authoritative Program session exposes the whole-Program semantic
    // getter even when root filtering produces no SourceFiles (for example a
    // lone `.js` root with `allowJs` disabled). The observable getter result
    // is an empty list, not an absent capability; emit relies on that typed
    // distinction to execute the empty output plan without a checker state.
    let mut program_semantic_diagnostics = authoritative_run.is_some().then(Vec::new);
    let mut partial_checks = Vec::new();
    let mut global_diagnostics = Vec::new();
    let mut authoritative_failure = None;
    let ParsedProgramInputs {
        program_sources,
        authoritative_program_metadata,
        program_diagnostics,
        host,
    } = parse_program_inputs(
        libs,
        files,
        options,
        current_directory,
        identity_domain,
        authoritative_run,
        &mut work_counters,
        workers,
    );

    // The production H0 path publishes through a direct, session-owned store.
    // Library documents may already come from the separately authorized
    // harness cache, but fixture documents are never inserted into a global
    // map or kept past the ProgramSnapshot below.
    let lib_count = lib_documents.len();
    let mut document_store = EphemeralDocumentStore::with_documents(
        identity_domain.clone(),
        lib_documents.iter().cloned(),
    );

    // Fixture bind pass: each completed binder owns exact persistent-symbol
    // and private-name-serial leases in the source's identity domain.
    observe_phase(CheckPhase::Bind);

    let bind_data =
        bind_sources_in_program_order(&program_sources, options, identity_domain, workers);
    for (source_file, data) in program_sources.iter().zip(bind_data) {
        work_counters.record_bind();
        document_store
            .publish(Arc::clone(source_file), data)
            .expect("completed bind must belong to the ephemeral document domain");
    }

    // Checker-state construction (M4 5.0) + the check driver (M4 5.4):
    // initializeTypeChecker merges globals in Program order (default
    // libraries first), then the oracle-facing adapter requests diagnostics
    // for each FIXTURE source in Program order. Library documents remain
    // first-class Program members, but the oracle driver never calls
    // getSemanticDiagnostics(libSourceFile), so this adapter must not publish
    // their file diagnostics implicitly.
    observe_phase(CheckPhase::Check);

    if lib_documents.is_empty() && program_sources.is_empty() && collect_global_diagnostics {
        global_diagnostics = globals::missing_init_global_type_diagnostics(options);
    }
    if !lib_documents.is_empty() || !program_sources.is_empty() {
        // The store owns all published handles until this snapshot takes
        // ownership. This is the one-shot H0 adapter: dropping the consumed
        // ProgramSession drops the complete store and cannot leave a process
        // cache behind.
        // Prefix storage and diagnostic scheduling do not determine default
        // library membership. The legacy oracle prepends ordinary roots under
        // noLib; owned/authoritative Programs supply their catalog libraries.
        let mut file_facts = vec![lib_facts; lib_count];
        file_facts.resize(
            lib_count + program_sources.len(),
            ProgramFileFacts::ORDINARY,
        );
        let snapshot = document_store
            .into_snapshot_with_file_facts(file_facts)
            .expect("program snapshot identity allocation failed");
        report_program_memory(&snapshot);
        file_diagnostics = syntactic_file_rows(&snapshot, lib_count, options);
        let authoritative_metadata = authoritative_run.map(|run| {
            let mut metadata = run.lib_metadata.clone();
            metadata.extend(authoritative_program_metadata.iter().cloned());
            metadata
        });
        // A per-checker provider source reaching the serial driver
        // constructs exactly one provider for its one checker state.
        let checker_provider: Option<Box<dyn AuthoritativeModuleProvider + '_>> = authoritative_run
            .and_then(|run| match run.provider {
                AuthoritativeProviderSource::Shared(_) => None,
                AuthoritativeProviderSource::PerChecker(factory) => Some(factory.provider()),
            });
        // The authoritative eager no-emit path (the CLI's noEmit check and
        // the sharded driver's serial replay) shares one implementation.
        if let Some(run) = authoritative_run.filter(|run| {
            run.diagnostic_schedule == DiagnosticSchedule::Eager && emit_operation.is_none()
        }) {
            let provider: &dyn AuthoritativeModuleProvider = match run.provider {
                AuthoritativeProviderSource::Shared(provider) => provider,
                AuthoritativeProviderSource::PerChecker(_) => checker_provider
                    .as_deref()
                    .expect("per-checker provider constructed above"),
            };
            work_counters.record_checker_shards(1, 1);
            perf::add(PerfCounter::CheckerShardsRun, 1);
            perf::add(PerfCounter::CheckerShardThreads, 1);
            return check_snapshot_serially(
                &snapshot,
                lib_count,
                options,
                &program_diagnostics,
                file_diagnostics,
                Some((
                    provider,
                    authoritative_metadata
                        .as_deref()
                        .expect("authoritative metadata assembled above"),
                )),
                host,
                collect_global_diagnostics,
                run.library_prefix == LibraryPrefixCompletion::Complete,
                work_counters,
                None,
                false,
            );
        }
        let mut state = init_checker_state(
            &snapshot,
            options,
            authoritative_run
                .zip(authoritative_metadata.as_deref())
                .map(|(run, metadata)| {
                    let provider: &dyn AuthoritativeModuleProvider = match run.provider {
                        AuthoritativeProviderSource::Shared(provider) => provider,
                        AuthoritativeProviderSource::PerChecker(_) => checker_provider
                            .as_deref()
                            .expect("per-checker provider constructed above"),
                    };
                    (provider, metadata)
                }),
            host,
        );
        reserve_type_tables(&mut state, snapshot_node_count(&snapshot));
        work_counters.record_checker_shards(1, 1);
        perf::add(PerfCounter::CheckerShardsRun, 1);
        perf::add(PerfCounter::CheckerShardThreads, 1);
        let mut emit_operation = emit_operation;
        if authoritative_run
            .is_some_and(|run| run.diagnostic_schedule == DiagnosticSchedule::EagerAfterEmit)
        {
            // The operation's first call: the checker is initialized and no
            // source is checked. The state returns to the eager schedule
            // below with whatever the call resolved.
            let unchecked = assemble_check_result(
                &file_diagnostics,
                program_semantic_diagnostics.as_deref(),
                &global_diagnostics,
                &state.partial_check_records,
                work_counters,
            );
            let session = CheckerSession::from_checked_state(state)
                .with_program_diagnostics(program_diagnostics.clone(), None);
            emit_operation
                .as_deref_mut()
                .expect("the emit-first schedule requires a scoped consumer")(
                &snapshot, &session, &unchecked,
            );
            state = session.into_state();
        }
        if collect_global_diagnostics {
            global_diagnostics = state.visible_global_diagnostics.clone();
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut global_diagnostics);
        }
        if authoritative_run
            .is_some_and(|run| run.diagnostic_schedule == DiagnosticSchedule::OnDemand)
        {
            let checked = assemble_check_result(
                &file_diagnostics,
                program_semantic_diagnostics.as_deref(),
                &global_diagnostics,
                &state.partial_check_records,
                work_counters,
            );
            if let Some(failure) = state.take_authoritative_module_failure() {
                return CheckExecution {
                    result: checked,
                    authoritative_failure: Some(failure),
                };
            }
            let session = CheckerSession::from_checked_state(state)
                .with_program_diagnostics(program_diagnostics.clone(), None);
            emit_operation.expect("on-demand initialization requires a scoped consumer")(
                &snapshot, &session, &checked,
            );
            let mut state = session.into_state();
            return CheckExecution {
                result: assemble_check_result(
                    &file_diagnostics,
                    program_semantic_diagnostics.as_deref(),
                    &global_diagnostics,
                    &state.partial_check_records,
                    work_counters,
                ),
                authoritative_failure: state.take_authoritative_module_failure(),
            };
        }

        // getDiagnosticsWorker snapshots global diagnostics around each
        // requested source. Only newly-published file-less rows are
        // prepended to that source's checker diagnostics. `program_file_ids`
        // models the exact source-file getter calls made by driver.mjs; the
        // library prefix participates in binding and global merge but is not
        // itself queried. A future arbitrary-query API can schedule any
        // ProgramFileId and use the same Program-aware skip policy.
        let program_file_ids = state.binder.file_ids().skip(lib_count).collect::<Vec<_>>();
        let mut global_checker_diagnostics_by_file = vec![Vec::new(); state.binder.file_count()];
        check_files_in_order(
            &mut state,
            &program_file_ids,
            &mut global_checker_diagnostics_by_file,
        );

        // Public per-file getter assembly. This deliberately does not
        // use a name-sorted map: the outer observation order is Program order.
        for &file in &program_file_ids {
            if state.skip_type_checking_file(file) {
                continue;
            }
            let source_index = file.index();
            let result_index = source_index - lib_count;
            let source = state.binder.source(source_index);
            let checker_for_file = state.diagnostics.iter().filter(|diagnostic| {
                diagnostic.file_name.as_ref().map(JsString::as_js) == Some(source.file_name.as_js())
            });

            // getSuggestionDiagnostics is a separate checker
            // collection and does not pass through
            // getDiagnosticsHelper. Preserve its collection order and
            // multiplicity exactly.
            file_diagnostics[result_index].suggestion.extend(
                checker_for_file
                    .clone()
                    .filter(|diagnostic| diagnostic.category() == DiagnosticCategory::Suggestion)
                    .cloned(),
            );
            file_diagnostics[result_index].semantic = semantic_diagnostics_for_program_file(
                &state,
                source_index,
                &global_checker_diagnostics_by_file[source_index],
                &program_diagnostics,
                options,
            );
        }

        // emitFilesAndReportErrors requests
        // program.getSemanticDiagnostics(undefined), which maps the same
        // per-file getter over *all* Program sources. The fixture projections
        // above remain the legacy oracle-driver observation. Production
        // authoritative sessions additionally complete the library prefix,
        // honoring the Program-owned default-library facts rather than
        // inferring ownership from a path or declaration suffix.
        if authoritative_run.is_some() {
            // Program.getSourceFiles() order is the ProgramBinder order:
            // default-library prefix first, followed by fixture sources.
            // Revisit the complete sequence rather than only the library
            // prefix. check_source_file is TypeChecked-guarded, so fixture
            // files already observed above are cached no-ops, while a
            // library check may still publish a diagnostic owned by a
            // declaration in a merged fixture symbol.
            let all_program_file_ids = state.binder.file_ids().collect::<Vec<_>>();
            // The completion pass is the expensive half: without
            // `skipDefaultLibCheck`, checking the standard library prefix
            // costs ~1s per program. `FixtureObservedOnly` sessions skip it
            // because their consumers read only the per-file projections
            // assembled above, which this pass cannot alter; the assembly
            // below still runs so the whole-Program view stays present,
            // carrying exactly the rows fixture checking already observed.
            if authoritative_run
                .is_some_and(|run| run.library_prefix == LibraryPrefixCompletion::Complete)
            {
                check_files_in_order(
                    &mut state,
                    &all_program_file_ids,
                    &mut global_checker_diagnostics_by_file,
                );
            }

            let mut diagnostics = Vec::new();
            // Reassemble every file from the completed diagnostic ledger.
            // Reusing the earlier fixture projection here would miss rows
            // added to a fixture-owned declaration while checking the
            // canonical declaration in an earlier library file (for example,
            // a merged interface's duplicate index signatures).
            for &file in &all_program_file_ids {
                if state.skip_type_checking_file(file) {
                    continue;
                }
                diagnostics.extend(semantic_diagnostics_for_program_file(
                    &state,
                    file.index(),
                    &global_checker_diagnostics_by_file[file.index()],
                    &program_diagnostics,
                    options,
                ));
            }
            // getDiagnosticsHelper's sourceFile === undefined arm performs
            // one stable whole-result sort/dedup after flattening.
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
            program_semantic_diagnostics = Some(diagnostics);
        }
        partial_checks = state.partial_check_records.clone();
        authoritative_failure = state.take_authoritative_module_failure();
        if authoritative_failure.is_none() {
            if let Some(operation) = emit_operation {
                let checked = assemble_check_result(
                    &file_diagnostics,
                    program_semantic_diagnostics.as_deref(),
                    &global_diagnostics,
                    &partial_checks,
                    work_counters,
                );
                let session = CheckerSession::from_checked_state(state).with_program_diagnostics(
                    program_diagnostics.clone(),
                    checked.program_semantic_diagnostics.clone(),
                );
                operation(&snapshot, &session, &checked);
            }
        }
    }

    CheckExecution {
        result: assemble_check_result(
            &file_diagnostics,
            program_semantic_diagnostics.as_deref(),
            &global_diagnostics,
            &partial_checks,
            work_counters,
        ),
        authoritative_failure,
    }
}

/// Assemble one `getSemanticDiagnosticsForFile` result after its checker pass
/// has run. Both the fixture getter projection and the all-Program getter use
/// this path so comment directives, plain-JS filtering, and semantic
/// filtering cannot drift between the two public observations.
fn semantic_diagnostics_for_program_file(
    state: &state::CheckerState<'_>,
    source_index: usize,
    global_checker_diagnostics: &[Diagnostic],
    program_diagnostics: &[Diagnostic],
    options: &CompilerOptions,
) -> DiagnosticList {
    let source = state.binder.source(source_index);
    let checker_for_file = state
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.file_name.as_ref().map(JsString::as_js) == Some(source.file_name.as_js())
        })
        .collect::<Vec<_>>();
    semantic_diagnostics_for_file_rows(
        source,
        &state.binder.file(source_index).bind_diagnostics,
        &checker_for_file,
        state
            .partially_checked_ranges
            .get(&source_index)
            .map(Vec::as_slice),
        global_checker_diagnostics,
        program_diagnostics,
        options,
    )
}

/// The `getSemanticDiagnosticsForFile` assembly over an explicit row source:
/// the checker rows owned by `source` (in publication order, from one or
/// several checker states), the owner's partially-checked ranges, and the
/// file-less rows attributed to this file. The serial driver passes one
/// state's rows; the sharded driver passes the merged rows of every shard.
/// tsrs-native: shared assembly so the two drivers cannot drift.
#[allow(clippy::too_many_arguments)]
fn semantic_diagnostics_for_file_rows(
    source: &tsc_syntax::SourceFile,
    bind_diagnostics: &[Diagnostic],
    checker_for_file: &[&Diagnostic],
    partial_ranges: Option<&[(u32, u32)]>,
    global_checker_diagnostics: &[Diagnostic],
    program_diagnostics: &[Diagnostic],
    options: &CompilerOptions,
) -> DiagnosticList {
    let javascript_file = is_js_file_name(&source.file_name);
    let directive = check_directive(source.text());
    let plain_js = is_plain_js_file(javascript_file, directive, options);

    // getBindAndCheckDiagnosticsForFileNoCache:
    // bind -> check (new globals first) -> checked-JS JSDoc.
    let mut bind_and_check = Vec::new();
    bind_and_check.extend(bind_diagnostics.iter().cloned());
    bind_and_check.extend(
        global_checker_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.category() != DiagnosticCategory::Suggestion)
            .cloned(),
    );
    bind_and_check.extend(
        checker_for_file
            .iter()
            .filter(|diagnostic| diagnostic.category() != DiagnosticCategory::Suggestion)
            .map(|diagnostic| (*diagnostic).clone()),
    );
    if javascript_file && !plain_js {
        bind_and_check.extend(source.js_doc_diagnostics.iter().cloned());
    }

    if plain_js {
        bind_and_check.retain(|diagnostic| plain_js_errors::is_plain_js_error(diagnostic.code()));
    } else {
        let mut used_directive_lines = rustc_hash::FxHashSet::default();
        bind_and_check = filter_by_comment_directives_and_mark_used(
            source,
            bind_and_check.into_iter(),
            Some(&mut used_directive_lines),
        );
        if let Some(partial_ranges) = partial_ranges {
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

    // filterSemanticDiagnostics applies only to the bind/check half, before
    // getProgramDiagnostics is concatenated.
    filter_semantic_diagnostics(&mut bind_and_check, options);

    let mut program_for_file = program_diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.file_name.as_ref().map(JsString::as_js) == Some(source.file_name.as_js())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !source.comment_directives.is_empty() {
        // getProgramDiagnostics owns a fresh directive map; use in
        // bind/check does not consume this one.
        program_for_file =
            filter_by_comment_directives_and_mark_used(source, program_for_file.into_iter(), None);
    }
    bind_and_check.extend(program_for_file);

    // program.getSemanticDiagnostics(sourceFile) uses getDiagnosticsHelper.
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut bind_and_check);
    bind_and_check
}

fn assemble_check_result(
    file_diagnostics: &[FileDiagnosticPasses],
    program_semantic_diagnostics: Option<&[Diagnostic]>,
    global_diagnostics: &[Diagnostic],
    partial_checks: &[PartialCheck],
    work_counters: CheckWorkCounters,
) -> CheckResult {
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

    CheckResult {
        diagnostics,
        syntactic_diagnostics,
        semantic_diagnostics,
        program_semantic_diagnostics: program_semantic_diagnostics.map(|rows| rows.to_vec()),
        global_diagnostics: global_diagnostics.to_vec(),
        suggestion_diagnostics,
        file_diagnostics: file_diagnostics.to_vec(),
        partial_checks: partial_checks.to_vec(),
        work_counters,
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
/// Published bundles contain only Arc-owned ParsedDocument/BoundDocument
/// records; the temporary BinderWorker is consumed before the bundle is
/// inserted into the cache.
struct LibBundle {
    options: &'static CompilerOptions,
    documents: &'static [Arc<BoundDocument>],
    identity_domain: IdentityDomain,
}

/// A scoped parsed+bound library prefix for one or more harness sessions.
///
/// Unlike [`LibBundle`], this value is owned by the caller and is dropped with
/// the acceptance case. It exists for deterministic repeated observations:
/// the two isolated emit sessions can share the immutable library AST without
/// retaining every distinct `ts-tests` lib/options combination for the whole
/// process. The production checker never constructs this type.
#[doc(hidden)]
pub struct OwnedHarnessLibBundle {
    options: CompilerOptions,
    documents: Vec<Arc<BoundDocument>>,
    identity_domain: IdentityDomain,
}

impl OwnedHarnessLibBundle {
    fn exactly_matches(&self, libs: &[&InputFile], options: &CompilerOptions) -> bool {
        &self.options == options
            && self.documents.len() == libs.len()
            && self.documents.iter().zip(libs).all(|(document, lib)| {
                let source = document.source();
                source.file_name == lib.name && Arc::ptr_eq(source.snapshot(), lib.snapshot())
            })
    }
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
            && self.documents.len() == libs.len()
            && self.documents.iter().zip(libs).all(|(document, lib)| {
                let source = document.source();
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
    use rustc_hash::FxHashMap as HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    type Key = (Vec<(JsString, u64)>, CompilerOptions);
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
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::default()));
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
    let identity_domain = IdentityDomain::reclaiming();
    let (sources, _lib_work) =
        parse_lib_sources(libs, options, &identity_domain, WorkerBudget::serial());
    let binders = bind_lib_sources(&sources, options, &identity_domain, WorkerBudget::serial());
    let data = binders_into_data(binders);
    let sources = sources.into_iter().map(Arc::new).collect::<Vec<_>>();
    let documents = publish_bound_documents_from_handles(sources, data);
    let documents: &'static [Arc<BoundDocument>] = Box::leak(documents.into_boxed_slice());
    Box::leak(Box::new(LibBundle {
        options,
        documents,
        identity_domain,
    }))
}

fn build_owned_lib_bundle(libs: &[&InputFile], options: &CompilerOptions) -> OwnedHarnessLibBundle {
    let options = lib_bundle_options(options);
    let identity_domain = IdentityDomain::reclaiming();
    let (sources, _lib_work) =
        parse_lib_sources(libs, &options, &identity_domain, WorkerBudget::serial());
    let binders = bind_lib_sources(&sources, &options, &identity_domain, WorkerBudget::serial());
    let data = binders_into_data(binders);
    let sources = sources.into_iter().map(Arc::new).collect::<Vec<_>>();
    let documents = publish_bound_documents_from_handles(sources, data);
    OwnedHarnessLibBundle {
        options,
        documents,
        identity_domain,
    }
}

/// Materialize the library prefix: adopt each library's loader-planned tree
/// when it is provably the parse this session would perform, otherwise parse.
/// Returns the sources with the parse work actually performed.
fn parse_lib_sources(
    libs: &[&InputFile],
    options: &CompilerOptions,
    identity_domain: &IdentityDomain,
    workers: WorkerBudget,
) -> (Vec<tsc_syntax::SourceFile>, LibParseWork) {
    let mut work = LibParseWork::default();
    let mut pending: Vec<PendingProgramSource> = Vec::new();
    for lib in libs {
        let parse_options = tsc_syntax::ParseOptions {
            script_target: options.emit_script_target(),
            language_variant: tsc_syntax::LanguageVariant::Standard,
            javascript_file: false,
            force_external_module: false,
            detect_external_module_from_jsx: false,
            node_id_base: 0,
            node_array_id_base: 0,
            js_doc_parsing_mode: lib
                .js_doc_parsing_mode
                .unwrap_or(tsc_syntax::JSDocParsingMode::ParseAll),
        };
        pending.push(match lib.take_preparsed_source(&parse_options) {
            Some(source) => {
                work.adopted += 1;
                let (node_lease, array_lease) = source
                    .lease_identities(identity_domain)
                    .expect("library source identity allocation failed");
                PendingProgramSource::Leased(source, node_lease, array_lease)
            }
            None => {
                work.parsed += 1;
                PendingProgramSource::Ready(
                    tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                        lib.name.clone(),
                        Arc::clone(lib.snapshot()),
                        parse_options,
                        None,
                        identity_domain,
                    )
                    .expect("library source identity allocation failed"),
                )
            }
        });
    }
    let sources = workers.map_ordered(
        pending,
        PendingProgramSource::weight,
        PendingProgramSource::relocate,
    );
    (sources, work)
}

/// Bind the library prefix on the budget's workers at identities reserved
/// in order from `identity_domain` (see [`bind_reserved_in_program_order`]).
fn bind_lib_sources<'a>(
    sources: &'a [tsc_syntax::SourceFile],
    options: &'a CompilerOptions,
    identity_domain: &IdentityDomain,
    workers: WorkerBudget,
) -> Vec<tsc_binder::Binder<'a>> {
    let sources = sources.iter().collect::<Vec<_>>();
    bind_reserved_in_program_order(&sources, options, identity_domain, workers)
}

/// Reserve every source's symbol and private-name-serial ranges in order on
/// the calling thread (the order-dependent step), then bind each source on
/// the budget's workers directly at its reserved bases, so no bind identity
/// is rewritten afterwards. A source's ranges depend only on the sources
/// before it, so the identities are the same for every budget. A source
/// that outgrows a reservation (its node count bounds both spaces, so none
/// is expected) relocates into an exact lease at the domain's tail, still in
/// Program order.
fn bind_reserved_in_program_order<'a>(
    sources: &[&'a tsc_syntax::SourceFile],
    options: &'a CompilerOptions,
    identity_domain: &IdentityDomain,
    workers: WorkerBudget,
) -> Vec<tsc_binder::Binder<'a>> {
    let phase_started = std::time::Instant::now();
    let reservations = sources
        .iter()
        .map(|source| reserve_bind_identities(source, identity_domain))
        .collect::<Vec<_>>();
    tsc_types::trace::mark("checker: bind reservations (serial)", phase_started);
    let phase_started = std::time::Instant::now();
    let mut binders = workers.map_ordered(
        (0..sources.len()).collect(),
        |&index| sources[index].text().len(),
        |index| {
            let (symbol_lease, serial_lease) = &reservations[index];
            let mut binder = tsc_binder::Binder::bind_reserved(
                sources[index],
                options,
                symbol_lease.range().start(),
                serial_lease.range().start(),
            );
            let attached = binder
                .attach_reserved_leases(identity_domain, symbol_lease.clone(), serial_lease.clone())
                .expect("bind identity reservation failed");
            (binder, attached)
        },
    );
    tsc_types::trace::mark(
        "checker: bind (parallel, reserved identities)",
        phase_started,
    );
    let phase_started = std::time::Instant::now();
    let mut overflowed = 0usize;
    for (binder, attached) in &mut binders {
        if !*attached {
            binder
                .relocate_into_identity_domain(identity_domain)
                .expect("bind identity relocation failed");
            overflowed += 1;
        }
    }
    if tsc_types::trace::enabled() {
        tsc_types::trace::mark(
            &format!("checker: bind overflow relocation ({overflowed} sources)"),
            phase_started,
        );
    }
    binders.into_iter().map(|(binder, _)| binder).collect()
}

/// One symbol range and one private-name-serial range for `source`, leased
/// from the domain's bump tail in the caller's (Program) order. Every
/// persistent symbol and every serial is minted for a node of the source, so
/// its node count bounds both; a bind that still outgrows its range
/// relocates into an exact lease instead.
fn reserve_bind_identities(
    source: &tsc_syntax::SourceFile,
    identity_domain: &IdentityDomain,
) -> (IdentityLease, IdentityLease) {
    let bound = u32::try_from(source.arena.nodes().len())
        .expect("source node count exceeds u32")
        .max(1);
    let mut symbol = None;
    let mut serial = None;
    let leases = identity_domain
        .lease_batch(&[
            (tsc_types::IdentitySpace::Symbol, bound),
            (tsc_types::IdentitySpace::PrivateNameSerial, bound),
        ])
        .expect("bind identity reservation failed");
    for lease in leases {
        match lease.space() {
            tsc_types::IdentitySpace::Symbol => symbol = Some(lease),
            tsc_types::IdentitySpace::PrivateNameSerial => serial = Some(lease),
            space => panic!("unexpected bind reservation space {space}"),
        }
    }
    (
        symbol.expect("symbol reservation"),
        serial.expect("private-name serial reservation"),
    )
}

/// Bind every Program source on the budget's scoped worker threads (the
/// calling thread alone under a serial budget) at identities reserved in
/// Program order beforehand (see [`bind_reserved_in_program_order`]).
/// Binding is per-file (tsc's binder never reads another file), and the
/// reservations are the same for every budget.
fn bind_sources_in_program_order(
    sources: &[Arc<tsc_syntax::SourceFile>],
    options: &CompilerOptions,
    identity_domain: &IdentityDomain,
    workers: WorkerBudget,
) -> Vec<BindData> {
    let sources = sources.iter().map(Arc::as_ref).collect::<Vec<_>>();
    bind_reserved_in_program_order(&sources, options, identity_domain, workers)
        .into_iter()
        .map(tsc_binder::Binder::into_bind_data)
        .collect()
}

/// Consume completed bind workers into immutable document handles. The
/// worker borrow of the source/options ends at `into_bind_data`; snapshots
/// retain only the Arc-owned parsed source and checker-facing BindData.
fn binders_into_data(binders: Vec<tsc_binder::Binder<'_>>) -> Vec<BindData> {
    binders
        .into_iter()
        .map(tsc_binder::Binder::into_bind_data)
        .collect()
}

fn publish_bound_documents(
    sources: Vec<tsc_syntax::SourceFile>,
    data: Vec<BindData>,
) -> Vec<Arc<BoundDocument>> {
    assert_eq!(sources.len(), data.len());
    sources
        .into_iter()
        .zip(data)
        .map(|(source, data)| {
            let parsed = Arc::new(ParsedDocument::new(Arc::new(source)));
            Arc::new(BoundDocument::new(parsed, data))
        })
        .collect()
}

fn publish_bound_documents_from_handles(
    sources: Vec<Arc<tsc_syntax::SourceFile>>,
    data: Vec<BindData>,
) -> Vec<Arc<BoundDocument>> {
    assert_eq!(sources.len(), data.len());
    sources
        .into_iter()
        .zip(data)
        .map(|(source, data)| {
            let parsed = Arc::new(ParsedDocument::new(source));
            Arc::new(BoundDocument::new(parsed, data))
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_diagnostics::{Diagnostic, DiagnosticList};
use tsc_host::to_file_name_lower_case;
use tsc_types::CompilerOptions;

use crate::error::{PreparationError, PreparationErrorKind, PreparationOperation};
use crate::module_resolution::{is_external_module_name_relative, validate_owned_path_text};
use crate::path::{CanonicalPath, ProgramPath};
use crate::resolution::{
    MissingResolutionError, ModuleResolution, ResolutionError, ResolutionKey, ResolutionMode,
    ResolutionOutcome, ResolvedModuleTarget, TypeReferenceResolution, TypeReferenceResolutionKey,
    TypeReferenceResolutionOrigin,
};

const TYPESCRIPT_EXTENSIONLESS_SOURCE_PROBE_EXTENSIONS: [&str; 3] = [".ts", ".tsx", ".d.ts"];
const ALL_EXTENSIONLESS_SOURCE_PROBE_EXTENSIONS: [&str; 5] =
    [".ts", ".tsx", ".d.ts", ".js", ".jsx"];

pub(crate) const fn extensionless_source_probe_extensions(
    allow_js: bool,
) -> &'static [&'static str] {
    if allow_js {
        &ALL_EXTENSIONLESS_SOURCE_PROBE_EXTENSIONS
    } else {
        &TYPESCRIPT_EXTENSIONLESS_SOURCE_PROBE_EXTENSIONS
    }
}

/// Stable index into [`PreparedProgram::source_files`] in final
/// `createProgram` order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceFileId(u32);

impl SourceFileId {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Host path facts that remain observable after preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathContext {
    current_directory: ProgramPath,
    use_case_sensitive_file_names: bool,
}

impl PathContext {
    pub fn new(current_directory: ProgramPath, use_case_sensitive_file_names: bool) -> Self {
        Self {
            current_directory,
            use_case_sensitive_file_names,
        }
    }

    pub fn current_directory(&self) -> &ProgramPath {
        &self.current_directory
    }

    pub const fn use_case_sensitive_file_names(&self) -> bool {
        self.use_case_sensitive_file_names
    }
}

/// One decoded source owned by a prepared program.
///
/// `path` is the exact final program/`SourceFile` identity. For a resolved
/// dependency this may already be the physical `resolvedFileName`; its
/// distinct lexical spelling then remains on the resolution's
/// `original_path`. `real_path` is an explicit compatibility fact for a
/// producer that owns a lexical source plus a separately observed physical
/// alias. The recursive loader does not blanket-realpath roots or local
/// sources; resolver-owned external transitions use the first representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedSourceFile {
    path: ProgramPath,
    alternate_display_paths: Vec<PathBuf>,
    real_path: Option<ProgramPath>,
    text: String,
    may_be_emitted: bool,
    implied_node_format: Option<ResolutionMode>,
    implied_node_format_for_emit: Option<ResolutionMode>,
    package_scope: Option<CanonicalPath>,
}

impl PreparedSourceFile {
    pub fn new(path: ProgramPath, text: impl Into<String>) -> Self {
        let may_be_emitted = !path.display().to_str().is_some_and(|file_name| {
            file_name.ends_with(".d.ts")
                || file_name.ends_with(".d.cts")
                || file_name.ends_with(".d.mts")
                || (file_name.ends_with(".ts")
                    && file_name
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(file_name)
                        .contains(".d."))
        });
        Self {
            path,
            alternate_display_paths: Vec::new(),
            real_path: None,
            text: text.into(),
            // A prepared source is normally a direct program input. Loaders
            // that admit an external-library dependency retain that distinct
            // source-side fact with `with_may_be_emitted(false)`; resolution
            // provenance is deliberately not used as a substitute.
            may_be_emitted,
            implied_node_format: None,
            implied_node_format_for_emit: None,
            package_scope: None,
        }
    }

    /// Retain the physical host fact separately from the lexical program
    /// identity. The real path never replaces `path.canonical()` here.
    pub fn with_real_path(mut self, real_path: ProgramPath) -> Self {
        self.real_path = Some(real_path);
        self
    }

    /// tsrs-native: retain the program's `sourceFileMayBeEmitted` verdict for
    /// this source at the prepared-program boundary.
    ///
    /// This belongs to the selected source, not to any one resolution that
    /// reaches it. In particular, a root below `node_modules` can still be
    /// emit-eligible even when a package lookup records external-library
    /// resolution provenance.
    pub fn with_may_be_emitted(mut self, may_be_emitted: bool) -> Self {
        self.may_be_emitted = may_be_emitted;
        self
    }

    pub fn with_implied_node_format(mut self, mode: ResolutionMode) -> Self {
        self.implied_node_format = Some(mode);
        self.implied_node_format_for_emit = Some(mode);
        self
    }

    /// Retain both tsc's raw `SourceFile.impliedNodeFormat` and the effective
    /// value from `getImpliedNodeFormatForEmitWorker`. They differ when a
    /// default CommonJS implication falls back to a non-Node emit module kind.
    pub fn with_implied_node_formats(
        mut self,
        implied: Option<ResolutionMode>,
        for_emit: Option<ResolutionMode>,
    ) -> Self {
        self.implied_node_format = implied;
        self.implied_node_format_for_emit = for_emit;
        self
    }

    pub fn with_package_scope(mut self, package_json: CanonicalPath) -> Self {
        self.package_scope = Some(package_json);
        self
    }

    pub fn path(&self) -> &ProgramPath {
        &self.path
    }

    /// Alternate spellings that collapsed to this canonical identity, in
    /// discovery order. These are retained for casing diagnostics.
    pub fn alternate_display_paths(&self) -> &[PathBuf] {
        &self.alternate_display_paths
    }

    pub fn real_path(&self) -> Option<&ProgramPath> {
        self.real_path.as_ref()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// tsrs-native: expose the retained source-side emit-eligibility fact.
    pub const fn may_be_emitted(&self) -> bool {
        self.may_be_emitted
    }

    pub const fn implied_node_format(&self) -> Option<ResolutionMode> {
        self.implied_node_format
    }

    pub const fn implied_node_format_for_emit(&self) -> Option<ResolutionMode> {
        self.implied_node_format_for_emit
    }

    pub fn package_scope(&self) -> Option<&CanonicalPath> {
        self.package_scope.as_ref()
    }

    fn compatible_with(&self, other: &Self) -> bool {
        self.text == other.text
            && self.may_be_emitted == other.may_be_emitted
            && self.implied_node_format == other.implied_node_format
            && self.implied_node_format_for_emit == other.implied_node_format_for_emit
            && self.package_scope == other.package_scope
            && canonical_of(self.real_path.as_ref()) == canonical_of(other.real_path.as_ref())
    }

    pub(crate) fn remember_display_alias(&mut self, display: &Path) {
        if display != self.path.display()
            && !self
                .alternate_display_paths
                .iter()
                .any(|existing| existing == display)
        {
            self.alternate_display_paths.push(display.to_path_buf());
        }
    }
}

fn canonical_of(path: Option<&ProgramPath>) -> Option<&CanonicalPath> {
    path.map(ProgramPath::canonical)
}

// Keep diagnostic-source ownership aligned with
// `FormatDiagnosticsHost::file_text`: both compare exact spellings first and
// otherwise normalize only slash direction.
fn diagnostic_file_names_equal(left: &str, right: &str) -> bool {
    left == right || left.replace('\\', "/") == right.replace('\\', "/")
}

/// One configured or command-line root name in its original order.
///
/// A loaded extensionless root keeps the requested path here while `source`
/// identifies the selected first-group `.ts`/`.tsx`/`.d.ts` (and, under
/// `allowJs`, `.js`/`.jsx`) candidate.
///
/// A missing root is retained together with the exact tsc
/// program-construction diagnostic that also appears in
/// [`PreparationDiagnostics::program`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRoot {
    path: ProgramPath,
    source: Option<SourceFileId>,
    missing_diagnostic: Option<Diagnostic>,
}

/// Decoded text for a config or other non-program file that may own a
/// located preparation diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAuxiliaryFile {
    path: ProgramPath,
    alternate_display_paths: Vec<PathBuf>,
    text: String,
}

impl PreparedAuxiliaryFile {
    pub fn new(path: ProgramPath, text: impl Into<String>) -> Self {
        Self {
            path,
            alternate_display_paths: Vec::new(),
            text: text.into(),
        }
    }

    pub fn path(&self) -> &ProgramPath {
        &self.path
    }

    pub fn alternate_display_paths(&self) -> &[PathBuf] {
        &self.alternate_display_paths
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn remember_display_alias(&mut self, display: &Path) {
        if display != self.path.display()
            && !self
                .alternate_display_paths
                .iter()
                .any(|existing| existing == display)
        {
            self.alternate_display_paths.push(display.to_path_buf());
        }
    }
}

impl PreparedRoot {
    pub fn loaded(path: ProgramPath, source: SourceFileId) -> Self {
        Self {
            path,
            source: Some(source),
            missing_diagnostic: None,
        }
    }

    pub fn missing(path: ProgramPath, diagnostic: Diagnostic) -> Self {
        Self {
            path,
            source: None,
            missing_diagnostic: Some(diagnostic),
        }
    }

    pub fn path(&self) -> &ProgramPath {
        &self.path
    }

    pub const fn source(&self) -> Option<SourceFileId> {
        self.source
    }

    pub fn missing_diagnostic(&self) -> Option<&Diagnostic> {
        self.missing_diagnostic.as_ref()
    }
}

fn extensionless_root_text(path: &CanonicalPath) -> Option<&str> {
    let text = path
        .as_path()
        .to_str()
        .expect("canonical program paths are representable");
    (!text
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|base_name| base_name.contains('.')))
    .then_some(text)
}

fn extensionless_root_source_index<'root>(
    root: &'root CanonicalPath,
    source: &CanonicalPath,
    allow_js: bool,
) -> Option<(&'root str, usize)> {
    let root_text = extensionless_root_text(root)?;
    let source_text = source.as_path().to_str()?;
    let extension = source_text.strip_prefix(root_text)?;
    extensionless_source_probe_extensions(allow_js)
        .iter()
        .position(|candidate| *candidate == extension)
        .map(|index| (root_text, index))
}

/// Parsed `package.json` module-type fact retained with its source metadata.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum PackageJsonType {
    Module,
    CommonJs,
    Other,
    #[default]
    Unspecified,
}

/// Package metadata needed to reproduce package scope and package identities.
///
/// The decoded JSON text remains owned so later H0 resolver slices can add
/// fields such as `exports`, `imports`, and `typesVersions` without rereading
/// the host or losing the exact source spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageMetadata {
    package_json: ProgramPath,
    alternate_display_paths: Vec<PathBuf>,
    text: String,
    name: Option<String>,
    version: Option<String>,
    module_type: PackageJsonType,
}

impl PackageMetadata {
    pub fn new(package_json: ProgramPath, text: impl Into<String>) -> Self {
        Self {
            package_json,
            alternate_display_paths: Vec::new(),
            text: text.into(),
            name: None,
            version: None,
            module_type: PackageJsonType::Unspecified,
        }
    }

    /// Construct metadata from facts produced by the exact package-JSON
    /// parser. H0.1c does not parse JSON or attempt to re-derive these facts;
    /// the H0.2 loader owns that validation boundary.
    pub fn from_trusted_parsed(
        package_json: ProgramPath,
        text: impl Into<String>,
        name: Option<String>,
        version: Option<String>,
        module_type: PackageJsonType,
    ) -> Self {
        Self {
            package_json,
            alternate_display_paths: Vec::new(),
            text: text.into(),
            name,
            version,
            module_type,
        }
    }

    pub fn package_json(&self) -> &ProgramPath {
        &self.package_json
    }

    pub fn alternate_display_paths(&self) -> &[PathBuf] {
        &self.alternate_display_paths
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub const fn module_type(&self) -> PackageJsonType {
        self.module_type
    }

    fn compatible_with(&self, other: &Self) -> bool {
        self.text == other.text
            && self.name == other.name
            && self.version == other.version
            && self.module_type == other.module_type
    }

    fn remember_display_alias(&mut self, display: &Path) {
        if display != self.package_json.display()
            && !self
                .alternate_display_paths
                .iter()
                .any(|existing| existing == display)
        {
            self.alternate_display_paths.push(display.to_path_buf());
        }
    }
}

/// One raw `paths` pattern and its ordered substitutions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathMapping {
    pattern: String,
    substitutions: Vec<String>,
}

impl PathMapping {
    pub fn new(pattern: impl Into<String>, substitutions: Vec<String>) -> Self {
        Self {
            pattern: pattern.into(),
            substitutions,
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn substitutions(&self) -> &[String] {
        &self.substitutions
    }
}

/// Immutable `paths` entries and the config directory that declared them.
///
/// Keeping these values behind one shared owner prevents an inherited mapping
/// from being paired with the consuming config's directory. It also lets
/// one-shot resolver workers reuse a prepared mapping table without cloning
/// every pattern and substitution. Exact-key lookup indices and valid
/// single-star pattern offsets are compiled once here rather than reparsed for
/// every resolution request.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ProgramPathMappings {
    entries: Vec<PathMapping>,
    exact_mapping_indices: Box<[usize]>,
    wildcard_patterns: Box<[(usize, usize)]>,
    config_base_path: Option<String>,
    validation_error: Option<ResolutionError>,
}

impl ProgramPathMappings {
    /// tsc-port: tryParsePattern @6.0.3
    /// tsc-hash: 23d0684a13d57da52a90e622a841b0d031ddf89df349671683899401ebe9b83a
    /// tsc-span: _tsc.js:18773-18782
    /// tsc-port: tryParsePatterns @6.0.3
    /// tsc-hash: acf73d9f935712f2442e047a8b74f826af12a66bc1fc9880d2e064861b9f0bb6
    /// tsc-span: _tsc.js:18784-18809
    fn new(entries: Vec<PathMapping>, config_base_path: Option<String>) -> Self {
        let mut exact_mapping_indices = Vec::new();
        let mut wildcard_patterns = Vec::new();
        for (index, mapping) in entries.iter().enumerate() {
            match mapping.pattern().find('*') {
                Some(star) if !mapping.pattern()[star + 1..].contains('*') => {
                    wildcard_patterns.push((index, star));
                }
                Some(_) => {}
                None => exact_mapping_indices.push(index),
            }
        }
        exact_mapping_indices.sort_unstable_by(|left, right| {
            entries[*left]
                .pattern()
                .cmp(entries[*right].pattern())
                .then_with(|| left.cmp(right))
        });
        let validation_error = validate_path_mappings(&entries).err();
        Self {
            entries,
            exact_mapping_indices: exact_mapping_indices.into_boxed_slice(),
            wildcard_patterns: wildcard_patterns.into_boxed_slice(),
            config_base_path,
            validation_error,
        }
    }

    pub(crate) fn entries(&self) -> &[PathMapping] {
        &self.entries
    }

    pub(crate) fn config_base_path(&self) -> Option<&str> {
        self.config_base_path.as_deref()
    }

    /// tsc-port: matchPatternOrExact @6.0.3
    /// tsc-hash: 7d7159d6541c0491d2bb993c52a825b08c9950255ecbe198f9505b7b6b95a37c
    /// tsc-span: _tsc.js:18834-18843
    pub(crate) fn exact_mapping_index(&self, specifier: &str) -> Option<usize> {
        self.exact_mapping_indices
            .binary_search_by(|index| self.entries[*index].pattern().cmp(specifier))
            .ok()
            .map(|position| self.exact_mapping_indices[position])
    }

    pub(crate) fn wildcard_patterns(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.wildcard_patterns.iter().copied()
    }

    pub(crate) fn validation_error(&self) -> Option<&ResolutionError> {
        self.validation_error.as_ref()
    }
}

fn validate_path_mappings(entries: &[PathMapping]) -> Result<(), ResolutionError> {
    let mut patterns = BTreeSet::new();
    for mapping in entries {
        let pattern = mapping.pattern();
        // Empty and multi-star patterns are getOptionsDiagnostics rows
        // (TS5061 for the latter), not resolver-construction failures. The
        // matcher skips them with TypeScript's own truthiness/parse rules.
        validate_owned_path_text(pattern, "paths pattern", /* allow_empty */ true)?;
        if !patterns.insert(pattern) {
            return Err(ResolutionError::invalid_data(format!(
                "duplicate paths pattern {pattern:?} has no object-equivalent ordering semantics"
            )));
        }
        for substitution in mapping.substitutions() {
            validate_owned_path_text(
                substitution,
                "paths substitution",
                /* allow_empty */ true,
            )?;
            // Empty arrays and multi-star substitutions are diagnosed as
            // TS5066/TS5062 by the option-diagnostic layer. They remain in
            // the resolver input so program construction can recover without
            // converting an option diagnostic into an infrastructure error.
        }
    }
    Ok(())
}

/// Program/host options that do not belong in the checker's
/// [`CompilerOptions`]. Optional collections preserve absent versus explicitly
/// empty config values where the distinction affects discovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProgramOptions {
    no_lib: Option<bool>,
    preserve_symlinks: Option<bool>,
    types: Option<Vec<String>>,
    type_roots: Option<Vec<ProgramPath>>,
    config_file_path: Option<ProgramPath>,
    /// Host-selected default library basename. TypeScript's test project host
    /// deliberately returns `lib.es5.d.ts` independently of `target`; keeping
    /// that host fact here avoids pretending that `compilerOptions.lib` was
    /// explicitly supplied.
    default_library_file_name: Option<String>,
    root_dirs: Option<Vec<ProgramPath>>,
    paths: Option<Arc<ProgramPathMappings>>,
}

impl ProgramOptions {
    pub fn with_no_lib(mut self, value: bool) -> Self {
        self.no_lib = Some(value);
        self
    }

    /// Retain the raw `preserveSymlinks` option. Only an explicit true value
    /// preserves lexical resolver results instead of following real paths.
    pub fn with_preserve_symlinks(mut self, value: bool) -> Self {
        self.preserve_symlinks = Some(value);
        self
    }

    pub fn with_types(mut self, value: Vec<String>) -> Self {
        self.types = Some(value);
        self
    }

    pub fn with_type_roots(mut self, value: Vec<ProgramPath>) -> Self {
        self.type_roots = Some(value);
        self
    }

    /// Retain the normalized config-file identity used as the base for
    /// effective type roots and the synthetic automatic-types origin.
    pub fn with_config_file_path(mut self, value: ProgramPath) -> Self {
        self.config_file_path = Some(value);
        self
    }

    /// Remove a config-file identity when an embedding host parsed a config
    /// without passing TypeScript's optional `configFileName` argument.
    pub fn without_config_file_path(mut self) -> Self {
        self.config_file_path = None;
        self
    }

    /// Override the basename selected for an absent `compilerOptions.lib`.
    /// Explicit `lib` entries continue to win in the loader.
    pub fn with_default_library_file_name(mut self, value: impl Into<String>) -> Self {
        self.default_library_file_name = Some(value.into());
        self
    }

    pub fn with_root_dirs(mut self, value: Vec<ProgramPath>) -> Self {
        self.root_dirs = Some(value);
        self
    }

    pub fn with_paths(mut self, value: Vec<PathMapping>) -> Self {
        self.paths = Some(Arc::new(ProgramPathMappings::new(value, None)));
        self
    }

    /// Retain config-derived `paths` together with the directory of the config
    /// which declared the effective map.
    ///
    /// TypeScript uses this base only when `baseUrl` is absent. Keeping it in
    /// the same immutable allocation as the mappings prevents a stale
    /// `pathsBasePath` from surviving after the mappings themselves are
    /// replaced or removed.
    pub fn with_config_paths(
        mut self,
        value: Vec<PathMapping>,
        paths_base_path: impl Into<String>,
    ) -> Self {
        self.paths = Some(Arc::new(ProgramPathMappings::new(
            value,
            Some(paths_base_path.into()),
        )));
        self
    }

    pub const fn no_lib(&self) -> Option<bool> {
        self.no_lib
    }

    pub const fn preserve_symlinks(&self) -> Option<bool> {
        self.preserve_symlinks
    }

    pub const fn preserve_symlinks_effective(&self) -> bool {
        matches!(self.preserve_symlinks, Some(true))
    }

    pub fn types(&self) -> Option<&[String]> {
        self.types.as_deref()
    }

    pub fn type_roots(&self) -> Option<&[ProgramPath]> {
        self.type_roots.as_deref()
    }

    pub fn config_file_path(&self) -> Option<&ProgramPath> {
        self.config_file_path.as_ref()
    }

    pub fn default_library_file_name(&self) -> Option<&str> {
        self.default_library_file_name.as_deref()
    }

    pub fn root_dirs(&self) -> Option<&[ProgramPath]> {
        self.root_dirs.as_deref()
    }

    pub fn paths(&self) -> Option<&[PathMapping]> {
        self.paths.as_deref().map(ProgramPathMappings::entries)
    }

    /// The declaring config directory paired with the effective `paths` map.
    /// Programmatic mappings created by [`Self::with_paths`] return `None` and
    /// therefore retain the host-current-directory fallback.
    pub fn paths_base_path(&self) -> Option<&str> {
        self.paths
            .as_deref()
            .and_then(ProgramPathMappings::config_base_path)
    }

    pub(crate) fn shared_paths(&self) -> Option<Arc<ProgramPathMappings>> {
        self.paths.clone()
    }
}

/// Preparation diagnostics remain separated in the same buckets consumed by
/// the no-emit driver. No sorting or deduplication occurs here.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreparationDiagnostics {
    config: DiagnosticList,
    options: DiagnosticList,
    program: DiagnosticList,
}

impl PreparationDiagnostics {
    pub fn new(
        config: Vec<Diagnostic>,
        options: Vec<Diagnostic>,
        program: Vec<Diagnostic>,
    ) -> Self {
        Self {
            config,
            options,
            program,
        }
    }

    pub fn config(&self) -> &[Diagnostic] {
        &self.config
    }

    pub fn options(&self) -> &[Diagnostic] {
        &self.options
    }

    pub fn program(&self) -> &[Diagnostic] {
        &self.program
    }
}

/// Authoritative module and type-reference tables. Separate maps keep the
/// vendored `(source, specifier, mode)` key exact for each resolver API.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolutionTable {
    modules: BTreeMap<ResolutionKey, ModuleResolution>,
    type_references: BTreeMap<TypeReferenceResolutionKey, TypeReferenceResolution>,
}

impl ResolutionTable {
    pub fn require_module(
        &self,
        key: &ResolutionKey,
    ) -> Result<&ModuleResolution, MissingResolutionError> {
        self.modules
            .get(key)
            .ok_or_else(|| MissingResolutionError::module(key))
    }

    pub fn require_type_reference(
        &self,
        key: &TypeReferenceResolutionKey,
    ) -> Result<&TypeReferenceResolution, MissingResolutionError> {
        self.type_references
            .get(key)
            .ok_or_else(|| MissingResolutionError::type_reference(key))
    }

    /// Iterate authoritative type-reference rows in exact key order.
    ///
    /// The underlying `BTreeMap` order is stable across insertion order and
    /// supplies the program driver with a deterministic diagnostic stream.
    pub fn type_references(
        &self,
    ) -> impl ExactSizeIterator<Item = (&TypeReferenceResolutionKey, &TypeReferenceResolution)>
    {
        self.type_references.iter()
    }

    pub fn module_len(&self) -> usize {
        self.modules.len()
    }

    pub fn type_reference_len(&self) -> usize {
        self.type_references.len()
    }
}

/// Fully owned input to the one-shot H0 checker session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedProgram {
    path_context: PathContext,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    source_files: Vec<PreparedSourceFile>,
    source_by_canonical: BTreeMap<CanonicalPath, SourceFileId>,
    roots: Vec<PreparedRoot>,
    library_files: Vec<SourceFileId>,
    auxiliary_files: BTreeMap<CanonicalPath, PreparedAuxiliaryFile>,
    packages: BTreeMap<CanonicalPath, PackageMetadata>,
    resolutions: ResolutionTable,
    diagnostics: PreparationDiagnostics,
}

impl PreparedProgram {
    pub fn builder(
        path_context: PathContext,
        compiler_options: CompilerOptions,
    ) -> PreparedProgramBuilder {
        PreparedProgramBuilder::new(path_context, compiler_options)
    }

    pub fn current_directory(&self) -> &ProgramPath {
        self.path_context.current_directory()
    }

    pub fn path_context(&self) -> &PathContext {
        &self.path_context
    }

    pub fn compiler_options(&self) -> &CompilerOptions {
        &self.compiler_options
    }

    pub fn program_options(&self) -> &ProgramOptions {
        &self.program_options
    }

    pub fn source_files(&self) -> &[PreparedSourceFile] {
        &self.source_files
    }

    pub fn source_file(&self, id: SourceFileId) -> Option<&PreparedSourceFile> {
        self.source_files.get(id.index())
    }

    pub fn source_id(&self, path: &CanonicalPath) -> Option<SourceFileId> {
        self.source_by_canonical.get(path).copied()
    }

    pub fn roots(&self) -> &[PreparedRoot] {
        &self.roots
    }

    pub fn library_files(&self) -> &[SourceFileId] {
        &self.library_files
    }

    pub fn auxiliary_files(&self) -> impl Iterator<Item = &PreparedAuxiliaryFile> {
        self.auxiliary_files.values()
    }

    pub fn auxiliary_file(&self, path: &CanonicalPath) -> Option<&PreparedAuxiliaryFile> {
        self.auxiliary_files.get(path)
    }

    pub fn packages(&self) -> impl Iterator<Item = &PackageMetadata> {
        self.packages.values()
    }

    pub fn package(&self, package_json: &CanonicalPath) -> Option<&PackageMetadata> {
        self.packages.get(package_json)
    }

    pub fn resolutions(&self) -> &ResolutionTable {
        &self.resolutions
    }

    pub fn diagnostics(&self) -> &PreparationDiagnostics {
        &self.diagnostics
    }
}

/// Consuming builder that validates every cross-reference before publishing
/// a [`PreparedProgram`].
///
/// The first failed `add_*` operation permanently poisons the builder. Every
/// later `add_*` operation and `build` returns that same error, so a caller
/// cannot accidentally ignore a host/resolution failure and publish a partial
/// table.
#[derive(Clone, Debug)]
pub struct PreparedProgramBuilder {
    path_context: PathContext,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    source_files: Vec<PreparedSourceFile>,
    source_by_canonical: BTreeMap<CanonicalPath, SourceFileId>,
    source_by_realpath: BTreeMap<CanonicalPath, SourceFileId>,
    roots: Vec<PreparedRoot>,
    library_files: Vec<SourceFileId>,
    auxiliary_files: BTreeMap<CanonicalPath, PreparedAuxiliaryFile>,
    packages: BTreeMap<CanonicalPath, PackageMetadata>,
    text_by_canonical: BTreeMap<CanonicalPath, String>,
    resolutions: ResolutionTable,
    diagnostics: PreparationDiagnostics,
    fatal_error: Option<PreparationError>,
}

impl PreparedProgramBuilder {
    pub fn new(path_context: PathContext, compiler_options: CompilerOptions) -> Self {
        Self {
            path_context,
            compiler_options,
            program_options: ProgramOptions::default(),
            source_files: Vec::new(),
            source_by_canonical: BTreeMap::new(),
            source_by_realpath: BTreeMap::new(),
            roots: Vec::new(),
            library_files: Vec::new(),
            auxiliary_files: BTreeMap::new(),
            packages: BTreeMap::new(),
            text_by_canonical: BTreeMap::new(),
            resolutions: ResolutionTable::default(),
            diagnostics: PreparationDiagnostics::default(),
            fatal_error: None,
        }
    }

    pub fn set_program_options(&mut self, options: ProgramOptions) {
        self.program_options = options;
    }

    pub fn set_diagnostics(&mut self, diagnostics: PreparationDiagnostics) {
        self.diagnostics = diagnostics;
    }

    pub fn add_source_file(
        &mut self,
        source: PreparedSourceFile,
    ) -> Result<SourceFileId, PreparationError> {
        self.ensure_healthy()?;
        let result = self.try_add_source_file(source);
        self.record_result(result)
    }

    fn try_add_source_file(
        &mut self,
        source: PreparedSourceFile,
    ) -> Result<SourceFileId, PreparationError> {
        let canonical = source.path().canonical().clone();
        self.register_text_owner(
            &canonical,
            source.text(),
            source.path().display(),
            PreparationOperation::AddSourceFile,
        )?;
        if let Some(real_path) = source.real_path() {
            self.register_text_owner(
                real_path.canonical(),
                source.text(),
                real_path.display(),
                PreparationOperation::AddSourceFile,
            )?;
        }
        if let Some(existing_id) = self.source_by_canonical.get(&canonical).copied() {
            let existing = &mut self.source_files[existing_id.index()];
            if existing.compatible_with(&source) {
                existing.remember_display_alias(source.path().display());
                return Ok(existing_id);
            }
            return Err(PreparationError::new(
                PreparationErrorKind::IdentityConflict,
                PreparationOperation::AddSourceFile,
                Some(source.path().display().to_path_buf()),
                format!(
                    "canonical source {} already belongs to {} with incompatible text or metadata",
                    canonical,
                    existing.path().display().display()
                ),
            ));
        }

        if let Some(realpath) = source.real_path().map(ProgramPath::canonical) {
            if let Some(existing_id) = self.source_by_realpath.get(realpath).copied() {
                let existing = &self.source_files[existing_id.index()];
                if existing.text() != source.text() {
                    return Err(PreparationError::new(
                        PreparationErrorKind::IdentityConflict,
                        PreparationOperation::AddSourceFile,
                        Some(source.path().display().to_path_buf()),
                        format!(
                            "physical source {} already belongs to {} with incompatible text",
                            realpath,
                            existing.path().display().display()
                        ),
                    ));
                }
            }
        }

        let raw = u32::try_from(self.source_files.len()).map_err(|_| {
            PreparationError::new(
                PreparationErrorKind::ResourceLimit,
                PreparationOperation::AddSourceFile,
                Some(source.path().display().to_path_buf()),
                "prepared source count exceeds the SourceFileId range",
            )
        })?;
        let id = SourceFileId(raw);
        self.source_by_canonical.insert(canonical, id);
        if let Some(realpath) = source.real_path().map(ProgramPath::canonical).cloned() {
            self.source_by_realpath.entry(realpath).or_insert(id);
        }
        self.source_files.push(source);
        Ok(id)
    }

    pub fn add_root(&mut self, root: PreparedRoot) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self.try_add_root(root);
        self.record_result(result)
    }

    fn try_add_root(&mut self, root: PreparedRoot) -> Result<(), PreparationError> {
        if let Some(source) = root.source() {
            let prepared = self.require_source(source, PreparationOperation::AddRootFile)?;
            if !self
                .root_request_selects_source(root.path().canonical(), prepared.path().canonical())?
            {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidData,
                    PreparationOperation::AddRootFile,
                    Some(root.path().display().to_path_buf()),
                    format!(
                        "root {} does not match SourceFileId {} ({})",
                        root.path().canonical(),
                        source.raw(),
                        prepared.path().canonical()
                    ),
                ));
            }
        } else if self.root_request_has_source(root.path().canonical())? {
            return Err(PreparationError::new(
                PreparationErrorKind::InvalidData,
                PreparationOperation::AddRootFile,
                Some(root.path().display().to_path_buf()),
                "a missing root cannot hide an owned prepared source",
            ));
        }
        self.roots.push(root);
        Ok(())
    }

    fn root_request_selects_source(
        &self,
        root: &CanonicalPath,
        source: &CanonicalPath,
    ) -> Result<bool, PreparationError> {
        if extensionless_root_text(root).is_none() {
            return Ok(root == source);
        }
        if root == source {
            return Ok(false);
        }
        let Some((root_text, selected_index)) =
            extensionless_root_source_index(root, source, self.compiler_options.allow_js)
        else {
            return Ok(false);
        };
        for extension in
            &extensionless_source_probe_extensions(self.compiler_options.allow_js)[..selected_index]
        {
            let candidate =
                CanonicalPath::from_trusted_normalized(format!("{root_text}{extension}"))?;
            if self.source_by_canonical.contains_key(&candidate) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn root_request_has_source(&self, root: &CanonicalPath) -> Result<bool, PreparationError> {
        if self.source_by_canonical.contains_key(root) {
            return Ok(true);
        }
        let Some(root_text) = extensionless_root_text(root) else {
            return Ok(false);
        };
        for extension in extensionless_source_probe_extensions(self.compiler_options.allow_js) {
            let candidate =
                CanonicalPath::from_trusted_normalized(format!("{root_text}{extension}"))?;
            if self.source_by_canonical.contains_key(&candidate) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn add_root_file(&mut self, source: SourceFileId) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self
            .require_source(source, PreparationOperation::AddRootFile)
            .map(|source| source.path().clone())
            .and_then(|path| self.try_add_root(PreparedRoot::loaded(path, source)));
        self.record_result(result)
    }

    pub fn add_library_file(&mut self, source: SourceFileId) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self
            .require_source(source, PreparationOperation::AddLibraryFile)
            .map(|_| ())
            .map(|()| {
                if !self.library_files.contains(&source) {
                    self.library_files.push(source);
                }
            });
        self.record_result(result)
    }

    pub fn add_auxiliary_file(
        &mut self,
        file: PreparedAuxiliaryFile,
    ) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self.try_add_auxiliary_file(file);
        self.record_result(result)
    }

    fn try_add_auxiliary_file(
        &mut self,
        file: PreparedAuxiliaryFile,
    ) -> Result<(), PreparationError> {
        let canonical = file.path().canonical().clone();
        self.register_text_owner(
            &canonical,
            file.text(),
            file.path().display(),
            PreparationOperation::AddAuxiliaryFile,
        )?;
        if let Some(existing) = self.auxiliary_files.get_mut(&canonical) {
            if existing.text() == file.text() {
                existing.remember_display_alias(file.path().display());
                return Ok(());
            }
            return Err(PreparationError::new(
                PreparationErrorKind::IdentityConflict,
                PreparationOperation::AddAuxiliaryFile,
                Some(file.path().display().to_path_buf()),
                format!("canonical auxiliary file {canonical} has incompatible text"),
            ));
        }
        self.auxiliary_files.insert(canonical, file);
        Ok(())
    }

    pub fn add_package_metadata(
        &mut self,
        package: PackageMetadata,
    ) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self.try_add_package_metadata(package);
        self.record_result(result)
    }

    fn try_add_package_metadata(
        &mut self,
        package: PackageMetadata,
    ) -> Result<(), PreparationError> {
        let canonical = package.package_json().canonical().clone();
        self.register_text_owner(
            &canonical,
            package.text(),
            package.package_json().display(),
            PreparationOperation::AddPackageMetadata,
        )?;
        if let Some(existing) = self.packages.get_mut(&canonical) {
            if existing.compatible_with(&package) {
                existing.remember_display_alias(package.package_json().display());
                return Ok(());
            }
            return Err(PreparationError::new(
                PreparationErrorKind::IdentityConflict,
                PreparationOperation::AddPackageMetadata,
                Some(package.package_json().display().to_path_buf()),
                format!("canonical package metadata {canonical} has conflicting facts"),
            ));
        }
        self.packages.insert(canonical, package);
        Ok(())
    }

    pub fn add_module_resolution(
        &mut self,
        key: ResolutionKey,
        resolution: Result<ModuleResolution, ResolutionError>,
    ) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self.try_add_module_resolution(key, resolution);
        self.record_result(result)
    }

    fn try_add_module_resolution(
        &mut self,
        key: ResolutionKey,
        resolution: Result<ModuleResolution, ResolutionError>,
    ) -> Result<(), PreparationError> {
        let resolution = match resolution {
            Ok(resolution) => resolution,
            Err(error) => {
                let error = PreparationError::from_resolution(
                    PreparationOperation::AddModuleResolution,
                    Some(key.source().as_path().to_path_buf()),
                    error,
                );
                return Err(error);
            }
        };
        self.require_resolution_source(&key, PreparationOperation::AddModuleResolution)?;
        self.validate_module_target(&key, &resolution)?;
        insert_resolution(
            &mut self.resolutions.modules,
            key,
            resolution,
            PreparationOperation::AddModuleResolution,
        )
    }

    pub fn add_type_reference_resolution(
        &mut self,
        key: TypeReferenceResolutionKey,
        resolution: Result<TypeReferenceResolution, ResolutionError>,
    ) -> Result<(), PreparationError> {
        self.ensure_healthy()?;
        let result = self.try_add_type_reference_resolution(key, resolution);
        self.record_result(result)
    }

    fn try_add_type_reference_resolution(
        &mut self,
        key: TypeReferenceResolutionKey,
        resolution: Result<TypeReferenceResolution, ResolutionError>,
    ) -> Result<(), PreparationError> {
        let resolution = match resolution {
            Ok(resolution) => resolution,
            Err(error) => {
                let error = PreparationError::from_resolution(
                    PreparationOperation::AddTypeReferenceResolution,
                    Some(key.origin().canonical_path().as_path().to_path_buf()),
                    error,
                );
                return Err(error);
            }
        };
        self.require_type_reference_origin(&key)?;
        self.validate_type_reference_target(&key, &resolution)?;
        insert_resolution(
            &mut self.resolutions.type_references,
            key,
            resolution,
            PreparationOperation::AddTypeReferenceResolution,
        )
    }

    pub fn build(self) -> Result<PreparedProgram, PreparationError> {
        if let Some(error) = self.fatal_error {
            return Err(error);
        }
        if self.compiler_options.no_emit != Some(true) {
            return Err(PreparationError::new(
                PreparationErrorKind::InvalidInput,
                PreparationOperation::BuildPreparedProgram,
                None,
                "H0 PreparedProgram requires compilerOptions.noEmit to be explicitly true",
            ));
        }

        self.validate_case_profile()?;
        self.validate_roots()?;

        for (expected, actual) in self.library_files.iter().copied().enumerate() {
            if actual.index() != expected {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidData,
                    PreparationOperation::BuildPreparedProgram,
                    self.source_files
                        .get(actual.index())
                        .map(|source| source.path().display().to_path_buf()),
                    "library files must form the ordered prefix of final program source order",
                ));
            }
        }

        for source in &self.source_files {
            if let Some(package_scope) = source.package_scope() {
                if !self.packages.contains_key(package_scope) {
                    return Err(PreparationError::new(
                        PreparationErrorKind::InvalidReference,
                        PreparationOperation::BuildPreparedProgram,
                        Some(source.path().display().to_path_buf()),
                        format!(
                            "source package scope {} has no prepared package metadata",
                            package_scope
                        ),
                    ));
                }
            }
        }

        self.validate_missing_root_diagnostics()?;
        self.validate_diagnostic_sources()?;

        Ok(PreparedProgram {
            path_context: self.path_context,
            compiler_options: self.compiler_options,
            program_options: self.program_options,
            source_files: self.source_files,
            source_by_canonical: self.source_by_canonical,
            roots: self.roots,
            library_files: self.library_files,
            auxiliary_files: self.auxiliary_files,
            packages: self.packages,
            resolutions: self.resolutions,
            diagnostics: self.diagnostics,
        })
    }

    fn ensure_healthy(&self) -> Result<(), PreparationError> {
        match &self.fatal_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn record_result<T>(
        &mut self,
        result: Result<T, PreparationError>,
    ) -> Result<T, PreparationError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let first = self
                    .fatal_error
                    .get_or_insert_with(|| error.clone())
                    .clone();
                Err(first)
            }
        }
    }

    fn register_text_owner(
        &mut self,
        canonical: &CanonicalPath,
        text: &str,
        display: &Path,
        operation: PreparationOperation,
    ) -> Result<(), PreparationError> {
        if let Some(existing) = self.text_by_canonical.get(canonical) {
            if existing == text {
                return Ok(());
            }
            return Err(PreparationError::new(
                PreparationErrorKind::IdentityConflict,
                operation,
                Some(display.to_path_buf()),
                format!("canonical text owner {canonical} already has incompatible decoded text"),
            ));
        }
        self.text_by_canonical
            .insert(canonical.clone(), text.to_owned());
        Ok(())
    }

    fn validate_diagnostic_sources(&self) -> Result<(), PreparationError> {
        let diagnostics = self
            .diagnostics
            .config()
            .iter()
            .chain(self.diagnostics.options())
            .chain(self.diagnostics.program())
            .chain(
                self.resolutions
                    .modules
                    .values()
                    .flat_map(|resolution| resolution.diagnostics()),
            )
            .chain(
                self.resolutions
                    .type_references
                    .values()
                    .flat_map(|resolution| resolution.diagnostics()),
            );
        for diagnostic in diagnostics {
            if let Some(file_name) = diagnostic.file_name.as_deref() {
                self.require_text_for_diagnostic_file(file_name)?;
            }
            for related in &diagnostic.related {
                if let Some(file_name) = related.file_name.as_deref() {
                    self.require_text_for_diagnostic_file(file_name)?;
                }
            }
        }
        Ok(())
    }

    fn validate_missing_root_diagnostics(&self) -> Result<(), PreparationError> {
        for root in &self.roots {
            let Some(expected) = root.missing_diagnostic() else {
                continue;
            };
            if !self
                .diagnostics
                .program()
                .iter()
                .any(|diagnostic| diagnostic == expected)
            {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidReference,
                    PreparationOperation::BuildPreparedProgram,
                    Some(root.path().display().to_path_buf()),
                    "missing root has no matching program-diagnostic occurrence",
                ));
            }
        }
        Ok(())
    }

    fn validate_roots(&self) -> Result<(), PreparationError> {
        for root in &self.roots {
            let valid = match root.source() {
                Some(source) => {
                    let source =
                        self.require_source(source, PreparationOperation::BuildPreparedProgram)?;
                    self.root_request_selects_source(
                        root.path().canonical(),
                        source.path().canonical(),
                    )?
                }
                None => !self.root_request_has_source(root.path().canonical())?,
            };
            if !valid {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidData,
                    PreparationOperation::BuildPreparedProgram,
                    Some(root.path().display().to_path_buf()),
                    "root selection no longer matches the final prepared source inventory",
                ));
            }
        }
        Ok(())
    }

    fn validate_case_profile(&self) -> Result<(), PreparationError> {
        if self.path_context.use_case_sensitive_file_names() {
            return Ok(());
        }

        self.validate_canonical_case(self.path_context.current_directory().canonical())?;
        for source in &self.source_files {
            self.validate_canonical_case(source.path().canonical())?;
            if let Some(real_path) = source.real_path() {
                self.validate_canonical_case(real_path.canonical())?;
            }
            if let Some(package_scope) = source.package_scope() {
                self.validate_canonical_case(package_scope)?;
            }
        }
        for root in &self.roots {
            self.validate_canonical_case(root.path().canonical())?;
        }
        for file in self.auxiliary_files.values() {
            self.validate_canonical_case(file.path().canonical())?;
        }
        for package in self.packages.values() {
            self.validate_canonical_case(package.package_json().canonical())?;
        }
        if let Some(type_roots) = self.program_options.type_roots() {
            for path in type_roots {
                self.validate_canonical_case(path.canonical())?;
            }
        }
        if let Some(path) = self.program_options.config_file_path() {
            self.validate_canonical_case(path.canonical())?;
        }
        if let Some(root_dirs) = self.program_options.root_dirs() {
            for path in root_dirs {
                self.validate_canonical_case(path.canonical())?;
            }
        }
        for (key, resolution) in &self.resolutions.modules {
            self.validate_canonical_case(key.source())?;
            if let Some(alternate_result) = resolution.alternate_result() {
                self.validate_canonical_case(alternate_result.canonical())?;
            }
            if let ResolutionOutcome::Resolved(module) = resolution.outcome() {
                self.validate_canonical_case(module.target().resolved_file().canonical())?;
                if let Some(original_path) = module.original_path() {
                    self.validate_canonical_case(original_path.canonical())?;
                }
            }
        }
        for (key, resolution) in &self.resolutions.type_references {
            self.validate_canonical_case(key.origin().canonical_path())?;
            if let ResolutionOutcome::Resolved(directive) = resolution.outcome() {
                self.validate_canonical_case(directive.target().canonical())?;
                if let Some(original_path) = directive.original_path() {
                    self.validate_canonical_case(original_path.canonical())?;
                }
            }
        }
        Ok(())
    }

    fn validate_canonical_case(&self, path: &CanonicalPath) -> Result<(), PreparationError> {
        let text = path
            .as_path()
            .to_str()
            .expect("canonical program paths are validated as Unicode");
        let folded = to_file_name_lower_case(text);
        if folded == text {
            return Ok(());
        }
        Err(PreparationError::new(
            PreparationErrorKind::InvalidData,
            PreparationOperation::BuildPreparedProgram,
            Some(path.as_path().to_path_buf()),
            format!("canonical path is not folded for a case-insensitive host (expected {folded})"),
        ))
    }

    fn require_text_for_diagnostic_file(&self, file_name: &str) -> Result<(), PreparationError> {
        let matches_path = |path: &ProgramPath, aliases: &[PathBuf]| {
            path.display()
                .to_str()
                .is_some_and(|candidate| diagnostic_file_names_equal(candidate, file_name))
                || path
                    .canonical()
                    .as_path()
                    .to_str()
                    .is_some_and(|candidate| diagnostic_file_names_equal(candidate, file_name))
                || aliases.iter().any(|path| {
                    path.to_str()
                        .is_some_and(|candidate| diagnostic_file_names_equal(candidate, file_name))
                })
        };
        let has_source = self.source_files.iter().any(|source| {
            matches_path(source.path(), source.alternate_display_paths())
                || source
                    .real_path()
                    .is_some_and(|path| matches_path(path, &[]))
        });
        let has_auxiliary = self
            .auxiliary_files
            .values()
            .any(|file| matches_path(file.path(), file.alternate_display_paths()));
        let has_package = self
            .packages
            .values()
            .any(|package| matches_path(package.package_json(), package.alternate_display_paths()));
        if has_source || has_auxiliary || has_package {
            return Ok(());
        }
        Err(PreparationError::new(
            PreparationErrorKind::InvalidReference,
            PreparationOperation::BuildPreparedProgram,
            Some(PathBuf::from(file_name)),
            "located diagnostic has no owned source text for rendering",
        ))
    }

    fn require_source(
        &self,
        source: SourceFileId,
        operation: PreparationOperation,
    ) -> Result<&PreparedSourceFile, PreparationError> {
        self.source_files.get(source.index()).ok_or_else(|| {
            PreparationError::new(
                PreparationErrorKind::InvalidReference,
                operation,
                None,
                format!("unknown SourceFileId {}", source.raw()),
            )
        })
    }

    fn require_resolution_source(
        &self,
        key: &ResolutionKey,
        operation: PreparationOperation,
    ) -> Result<SourceFileId, PreparationError> {
        self.source_by_canonical
            .get(key.source())
            .copied()
            .ok_or_else(|| {
                PreparationError::new(
                    PreparationErrorKind::InvalidReference,
                    operation,
                    Some(key.source().as_path().to_path_buf()),
                    format!(
                        "resolution source is not an owned prepared file: {:?}",
                        key.specifier()
                    ),
                )
            })
    }

    fn require_type_reference_origin(
        &self,
        key: &TypeReferenceResolutionKey,
    ) -> Result<(), PreparationError> {
        let TypeReferenceResolutionOrigin::Source(source) = key.origin() else {
            let containing_file = key.origin().canonical_path();
            let file_name = containing_file
                .as_path()
                .to_str()
                .expect("canonical program paths are validated as Unicode")
                .rsplit(['/', '\\'])
                .next();
            if file_name == Some("__inferred type names__.ts")
                && key.mode() == ResolutionMode::Unspecified
            {
                return Ok(());
            }
            return Err(PreparationError::new(
                PreparationErrorKind::InvalidData,
                PreparationOperation::AddTypeReferenceResolution,
                Some(containing_file.as_path().to_path_buf()),
                "automatic type-reference resolutions require the inferred-types containing file and unspecified mode",
            ));
        };
        self.source_by_canonical
            .contains_key(source)
            .then_some(())
            .ok_or_else(|| {
                PreparationError::new(
                    PreparationErrorKind::InvalidReference,
                    PreparationOperation::AddTypeReferenceResolution,
                    Some(source.as_path().to_path_buf()),
                    format!(
                        "type-reference source is not an owned prepared file: {:?}",
                        key.specifier()
                    ),
                )
            })
    }

    fn validate_module_target(
        &self,
        key: &ResolutionKey,
        resolution: &ModuleResolution,
    ) -> Result<(), PreparationError> {
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            return Ok(());
        };
        let resolved_file = module.target().resolved_file();
        let extension_path = if let Some(original_path) = module.original_path() {
            if !module.is_external_library_import() {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidData,
                    PreparationOperation::AddModuleResolution,
                    Some(original_path.display().to_path_buf()),
                    "a module original path requires an external-library import",
                ));
            }
            if is_external_module_name_relative(key.specifier()) {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidData,
                    PreparationOperation::AddModuleResolution,
                    Some(original_path.display().to_path_buf()),
                    "a module original path requires a non-relative, non-rooted module specifier",
                ));
            }
            if original_path.canonical() == resolved_file.canonical() {
                return Err(PreparationError::new(
                    PreparationErrorKind::InvalidData,
                    PreparationOperation::AddModuleResolution,
                    Some(original_path.display().to_path_buf()),
                    "a module original path must differ from its resolved file",
                ));
            }
            original_path
        } else {
            resolved_file
        };
        // createResolvedModuleWithFailedLookupLocationsHandlingSymlink
        // classifies the lexical path before following realpath. In a
        // case-insensitive profile an arbitrary twin such as `.d.CSS.ts`
        // must likewise be checked against the retained display spelling,
        // not a lower-cased canonical key or a differently named physical
        // target.
        let extension_file_name = extension_path
            .display()
            .to_str()
            .expect("display program paths are Unicode");
        if !module.extension().is_valid()
            || !module
                .extension()
                .matches_path_with_case_and_module_suffixes(
                    extension_file_name,
                    self.path_context.use_case_sensitive_file_names(),
                    self.compiler_options.module_suffixes.as_deref(),
                )
        {
            return Err(PreparationError::new(
                PreparationErrorKind::InvalidData,
                PreparationOperation::AddModuleResolution,
                Some(extension_path.display().to_path_buf()),
                format!(
                    "resolved module path does not match extension {}",
                    module.extension().as_str()
                ),
            ));
        }
        match module.target() {
            ResolvedModuleTarget::Source {
                source,
                resolved_file,
            } => {
                let prepared = self
                    .require_source(*source, PreparationOperation::AddModuleResolution)
                    .map_err(|_| {
                        PreparationError::new(
                            PreparationErrorKind::InvalidReference,
                            PreparationOperation::AddModuleResolution,
                            Some(key.source().as_path().to_path_buf()),
                            format!(
                                "resolved module {:?} references unknown SourceFileId {}",
                                key.specifier(),
                                source.raw()
                            ),
                        )
                    })?;
                self.validate_owned_resolution_paths(
                    prepared,
                    *source,
                    resolved_file,
                    module.original_path(),
                    PreparationOperation::AddModuleResolution,
                    "resolved module",
                )?;
            }
            ResolvedModuleTarget::Unloaded { .. } => {
                let is_owned = self
                    .source_by_canonical
                    .contains_key(resolved_file.canonical())
                    || self
                        .source_by_realpath
                        .contains_key(resolved_file.canonical());
                if is_owned {
                    return Err(PreparationError::new(
                        PreparationErrorKind::InvalidData,
                        PreparationOperation::AddModuleResolution,
                        Some(resolved_file.display().to_path_buf()),
                        "an unloaded resolution target is already an owned prepared source",
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_type_reference_target(
        &self,
        key: &TypeReferenceResolutionKey,
        resolution: &TypeReferenceResolution,
    ) -> Result<(), PreparationError> {
        let ResolutionOutcome::Resolved(directive) = resolution.outcome() else {
            return Ok(());
        };
        let source = directive.source();
        let prepared = self
            .require_source(source, PreparationOperation::AddTypeReferenceResolution)
            .map_err(|_| {
                PreparationError::new(
                    PreparationErrorKind::InvalidReference,
                    PreparationOperation::AddTypeReferenceResolution,
                    Some(key.origin().canonical_path().as_path().to_path_buf()),
                    format!(
                        "resolved type reference {:?} references unknown SourceFileId {}",
                        key.specifier(),
                        source.raw()
                    ),
                )
            })?;
        let target = directive.target();
        self.validate_owned_resolution_paths(
            prepared,
            source,
            target,
            directive.original_path(),
            PreparationOperation::AddTypeReferenceResolution,
            "type-reference",
        )
    }

    fn validate_owned_resolution_paths(
        &self,
        prepared: &PreparedSourceFile,
        source: SourceFileId,
        target: &ProgramPath,
        original_path: Option<&ProgramPath>,
        operation: PreparationOperation,
        label: &str,
    ) -> Result<(), PreparationError> {
        let program_path = prepared.path().canonical();
        let real = prepared.real_path().map(ProgramPath::canonical);
        let distinct_real = real.filter(|path| *path != program_path);
        let matches_program_path = program_path == target.canonical();
        let matches_real = real.is_some_and(|path| path == target.canonical());
        if !matches_program_path && !matches_real {
            return Err(PreparationError::new(
                PreparationErrorKind::InvalidData,
                operation,
                Some(target.display().to_path_buf()),
                format!(
                    "{label} target {} does not match SourceFileId {} ({})",
                    target.canonical(),
                    source.raw(),
                    program_path
                ),
            ));
        }

        match original_path {
            Some(original)
                if original.canonical() == program_path
                    && distinct_real == Some(target.canonical())
                    && !matches_program_path =>
            {
                Ok(())
            }
            Some(original)
                if matches_program_path
                    && distinct_real.is_none()
                    && original.canonical() != target.canonical() =>
            {
                Ok(())
            }
            Some(original) => Err(PreparationError::new(
                PreparationErrorKind::InvalidData,
                operation,
                Some(original.display().to_path_buf()),
                format!(
                    "{label} original path {} is not the lexical path for physical target {}",
                    original.canonical(),
                    target.canonical()
                ),
            )),
            None if matches_program_path => Ok(()),
            None => Err(PreparationError::new(
                PreparationErrorKind::InvalidData,
                operation,
                Some(target.display().to_path_buf()),
                format!(
                    "{label} physical target {} is missing its original lexical path {}",
                    target.canonical(),
                    program_path
                ),
            )),
        }
    }
}

trait TableKey {
    fn canonical_path(&self) -> &CanonicalPath;
    fn specifier(&self) -> &str;
    fn mode(&self) -> ResolutionMode;
}

impl TableKey for ResolutionKey {
    fn canonical_path(&self) -> &CanonicalPath {
        self.source()
    }

    fn specifier(&self) -> &str {
        self.specifier()
    }

    fn mode(&self) -> ResolutionMode {
        self.mode()
    }
}

impl TableKey for TypeReferenceResolutionKey {
    fn canonical_path(&self) -> &CanonicalPath {
        self.origin().canonical_path()
    }

    fn specifier(&self) -> &str {
        self.specifier()
    }

    fn mode(&self) -> ResolutionMode {
        self.mode()
    }
}

fn insert_resolution<K: Eq + Ord + TableKey, T: Eq>(
    table: &mut BTreeMap<K, T>,
    key: K,
    value: T,
    operation: PreparationOperation,
) -> Result<(), PreparationError> {
    if let Some(existing) = table.get(&key) {
        if existing == &value {
            return Ok(());
        }
        return Err(PreparationError::new(
            PreparationErrorKind::IdentityConflict,
            operation,
            Some(key.canonical_path().as_path().to_path_buf()),
            format!(
                "resolution key ({:?}, {:?}) has conflicting outcomes",
                key.specifier(),
                key.mode()
            ),
        ));
    }
    table.insert(key, value);
    Ok(())
}

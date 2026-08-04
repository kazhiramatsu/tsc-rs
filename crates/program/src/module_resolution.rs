use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Map, Value};
use tsc_host::{to_file_name_lower_case, CompilerHost};
use tsc_types::{compiler_version_satisfies, js_number_to_string, CompilerOptions};

use crate::json::{
    decode_user_object_key, json_number_as_f64, json_object_get, json_object_own_get,
    jsonc_prototype, parse_json_object,
};
use crate::path::ProgramPath;
use crate::prepared::{
    PackageJsonType, PackageMetadata, PathContext, ProgramOptions, ProgramPathMappings,
    SourceFileId,
};
use crate::resolution::{
    ModuleExtension, PackageId, ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModule,
    ResolvedModuleTarget, ResolvedTypeReferenceDirective,
};
use crate::text::decode_host_text;

// Package-import rewrites use an explicit continuation stack below. The work
// budget is derived from owned package.json input, so finite maps are not
// constrained by an arbitrary nesting depth while expanding wildcard cycles
// fail as a typed resource error before exhausting memory.
const MIN_PACKAGE_MAP_REWRITE_WORK_BUDGET: usize = 4_096;
const PACKAGE_MAP_REWRITE_INPUT_MULTIPLIER: usize = 8;
const MIN_JS_REPLACEMENT_OUTPUT_BUDGET: usize = 1 << 20;
const MAX_JS_REPLACEMENT_OUTPUT_BUDGET: usize = 64 << 20;
const JS_REPLACEMENT_INPUT_MULTIPLIER: usize = 16;
const MAX_JS_JSON_COERCION_OUTPUT_BUDGET: usize = 64 << 20;

/// Filesystem-derived module facts that have not yet been bound to a program
/// source id.
///
/// Manifest-backed resolutions retain the decoded `package.json` observation
/// used for package scopes and implied node formats. Manifestless legacy
/// results carry no synthetic metadata. Retained metadata is reference-counted
/// so repeated resolutions do not copy the decoded JSON text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostResolvedModule {
    resolved_file: ProgramPath,
    extension: ModuleExtension,
    original_path: Option<ProgramPath>,
    is_external_library_import: bool,
    resolved_using_ts_extension: bool,
    package_id: Option<PackageId>,
    alternate_result: Option<ProgramPath>,
    package_metadata: Option<Rc<PackageMetadata>>,
    realpath_may_be_missing_after_suffix_predicate: bool,
}

impl HostResolvedModule {
    pub fn resolved_file(&self) -> &ProgramPath {
        &self.resolved_file
    }

    pub fn extension(&self) -> &ModuleExtension {
        &self.extension
    }

    pub fn original_path(&self) -> Option<&ProgramPath> {
        self.original_path.as_ref()
    }

    pub const fn is_external_library_import(&self) -> bool {
        self.is_external_library_import
    }

    pub const fn resolved_using_ts_extension(&self) -> bool {
        self.resolved_using_ts_extension
    }

    pub fn package_id(&self) -> Option<&PackageId> {
        self.package_id.as_ref()
    }

    pub fn alternate_result(&self) -> Option<&ProgramPath> {
        self.alternate_result.as_ref()
    }

    pub fn package_metadata(&self) -> Option<&PackageMetadata> {
        self.package_metadata.as_deref()
    }

    /// Bind the host result to a target whose source membership has already
    /// been decided by the program loader.
    ///
    /// tsrs-native: bridges host probing to the owned resolution contract.
    pub fn into_resolved_module(
        self,
        target: ResolvedModuleTarget,
    ) -> Result<ResolvedModule, ResolutionError> {
        if target.resolved_file().canonical() != self.resolved_file.canonical() {
            return Err(ResolutionError::invalid_data(format!(
                "caller target {} does not match host resolution {}",
                target.resolved_file().display().display(),
                self.resolved_file.display().display()
            )));
        }

        let mut resolved = ResolvedModule::new(target, self.extension)
            .with_external_library_import(self.is_external_library_import)
            .with_resolved_using_ts_extension(self.resolved_using_ts_extension);
        if let Some(original_path) = self.original_path {
            resolved = resolved.with_original_path(original_path);
        }
        if let Some(package_id) = self.package_id {
            resolved = resolved.with_package_id(package_id);
        }
        Ok(resolved)
    }
}

/// Lossless filesystem facts for one module-resolution request.
///
/// `alternate_result` is independent of the primary outcome because Node10
/// may miss under its legacy package rules while a diagnostic-only Bundler
/// retry finds a declaration target. Keeping both values in the return value
/// avoids a resolver side channel and lets the prepared-program owner bind a
/// `NotFound` row with its alternate path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostModuleResolution {
    outcome: ResolutionOutcome<HostResolvedModule>,
    alternate_result: Option<ProgramPath>,
}

impl HostModuleResolution {
    fn new(
        outcome: ResolutionOutcome<HostResolvedModule>,
        alternate_result: Option<ProgramPath>,
    ) -> Self {
        Self {
            outcome,
            alternate_result,
        }
    }

    pub fn outcome(&self) -> &ResolutionOutcome<HostResolvedModule> {
        &self.outcome
    }

    pub fn alternate_result(&self) -> Option<&ProgramPath> {
        self.alternate_result
            .as_ref()
            .or_else(|| match &self.outcome {
                ResolutionOutcome::Resolved(module) => module.alternate_result(),
                ResolutionOutcome::NotFound => None,
            })
    }

    pub fn into_outcome(self) -> ResolutionOutcome<HostResolvedModule> {
        self.outcome
    }
}

/// Filesystem-derived facts for one resolved type-reference directive before
/// the program loader binds the target to a [`SourceFileId`](crate::SourceFileId).
///
/// Type-reference resolution shares package probing and package metadata with
/// module resolution, but retains the distinct `primary` result bit published
/// by `resolveTypeReferenceDirective`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostResolvedTypeReferenceDirective {
    resolved_file: ProgramPath,
    extension: ModuleExtension,
    original_path: Option<ProgramPath>,
    primary: bool,
    is_external_library_import: bool,
    package_id: Option<PackageId>,
    package_metadata: Option<Rc<PackageMetadata>>,
}

impl HostResolvedTypeReferenceDirective {
    fn from_module(module: HostResolvedModule, primary: bool) -> Self {
        Self {
            resolved_file: module.resolved_file,
            extension: module.extension,
            original_path: module.original_path,
            primary,
            is_external_library_import: module.is_external_library_import,
            package_id: module.package_id,
            package_metadata: module.package_metadata,
        }
    }

    pub fn resolved_file(&self) -> &ProgramPath {
        &self.resolved_file
    }

    pub fn extension(&self) -> &ModuleExtension {
        &self.extension
    }

    pub fn original_path(&self) -> Option<&ProgramPath> {
        self.original_path.as_ref()
    }

    pub const fn primary(&self) -> bool {
        self.primary
    }

    pub const fn is_external_library_import(&self) -> bool {
        self.is_external_library_import
    }

    pub fn package_id(&self) -> Option<&PackageId> {
        self.package_id.as_ref()
    }

    pub fn package_metadata(&self) -> Option<&PackageMetadata> {
        self.package_metadata.as_deref()
    }

    /// Bind the host result to a target whose source membership has already
    /// been decided by the program loader.
    ///
    /// tsrs-native: bridges host probing to the owned resolution contract.
    pub fn into_resolved_type_reference_directive(
        self,
        target: ProgramPath,
        source: SourceFileId,
    ) -> Result<ResolvedTypeReferenceDirective, ResolutionError> {
        if target.canonical() != self.resolved_file.canonical() {
            return Err(ResolutionError::invalid_data(format!(
                "caller target {} does not match host resolution {}",
                target.display().display(),
                self.resolved_file.display().display()
            )));
        }

        let mut resolved = ResolvedTypeReferenceDirective::new(target, source)
            .with_primary(self.primary)
            .with_external_library_import(self.is_external_library_import);
        if let Some(original_path) = self.original_path {
            resolved = resolved.with_original_path(original_path);
        }
        if let Some(package_id) = self.package_id {
            resolved = resolved.with_package_id(package_id);
        }
        Ok(resolved)
    }
}

#[derive(Clone, Debug)]
struct CachedPackage {
    root: String,
    exports: Option<Value>,
    has_own_exports: bool,
    imports: Option<Value>,
    types_versions: Option<Value>,
    typings: Option<String>,
    types: Option<String>,
    main: Option<String>,
    tsconfig: Option<String>,
    metadata: Rc<PackageMetadata>,
}

#[derive(Clone, Debug)]
enum PackageCacheEntry {
    Missing,
    Found(Rc<CachedPackage>),
}

#[derive(Clone, Debug)]
struct PackageRequest<'a> {
    package_name: &'a str,
    exports_subpath: String,
    trailing_separator: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveResolution {
    containing_directory: String,
    specifier: String,
    mode: ResolutionMode,
}

/// `Terminal` distinguishes an explicit `null` target (or a successful
/// target) from an inapplicable/missing branch. A string probe miss remains
/// `Continue` while a conditional object or fallback array is being walked.
/// Self-reference lookup preserves that distinction for node_modules fallback;
/// an external package boundary collapses the final miss to `NotFound`.
enum Search<T> {
    Continue,
    Terminal(ResolutionOutcome<T>),
}

struct SelectedPackageMapTarget<'a> {
    target: &'a Value,
    subpath: String,
    pattern: bool,
}

enum ImportsTargetState {
    Target {
        package: Rc<CachedPackage>,
        target: Value,
        subpath: String,
        pattern: bool,
    },
    Bare {
        containing_directory: String,
        specifier: String,
    },
    Result(Search<HostResolvedModule>),
}

enum ImportsTargetFrame {
    Sequence {
        package: Rc<CachedPackage>,
        remaining: std::vec::IntoIter<Value>,
        subpath: String,
        pattern: bool,
    },
    BareAfterPackageMap {
        containing_directory: String,
        specifier: String,
        features: BareResolutionFeatures,
    },
}

#[derive(Clone, Copy)]
enum ExtensionProbePass {
    Empty,
    All,
    Preferred,
    Declaration,
    Fallback,
    JsonConfig,
    JsonModule,
}

const fn probe_pass_has_declaration(pass: ExtensionProbePass) -> bool {
    matches!(
        pass,
        ExtensionProbePass::All | ExtensionProbePass::Preferred | ExtensionProbePass::Declaration
    )
}

const fn preferred_diagnostic_pass(pass: ExtensionProbePass) -> ExtensionProbePass {
    match pass {
        ExtensionProbePass::All | ExtensionProbePass::Preferred => ExtensionProbePass::Preferred,
        ExtensionProbePass::Declaration => ExtensionProbePass::Declaration,
        ExtensionProbePass::JsonConfig => ExtensionProbePass::JsonConfig,
        ExtensionProbePass::JsonModule => ExtensionProbePass::JsonModule,
        ExtensionProbePass::Empty | ExtensionProbePass::Fallback => ExtensionProbePass::Empty,
    }
}

type ExtensionProbe = (ModuleExtension, &'static str);
type ExtensionProbePlan<'a> = (&'a str, &'static [ExtensionProbe], usize);

const CJS_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Cts, ".cts"),
    (ModuleExtension::Dcts, ".d.cts"),
    (ModuleExtension::Cjs, ".cjs"),
];
const MJS_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Mts, ".mts"),
    (ModuleExtension::Dmts, ".d.mts"),
    (ModuleExtension::Mjs, ".mjs"),
];
const JS_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Ts, ".ts"),
    (ModuleExtension::Tsx, ".tsx"),
    (ModuleExtension::Dts, ".d.ts"),
    (ModuleExtension::Js, ".js"),
    (ModuleExtension::Jsx, ".jsx"),
];
const JSX_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Tsx, ".tsx"),
    (ModuleExtension::Ts, ".ts"),
    (ModuleExtension::Dts, ".d.ts"),
    (ModuleExtension::Jsx, ".jsx"),
    (ModuleExtension::Js, ".js"),
];
const TS_PROBES: &[ExtensionProbe] = JS_PROBES;
const TSX_PROBES: &[ExtensionProbe] = JSX_PROBES;
const DTS_PROBES: &[ExtensionProbe] = JS_PROBES;
const MTS_PROBES: &[ExtensionProbe] = MJS_PROBES;
const DMTS_PROBES: &[ExtensionProbe] = MJS_PROBES;
const CTS_PROBES: &[ExtensionProbe] = CJS_PROBES;
const DCTS_PROBES: &[ExtensionProbe] = CJS_PROBES;
const DECLARATION_DTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dts, ".d.ts")];
const DECLARATION_DMTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dmts, ".d.mts")];
const DECLARATION_DCTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dcts, ".d.cts")];
const JSON_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dts, ".d.json.ts"),
    (ModuleExtension::Json, ".json"),
];
const JSON_CONFIG_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Json, ".json")];
const JSON_DISABLED_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dts, ".d.json.ts")];
const DJSON_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dts, ".d.json.ts")];

#[derive(Clone, Copy)]
struct ExportProbeContext {
    is_external_library_import: bool,
    follow_realpath: bool,
    pass: ExtensionProbePass,
    mode: ResolutionMode,
    resolution_kind: i32,
    exports_pattern_trailers: bool,
    kind: PackageMapKind,
    bare_features: Option<BareResolutionFeatures>,
}

/// The `NodeResolutionFeatures.ExportsPatternTrailers` bit from TypeScript.
///
/// Node16, NodeNext, and Bundler carry the bit in their default feature masks.
/// An explicit per-request resolution mode ORs in `AllFeatures`, including for
/// legacy Classic and Node10 type-reference resolution; an unspecified legacy
/// request can therefore enable exports without enabling pattern trailers.
fn exports_pattern_trailers_enabled(mode: ResolutionMode, resolution_kind: i32) -> bool {
    mode != ResolutionMode::Unspecified || matches!(resolution_kind, 3 | 99 | 100)
}

#[derive(Clone, Copy)]
struct BareResolutionFeatures {
    use_package_exports: bool,
    enable_imports: bool,
    enable_self_name: bool,
    resolution_kind: i32,
}

#[derive(Clone, Copy)]
struct LegacyResolutionContext {
    is_external_library_import: bool,
    attach_package_id: bool,
    resolved_using_ts_extension: bool,
    follow_realpath: bool,
}

#[derive(Clone, Copy)]
struct TypesVersionsResolutionContext<'a> {
    legacy: LegacyResolutionContext,
    base_directory: &'a str,
    loader: TypesVersionsLoader,
    attach_exact_package_id: bool,
    only_record_failures: bool,
}

#[derive(Clone, Copy)]
enum TypesVersionsLoader {
    PackageDirectory,
    PackageSubpath,
}

struct SpecificPackageResolution {
    outcome: ResolutionOutcome<HostResolvedModule>,
    root_package_observed: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PackageMapKind {
    Exports,
    Imports,
}

#[derive(Clone, Copy)]
enum OptionalResolutionLoader {
    Classic,
    Node,
}

/// Sequential Node16/NodeNext/Bundler resolver for the H0.2 package-map
/// slices.
///
/// One resolver owns a per-run `package.json` cache. Host methods remain
/// fallible and are never translated into ordinary lookup misses.
pub struct ModuleResolver<'a> {
    host: &'a dyn CompilerHost,
    options: &'a CompilerOptions,
    preserve_symlinks: bool,
    path_context: PathContext,
    type_root_base_directory: String,
    type_roots: Option<Vec<ProgramPath>>,
    base_url: Option<Arc<str>>,
    paths: Option<Arc<ProgramPathMappings>>,
    paths_base_directory: Option<Arc<str>>,
    root_dirs: Option<Vec<String>>,
    package_cache: BTreeMap<String, PackageCacheEntry>,
    package_cache_enabled: bool,
    active_resolutions: Vec<ActiveResolution>,
    active_package_maps: Vec<String>,
}

impl<'a> ModuleResolver<'a> {
    /// Construct the resolver from the host's exact path profile.
    ///
    /// tsrs-native: establishes the shared program-layer path context.
    pub fn new(
        host: &'a dyn CompilerHost,
        options: &'a CompilerOptions,
    ) -> Result<Self, ResolutionError> {
        Self::new_with_owned_paths(host, options, false, None, None, None, None)
    }

    /// Construct a resolver with the ordered program-owned resolution options.
    ///
    /// `paths` mappings are shared immutably with this one-shot resolver while
    /// `rootDirs` are normalized into resolver-owned strings. The optional
    /// config identity also anchors default type roots. [`Self::new`]
    /// deliberately remains the compatibility entry point without these
    /// program-owned options.
    pub fn new_with_program_options(
        host: &'a dyn CompilerHost,
        options: &'a CompilerOptions,
        program_options: &ProgramOptions,
    ) -> Result<Self, ResolutionError> {
        Self::new_with_owned_paths(
            host,
            options,
            program_options.preserve_symlinks_effective(),
            program_options.shared_paths(),
            program_options.config_file_path(),
            program_options.root_dirs(),
            program_options.type_roots(),
        )
    }

    fn new_with_owned_paths(
        host: &'a dyn CompilerHost,
        options: &'a CompilerOptions,
        preserve_symlinks: bool,
        paths: Option<Arc<ProgramPathMappings>>,
        config_file_path: Option<&ProgramPath>,
        root_dirs: Option<&[ProgramPath]>,
        type_roots: Option<&[ProgramPath]>,
    ) -> Result<Self, ResolutionError> {
        let current_directory = host.current_directory()?;
        let normalized = normalize_absolute_path(&current_directory, None)?;
        let case_sensitive = host.use_case_sensitive_file_names();
        let current_directory = make_program_path(&normalized, case_sensitive)?;
        let type_root_base_directory = match config_file_path {
            Some(config_file_path) => {
                let config =
                    normalize_absolute_path(config_file_path.display(), Some(normalized.as_str()))?;
                let normalized_config = make_program_path(&config, case_sensitive)?;
                if &normalized_config != config_file_path {
                    return Err(ResolutionError::canonicalization(
                        Some(config_file_path.display().to_path_buf()),
                        "config-file display and canonical paths do not match the resolver path profile",
                    ));
                }
                directory_name(&config)
            }
            None => normalized.clone(),
        };
        let base_url =
            normalize_base_url(options.base_url.as_deref(), &normalized)?.map(Arc::<str>::from);
        let paths = validate_paths(paths)?;
        let paths_base_directory = match paths.as_deref() {
            None => None,
            Some(_) if base_url.is_some() => base_url.clone(),
            Some(paths) => Some(match paths.config_base_path() {
                Some(base_path) => Arc::from(normalize_paths_base_path(base_path, &normalized)?),
                None => Arc::from(normalized.clone()),
            }),
        };
        let root_dirs = validate_and_clone_root_dirs(root_dirs, &normalized, case_sensitive)?;
        Ok(Self {
            host,
            options,
            preserve_symlinks,
            path_context: PathContext::new(current_directory, case_sensitive),
            type_root_base_directory,
            type_roots: type_roots.map(<[_]>::to_vec),
            base_url,
            paths,
            paths_base_directory,
            root_dirs,
            package_cache: BTreeMap::new(),
            package_cache_enabled: true,
            active_resolutions: Vec::new(),
            active_package_maps: Vec::new(),
        })
    }

    /// Reuse a loader-owned path context after validating it against the host.
    ///
    /// tsrs-native: keeps resolver and prepared-program identities identical.
    pub fn from_path_context(
        host: &'a dyn CompilerHost,
        options: &'a CompilerOptions,
        path_context: PathContext,
    ) -> Result<Self, ResolutionError> {
        validate_path_context(host, &path_context)?;
        let current_directory = path_context
            .current_directory()
            .display()
            .to_str()
            .ok_or_else(|| {
                ResolutionError::canonicalization(
                    Some(path_context.current_directory().display().to_path_buf()),
                    "current directory is not valid Unicode",
                )
            })?;
        let base_url = normalize_base_url(options.base_url.as_deref(), current_directory)?
            .map(Arc::<str>::from);
        Ok(Self {
            host,
            options,
            preserve_symlinks: false,
            type_root_base_directory: current_directory.to_owned(),
            type_roots: None,
            path_context,
            base_url,
            paths: None,
            paths_base_directory: None,
            root_dirs: None,
            package_cache: BTreeMap::new(),
            package_cache_enabled: true,
            active_resolutions: Vec::new(),
            active_package_maps: Vec::new(),
        })
    }

    pub fn path_context(&self) -> &PathContext {
        &self.path_context
    }

    /// Probe one fully materialized extension candidate through the ordered
    /// `moduleSuffixes` runtime list.
    ///
    /// The extension-family caller remains the outer loop, matching
    /// TypeScript's extension-major/suffix-minor order. `None` and an empty
    /// list keep the allocation-free ordinary probe; a nonempty list probes
    /// only its entries and coerces a preserved JavaScript `undefined` slot to
    /// the literal text `"undefined"`.
    ///
    /// tsc-port: tryFile @6.0.3
    /// tsc-hash: f63a8d0332580ba937377a668f6c66050d6fc485b33526cd09c08e07b96d1f9f
    /// tsc-span: _tsc.js:41230-41238
    fn try_file<'candidate>(
        &self,
        file_name: &'candidate str,
    ) -> Result<Option<Cow<'candidate, str>>, ResolutionError> {
        let Some(suffixes) = self
            .options
            .module_suffixes
            .as_deref()
            .filter(|suffixes| !suffixes.is_empty())
        else {
            return self
                .host
                .file_exists(Path::new(file_name))
                .map(|exists| exists.then_some(Cow::Borrowed(file_name)))
                .map_err(Into::into);
        };

        let extension = module_suffix_extension(file_name);
        let file_name_without_extension = &file_name[..file_name.len() - extension.len()];
        let mut candidate = String::new();
        for suffix in suffixes {
            let suffix = suffix.runtime_text();
            if suffix.is_empty() {
                if self.host.file_exists(Path::new(file_name))? {
                    return Ok(Some(Cow::Borrowed(file_name)));
                }
                continue;
            }
            let candidate_length = file_name_without_extension
                .len()
                .checked_add(suffix.len())
                .and_then(|length| length.checked_add(extension.len()))
                .ok_or_else(|| {
                    ResolutionError::resource_limit(
                        "moduleSuffixes candidate length exceeds the addressable string range",
                    )
                })?;
            candidate.clear();
            candidate
                .try_reserve_exact(candidate_length)
                .map_err(|error| {
                    ResolutionError::resource_limit(format!(
                        "cannot reserve {candidate_length} bytes for a moduleSuffixes candidate: {error}"
                    ))
                })?;
            candidate.push_str(file_name_without_extension);
            candidate.push_str(suffix);
            candidate.push_str(extension);
            if self.host.file_exists(Path::new(&candidate))? {
                return Ok(Some(Cow::Owned(candidate)));
            }
        }
        Ok(None)
    }

    /// Every successfully decoded package manifest observed by this resolver,
    /// in canonical-path order and without duplicates.
    pub fn observed_package_metadata(&self) -> impl Iterator<Item = &PackageMetadata> {
        self.package_cache.values().filter_map(|entry| match entry {
            PackageCacheEntry::Missing => None,
            PackageCacheEntry::Found(package) => Some(package.metadata.as_ref()),
        })
    }

    /// Observe the nearest package scope used to derive a source file's
    /// implied Node format. A present manifest with no `type` field remains
    /// the nearest scope; lookup never falls through to an outer package.
    pub fn package_scope_for_file(
        &mut self,
        file: &Path,
    ) -> Result<Option<PackageMetadata>, ResolutionError> {
        let current_directory = self.current_directory_text()?;
        let file = normalize_absolute_path(file, Some(current_directory))?;
        Ok(self
            .find_nearest_package_scope(&directory_name(&file))?
            .map(|package| package.metadata.as_ref().clone()))
    }

    /// Resolve a bare package name through a package `exports` map.
    ///
    /// tsc-port: resolveModuleName @6.0.3
    /// tsc-hash: 13b7d3828132093e6470153f00f485a147414f9ff08a1d72d5db8b593a76cad0
    /// tsc-span: _tsc.js:40649-40716
    pub fn resolve(
        &mut self,
        containing_file: &Path,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        Ok(self
            .resolve_with_facts(containing_file, specifier, mode)?
            .into_outcome())
    }

    /// Resolve a bare `extends` specifier with TypeScript's NodeNext JSON
    /// config lookup surface. This shares the package-scope, exports-map,
    /// package-field, path-containment, and ancestor-search machinery used by
    /// ordinary production resolution while restricting probes to JSON config
    /// files.
    ///
    /// tsc-port: nodeNextJsonConfigResolver @6.0.3
    /// tsc-hash: 04dc60dd54f17108edbfd7553495d135e945fc0a19efd3ff149b8e8aa9b31cc2
    /// tsc-span: _tsc.js:40925-40942
    pub(crate) fn resolve_json_config(
        &mut self,
        containing_file: &Path,
        specifier: &str,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        // nodeNextJsonConfigResolver is invoked without a package-json cache.
        // Preserve repeated host observations within one imports/self/fallback
        // walk instead of reusing the ordinary production resolver cache.
        let previous_cache_state = self.package_cache_enabled;
        self.package_cache_enabled = false;
        let result = self.resolve_json_config_uncached(containing_file, specifier);
        self.package_cache_enabled = previous_cache_state;
        result
    }

    fn resolve_json_config_uncached(
        &mut self,
        containing_file: &Path,
        specifier: &str,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.validate_common_configuration()?;
        let current_directory = self.current_directory_text()?;
        let containing_file = normalize_absolute_path(containing_file, Some(current_directory))?;
        let containing_directory = directory_name(&containing_file);
        if is_relative_specifier(specifier) {
            return self.resolve_relative_with_passes(
                &containing_directory,
                specifier,
                ResolutionMode::Unspecified,
                &[ExtensionProbePass::JsonConfig],
                /* optional_follow_realpath */ false,
            );
        }
        if specifier.starts_with('#') {
            if let Search::Terminal(outcome) = self.resolve_package_imports(
                &containing_directory,
                specifier,
                ResolutionMode::Unspecified,
                ExtensionProbePass::JsonConfig,
                /* force_enabled */ true,
                /* use_package_exports */ true,
                Some(99),
            )? {
                return Ok(outcome);
            }
        }
        let request = parse_package_request(specifier)?;

        if let Search::Terminal(outcome) = self.try_self_reference(
            &containing_directory,
            &request,
            ResolutionMode::Unspecified,
            ExtensionProbePass::JsonConfig,
            Some(99),
        )? {
            return Ok(outcome);
        }
        if specifier.contains(':') {
            return Ok(ResolutionOutcome::NotFound);
        }

        for ancestor in ancestor_directories(&containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }
            let package_root = package_root_for_request(&node_modules, &request);
            let specific = self.resolve_specific_package(
                &package_root,
                &request.exports_subpath,
                ExtensionProbePass::JsonConfig,
                ResolutionMode::Unspecified,
                /* use_package_exports */ true,
                Some(99),
                /* follow_realpath */ false,
            )?;
            if matches!(specific.outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(specific.outcome);
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    /// Resolve a module while retaining diagnostic-only facts which remain
    /// observable when the primary outcome is `NotFound`.
    ///
    /// Classic and Node10 are admitted only through this module-resolution
    /// entry point. Type-reference resolution retains its modern-resolver
    /// boundary below.
    pub fn resolve_with_facts(
        &mut self,
        containing_file: &Path,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<HostModuleResolution, ResolutionError> {
        self.validate_supported_module_configuration(mode)?;
        let current_directory = self.current_directory_text()?;
        let containing_file = normalize_absolute_path(containing_file, Some(current_directory))?;
        let containing_directory = directory_name(&containing_file);
        match self.options.emit_module_resolution_kind() {
            1 => return self.resolve_classic(&containing_file, specifier, mode),
            2 => return self.resolve_node10(&containing_file, specifier, mode),
            _ => {}
        }
        if is_relative_specifier(specifier) {
            return self
                .resolve_relative(&containing_file, specifier, mode)
                .map(|outcome| HostModuleResolution::new(outcome, None));
        }

        self.resolve_non_relative(&containing_directory, specifier, mode)
            .map(|outcome| HostModuleResolution::new(outcome, None))
    }

    /// tsc-port: tryLoadModuleUsingOptionalResolutionSettings @6.0.3
    /// tsc-hash: ee22a81bb770c0dd9e251189adb12da01762e6557d3c9c770616ff4affb7dd3d
    /// tsc-span: _tsc.js:40717-40749
    /// tsc-port: tryLoadModuleUsingPaths @6.0.3
    /// tsc-hash: f79098a1c1d51c3d0b6e955bc3e8c700491405adc3f4606ae7bb2044219399dd
    /// tsc-span: _tsc.js:42036-42061
    ///
    /// A matching `paths` key owns the optional-settings attempt even when all
    /// of its substitutions miss. That suppresses `baseUrl`, or `rootDirs`
    /// for a rooted disk specifier; the caller must still continue to its
    /// ordinary Classic or Node lookup.
    fn resolve_using_optional_settings(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        loader: OptionalResolutionLoader,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let has_paths = self
            .paths
            .as_deref()
            .is_some_and(|paths| !paths.entries().is_empty());
        let path_relative = is_path_relative_specifier(specifier);
        let external_relative = is_relative_specifier(specifier);
        if path_relative {
            return self.resolve_using_root_dirs(
                containing_directory,
                specifier,
                probe_pass,
                mode,
                loader,
                follow_realpath,
            );
        }
        if !has_paths {
            if external_relative {
                return self.resolve_using_root_dirs(
                    containing_directory,
                    specifier,
                    probe_pass,
                    mode,
                    loader,
                    follow_realpath,
                );
            }
            if self.base_url.is_none() {
                return Ok(ResolutionOutcome::NotFound);
            }
        }
        validate_owned_path_text(specifier, "module specifier", /* allow_empty */ true)?;

        if let Some((mapping_index, capture)) = self.matching_paths(specifier) {
            let substitution_count = self
                .paths
                .as_deref()
                .expect("a matching paths index has a shared mapping owner")
                .entries()[mapping_index]
                .substitutions()
                .len();
            for substitution_index in 0..substitution_count {
                // Build the owned candidate before any mutable resolver work.
                // The mapping table and its raw substitution stay borrowed
                // only for this scope, so the common exact/empty-capture path
                // performs no intermediate String/Vec clone.
                let (candidate, extension) = {
                    let paths = self
                        .paths
                        .as_deref()
                        .expect("a matching paths index has a shared mapping owner");
                    let substitution =
                        &paths.entries()[mapping_index].substitutions()[substitution_index];
                    let expanded = match capture.as_ref() {
                        Some(capture) if !capture.is_empty() => Cow::Owned(js_replace_first_star(
                            substitution,
                            &specifier[capture.clone()],
                        )?),
                        None | Some(_) => Cow::Borrowed(substitution.as_str()),
                    };
                    let base_directory = self
                        .paths_base_directory
                        .as_deref()
                        .expect("paths mappings have an effective base directory");
                    (
                        normalize_optional_candidate(&expanded, base_directory)?,
                        recognized_module_extension(substitution),
                    )
                };

                // tryLoadModuleUsingPaths probes a substitution whose raw text
                // has a recognized extension exactly before invoking the
                // extension-family loader. The raw text is intentional: a
                // wildcard capture which happens to end in `.ts` does not
                // enable this shortcut.
                if let Some(extension) = extension {
                    if let Some(resolved_path) = self.try_file(&candidate)? {
                        let external = path_contains_node_modules(resolved_path.as_ref());
                        return self.finish_legacy_resolution(
                            None,
                            resolved_path.as_ref(),
                            extension,
                            LegacyResolutionContext {
                                is_external_library_import: external,
                                attach_package_id: false,
                                resolved_using_ts_extension: false,
                                follow_realpath: external && !external_relative && follow_realpath,
                            },
                        );
                    }
                }

                // tryLoadModuleUsingPaths performs this caller-side latch
                // before invoking either loader. Node's loader and the
                // extension-adder then observe the same parent again; those
                // repeated host calls are intentionally observable.
                if !self
                    .host
                    .directory_exists(Path::new(&directory_name(&candidate)))?
                {
                    continue;
                }

                let outcome = self.probe_optional_candidate(
                    &candidate,
                    probe_pass,
                    mode,
                    loader,
                    external_relative,
                    follow_realpath,
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            }
            return Ok(ResolutionOutcome::NotFound);
        }

        // Rooted disk paths are external-relative module names, but unlike
        // dot-relative names they are still eligible for `paths`. A matching
        // paths key owns a miss above; only a non-match continues here.
        if external_relative {
            return self.resolve_using_root_dirs(
                containing_directory,
                specifier,
                probe_pass,
                mode,
                loader,
                follow_realpath,
            );
        }

        let Some(base_url) = self.base_url.as_deref() else {
            return Ok(ResolutionOutcome::NotFound);
        };
        let candidate = normalize_optional_candidate(specifier, base_url)?;
        // tryLoadModuleUsingBaseUrl owns the same caller-side parent latch as
        // paths substitutions before handing the candidate to its loader.
        if !self
            .host
            .directory_exists(Path::new(&directory_name(&candidate)))?
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.probe_optional_candidate(
            &candidate,
            probe_pass,
            mode,
            loader,
            /* external_relative */ false,
            follow_realpath,
        )
    }

    /// tsc-port: tryLoadModuleUsingRootDirs @6.0.3
    /// tsc-hash: 40c8e65f00c7a16b6bc000a46a7aaadf71c8536799f9d3c50cee2bee4bb244b4
    /// tsc-span: _tsc.js:40750-40808
    fn resolve_using_root_dirs(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        loader: OptionalResolutionLoader,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let candidate = preserve_trailing_directory_separator(
            normalize_absolute_path(Path::new(specifier), Some(containing_directory))?,
            specifier,
        );
        let candidates = {
            let Some(root_dirs) = self.root_dirs.as_deref().filter(|roots| !roots.is_empty())
            else {
                return Ok(ResolutionOutcome::NotFound);
            };

            let mut matched: Option<(usize, String)> = None;
            for (index, root_dir) in root_dirs.iter().enumerate() {
                let prefix = if root_dir.ends_with('/') {
                    root_dir.clone()
                } else {
                    format!("{root_dir}/")
                };
                if candidate.starts_with(&prefix)
                    && matched
                        .as_ref()
                        .is_none_or(|(_, current)| current.len() < prefix.len())
                {
                    matched = Some((index, prefix));
                }
            }
            let Some((matched_index, matched_prefix)) = matched else {
                return Ok(ResolutionOutcome::NotFound);
            };
            let suffix = candidate[matched_prefix.len()..].to_owned();
            let matched_root = &root_dirs[matched_index];
            let mut candidates = Vec::with_capacity(root_dirs.len());
            candidates.push((candidate, containing_directory.to_owned()));
            for root_dir in root_dirs {
                // Upstream compares rootDir strings, so equal duplicate roots
                // are all skipped after the first longest-prefix match.
                if root_dir == matched_root {
                    continue;
                }
                let candidate = preserve_trailing_directory_separator(
                    normalize_absolute_path(Path::new(&join_normalized(root_dir, &suffix)), None)?,
                    &suffix,
                );
                let base_directory = directory_name(&candidate);
                candidates.push((candidate, base_directory));
            }
            candidates
        };

        for (candidate, preflight_directory) in candidates {
            // Upstream converts a missing preflight directory into
            // `onlyRecordFailures`, which suppresses every loader host query
            // for this candidate. Host failures remain observable.
            if !self
                .host
                .directory_exists(Path::new(&preflight_directory))?
            {
                continue;
            }
            let outcome = self.probe_optional_candidate(
                &candidate,
                probe_pass,
                mode,
                loader,
                /* external_relative */ true,
                follow_realpath,
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn matching_paths(&self, specifier: &str) -> Option<(usize, Option<std::ops::Range<usize>>)> {
        let paths = self.paths.as_deref()?;
        if let Some(index) = paths.exact_mapping_index(specifier) {
            // The exact empty key wins inside `matchPatternOrExact`, but the
            // returned empty string is falsey at `tryLoadModuleUsingPaths`.
            // It therefore suppresses wildcard selection without owning the
            // optional-settings attempt itself.
            if specifier.is_empty() {
                return None;
            }
            return Some((index, None));
        }

        let mut best: Option<(usize, usize, std::ops::Range<usize>)> = None;
        for (index, star) in paths.wildcard_patterns() {
            let mapping = &paths.entries()[index];
            let pattern = mapping.pattern();
            let prefix = &pattern[..star];
            let suffix = &pattern[star + 1..];
            if !specifier.starts_with(prefix)
                || !specifier.ends_with(suffix)
                || specifier.len() < prefix.len() + suffix.len()
            {
                continue;
            }
            if best
                .as_ref()
                .is_some_and(|(_, longest_prefix, _)| *longest_prefix >= prefix.len())
            {
                continue;
            }
            let capture = prefix.len()..specifier.len() - suffix.len();
            best = Some((index, prefix.len(), capture));
        }
        best.map(|(index, _, capture)| (index, Some(capture)))
    }

    fn probe_optional_candidate(
        &mut self,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        loader: OptionalResolutionLoader,
        external_relative: bool,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        match loader {
            OptionalResolutionLoader::Classic => self.probe_classic_file(
                candidate,
                probe_pass,
                !external_relative && follow_realpath,
            ),
            OptionalResolutionLoader::Node => self.probe_optional_node_candidate(
                candidate,
                probe_pass,
                mode,
                external_relative,
                follow_realpath,
            ),
        }
    }

    fn probe_optional_node_candidate(
        &mut self,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        external_relative: bool,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let allow_implicit = !self.is_node_esm_mode(mode);
        let context = LegacyResolutionContext {
            is_external_library_import: false,
            attach_package_id: false,
            resolved_using_ts_extension: is_typescript_family_specifier(candidate),
            follow_realpath: false,
        };
        if !candidate.ends_with('/') {
            // nodeLoadModuleByRelativeName turns a missing parent into
            // `onlyRecordFailures` before its file loader runs. That also
            // suppresses the later candidate-directory/package probes.
            if !self
                .host
                .directory_exists(Path::new(&directory_name(candidate)))?
            {
                return Ok(ResolutionOutcome::NotFound);
            }
            let outcome =
                self.probe_legacy_file(None, candidate, probe_pass, allow_implicit, context)?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return self.finalize_optional_node_resolution(
                    outcome,
                    external_relative,
                    /* attach_direct_package */ true,
                    follow_realpath,
                );
            }
        }
        let candidate_exists = self.host.directory_exists(Path::new(candidate))?;
        if !allow_implicit || !candidate_exists {
            return Ok(ResolutionOutcome::NotFound);
        }

        let package_json = join_normalized(candidate, "package.json");
        if let Some(directory_package) = self.load_package(&package_json)? {
            let outcome = self.resolve_legacy_package(
                &directory_package,
                ".",
                probe_pass,
                mode,
                LegacyResolutionContext {
                    attach_package_id: true,
                    resolved_using_ts_extension: false,
                    follow_realpath: false,
                    ..context
                },
                Some(candidate),
                /* allow_node_esm_index_fallback */ true,
            )?;
            return self.finalize_optional_node_resolution(
                outcome,
                external_relative,
                /* attach_direct_package */ false,
                follow_realpath,
            );
        }
        let outcome = self.probe_legacy_file(
            None,
            &join_normalized(candidate, "index"),
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                attach_package_id: false,
                resolved_using_ts_extension: false,
                follow_realpath: false,
                ..context
            },
        )?;
        self.finalize_optional_node_resolution(
            outcome,
            external_relative,
            /* attach_direct_package */ false,
            follow_realpath,
        )
    }

    fn finalize_optional_node_resolution(
        &mut self,
        outcome: ResolutionOutcome<HostResolvedModule>,
        external_relative: bool,
        attach_direct_package: bool,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let ResolutionOutcome::Resolved(mut module) = outcome else {
            return Ok(ResolutionOutcome::NotFound);
        };
        let lexical_path = module
            .resolved_file
            .display()
            .to_str()
            .ok_or_else(|| {
                ResolutionError::canonicalization(
                    Some(module.resolved_file.display().to_path_buf()),
                    "resolved module path is not valid Unicode",
                )
            })?
            .to_owned();
        module.is_external_library_import = path_contains_node_modules(&lexical_path);
        if attach_direct_package {
            self.attach_direct_node_package(&mut module)?;
        }
        if module.is_external_library_import && !external_relative && follow_realpath {
            self.follow_module_realpath(&mut module)?;
        }
        Ok(ResolutionOutcome::Resolved(module))
    }

    /// Attach the package facts used by `nodeLoadModuleByRelativeName` after
    /// a direct file probe has succeeded. Local candidates do not search
    /// ancestor manifests; an external optional-setting or type-reference
    /// candidate consults only its actual `node_modules` package root.
    fn attach_direct_node_package(
        &mut self,
        module: &mut HostResolvedModule,
    ) -> Result<(), ResolutionError> {
        let lexical_path = module
            .original_path
            .as_ref()
            .unwrap_or(&module.resolved_file)
            .display()
            .to_str()
            .ok_or_else(|| {
                ResolutionError::canonicalization(
                    Some(module.resolved_file.display().to_path_buf()),
                    "resolved module path is not valid Unicode",
                )
            })?
            .to_owned();
        // parseNodeModuleFromPath normalizes the selected file before locating
        // its last node_modules package, while withPackageId still slices the
        // resolver's raw selected spelling for submoduleName.
        let normalized_lexical_path = normalize_absolute_path(
            Path::new(&lexical_path),
            Some(self.current_directory_text()?),
        )?;
        let Some(package_root) = node_modules_package_root(&normalized_lexical_path) else {
            return Ok(());
        };
        let Some(package) = self.load_package(&join_normalized(&package_root, "package.json"))?
        else {
            return Ok(());
        };
        module.package_id = package_id_for_legacy_path_from_directory(
            &package,
            &package_root,
            &lexical_path,
            true,
        )?;
        module.package_metadata = Some(Rc::clone(&package.metadata));
        Ok(())
    }

    fn classify_selected_path_external(
        &self,
        module: &mut HostResolvedModule,
    ) -> Result<(), ResolutionError> {
        let selected_path = module
            .original_path
            .as_ref()
            .unwrap_or(&module.resolved_file)
            .display()
            .to_str()
            .ok_or_else(|| {
                ResolutionError::canonicalization(
                    Some(module.resolved_file.display().to_path_buf()),
                    "selected module path is not valid Unicode",
                )
            })?;
        module.is_external_library_import = path_contains_node_modules(selected_path);
        Ok(())
    }

    fn follow_module_realpath(
        &self,
        module: &mut HostResolvedModule,
    ) -> Result<(), ResolutionError> {
        let lexical_path = module
            .resolved_file
            .display()
            .to_str()
            .ok_or_else(|| {
                ResolutionError::canonicalization(
                    Some(module.resolved_file.display().to_path_buf()),
                    "resolved module path is not valid Unicode",
                )
            })?
            .to_owned();
        let (resolved_file, original_path) = self.realpath_program_path(
            &lexical_path,
            module.realpath_may_be_missing_after_suffix_predicate,
        )?;
        module.resolved_file = resolved_file;
        module.original_path = original_path;
        module.realpath_may_be_missing_after_suffix_predicate = false;
        Ok(())
    }

    /// Probe the file phase shared by custom type roots and relative
    /// type-reference directives. The successful lexical path determines its
    /// actual `node_modules` package before every type-reference result follows
    /// realpath, including local files.
    fn probe_direct_type_reference_file(
        &mut self,
        candidate: &str,
        mode: ResolutionMode,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let context = LegacyResolutionContext {
            is_external_library_import: false,
            attach_package_id: false,
            resolved_using_ts_extension: false,
            follow_realpath: false,
        };
        let outcome = self.probe_legacy_file(
            None,
            candidate,
            ExtensionProbePass::Declaration,
            /* allow_implicit */ !self.is_node_esm_mode(mode),
            context,
        )?;
        let ResolutionOutcome::Resolved(mut module) = outcome else {
            return Ok(ResolutionOutcome::NotFound);
        };
        self.classify_selected_path_external(&mut module)?;
        self.attach_direct_node_package(&mut module)?;
        if follow_realpath {
            self.follow_module_realpath(&mut module)?;
        }
        Ok(ResolutionOutcome::Resolved(module))
    }

    /// tsc-port: classicNameResolver @6.0.3
    /// tsc-hash: d928985c6c8e588d5b3e35a9135bf163db3cb28ca5f94d1580e68d218230341b
    /// tsc-span: _tsc.js:42110-42186
    ///
    /// The owned Classic slice includes optional `paths`/`baseUrl`, legacy
    /// ancestor file search, and its nearest automatic `node_modules/@types`
    /// fallback in the upstream extension-pass order.
    fn resolve_classic(
        &mut self,
        containing_file: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<HostModuleResolution, ResolutionError> {
        if specifier.contains('\0') {
            return Err(ResolutionError::invalid_data(format!(
                "invalid Classic module specifier {specifier:?}"
            )));
        }
        let containing_directory = directory_name(containing_file);
        let relative = is_relative_specifier(specifier);
        let mut request = None;

        for probe_pass in [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback] {
            if relative {
                let optional = self.resolve_using_optional_settings(
                    &containing_directory,
                    specifier,
                    probe_pass,
                    mode,
                    OptionalResolutionLoader::Classic,
                    /* follow_realpath */ true,
                )?;
                if matches!(optional, ResolutionOutcome::Resolved(_)) {
                    return Ok(HostModuleResolution::new(optional, None));
                }
                let candidate = preserve_trailing_directory_separator(
                    normalize_absolute_path(Path::new(specifier), Some(&containing_directory))?,
                    specifier,
                );
                let outcome = self.probe_classic_file(
                    &candidate, probe_pass, /* follow_external_realpath */ false,
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(HostModuleResolution::new(outcome, None));
                }
            } else {
                let optional = self.resolve_using_optional_settings(
                    &containing_directory,
                    specifier,
                    probe_pass,
                    mode,
                    OptionalResolutionLoader::Classic,
                    /* follow_realpath */ true,
                )?;
                if matches!(optional, ResolutionOutcome::Resolved(_)) {
                    return Ok(HostModuleResolution::new(optional, None));
                }
                if request.is_none() {
                    request = Some(parse_package_request(specifier)?);
                }
                for ancestor in ancestor_directories(&containing_directory) {
                    let candidate = normalize_absolute_path(
                        Path::new(&join_normalized(&ancestor, specifier)),
                        None,
                    )?;
                    let outcome = self.probe_classic_file(
                        &candidate, probe_pass, /* follow_external_realpath */ true,
                    )?;
                    if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                        return Ok(HostModuleResolution::new(outcome, None));
                    }
                }
                if matches!(probe_pass, ExtensionProbePass::Preferred) {
                    let outcome = self.resolve_legacy_at_types(
                        &containing_directory,
                        request.as_ref().expect("non-relative request was parsed"),
                        mode,
                    )?;
                    if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                        return Ok(HostModuleResolution::new(outcome, None));
                    }
                }
            }
        }
        Ok(HostModuleResolution::new(ResolutionOutcome::NotFound, None))
    }

    fn probe_classic_file(
        &mut self,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        follow_external_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let outcome = self.probe_legacy_file(
            None,
            candidate,
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                is_external_library_import: false,
                attach_package_id: false,
                resolved_using_ts_extension: is_typescript_family_specifier(candidate),
                follow_realpath: false,
            },
        )?;
        let ResolutionOutcome::Resolved(mut module) = outcome else {
            return Ok(ResolutionOutcome::NotFound);
        };
        self.classify_selected_path_external(&mut module)?;
        if module.is_external_library_import && follow_external_realpath {
            self.follow_module_realpath(&mut module)?;
        }
        Ok(ResolutionOutcome::Resolved(module))
    }

    /// tsc-port: nodeModuleNameResolverWorker @6.0.3 (Node10 branch and
    /// diagnostic Bundler retry)
    /// tsc-hash: ccf7790e149deb18d5f0d7ebb0c71377781e460ec97b5e8c5d332298727be3f3
    /// tsc-span: _tsc.js:40935-41020
    fn resolve_node10(
        &mut self,
        containing_file: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<HostModuleResolution, ResolutionError> {
        if is_relative_specifier(specifier) {
            let outcome = self.resolve_relative(containing_file, specifier, mode)?;
            return Ok(HostModuleResolution::new(outcome, None));
        }
        let containing_directory = directory_name(containing_file);
        let (mut outcome, resolved_package_directory) =
            self.resolve_node10_non_relative(&containing_directory, specifier, mode)?;
        let wanted_types_but_got_other = match &outcome {
            ResolutionOutcome::NotFound => false,
            ResolutionOutcome::Resolved(module) => {
                !is_typescript_module_extension(module.extension())
            }
        };
        let retry_with_exports_disabled = resolved_package_directory
            && mode == ResolutionMode::EsNext
            && match &outcome {
                ResolutionOutcome::Resolved(module) => {
                    module.is_external_library_import() && wanted_types_but_got_other
                }
                ResolutionOutcome::NotFound => false,
            };
        let retry_with_bundler = resolved_package_directory
            && !retry_with_exports_disabled
            && match &outcome {
                ResolutionOutcome::NotFound => true,
                ResolutionOutcome::Resolved(_) => wanted_types_but_got_other,
            };
        let alternate_result = if retry_with_exports_disabled || retry_with_bundler {
            let request = parse_package_request(specifier)?;
            let alternate = if retry_with_exports_disabled {
                self.resolve_modern_preferred_without_exports(
                    &containing_directory,
                    specifier,
                    &request,
                    mode,
                    ExtensionProbePass::Preferred,
                    /* force_package_maps */ true,
                    /* resolution_kind */ 2,
                )?
            } else {
                self.resolve_bundler_preferred_non_relative(
                    &containing_directory,
                    specifier,
                    &request,
                    ExtensionProbePass::Preferred,
                    /* enable_package_maps */ mode != ResolutionMode::Unspecified,
                )?
            };
            match alternate {
                ResolutionOutcome::Resolved(module) if module.is_external_library_import() => {
                    Some(module.resolved_file().clone())
                }
                ResolutionOutcome::Resolved(_) | ResolutionOutcome::NotFound => None,
            }
        } else {
            None
        };
        if let ResolutionOutcome::Resolved(module) = &mut outcome {
            if module.is_external_library_import() {
                self.follow_module_realpath(module)?;
            }
        }
        Ok(HostModuleResolution::new(outcome, alternate_result))
    }

    fn resolve_node10_non_relative(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<(ResolutionOutcome<HostResolvedModule>, bool), ResolutionError> {
        let mut resolved_package_directory = false;
        let mut request = None;
        let all_features = mode != ResolutionMode::Unspecified;
        for probe_pass in [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback] {
            let optional = self.resolve_using_optional_settings(
                containing_directory,
                specifier,
                probe_pass,
                mode,
                OptionalResolutionLoader::Node,
                /* follow_realpath */ false,
            )?;
            if matches!(optional, ResolutionOutcome::Resolved(_)) {
                // Upstream sets `resolvedPackageDirectory` only while walking
                // the ordinary node_modules package lookup, not when an
                // optional paths/baseUrl candidate happens to carry metadata.
                return Ok((optional, resolved_package_directory));
            }

            if all_features && specifier.starts_with('#') {
                if let Search::Terminal(outcome) = self.resolve_package_imports(
                    containing_directory,
                    specifier,
                    mode,
                    probe_pass,
                    /* force_enabled */ true,
                    /* use_package_exports */ true,
                    /* resolution_kind */ None,
                )? {
                    return Ok((outcome, resolved_package_directory));
                }
            }
            if request.is_none() {
                request = Some(parse_package_request(specifier)?);
            }
            let request = request.as_ref().expect("non-relative request was parsed");
            if all_features {
                if let Search::Terminal(outcome) = self.try_self_reference(
                    containing_directory,
                    request,
                    mode,
                    probe_pass,
                    /* resolution_kind */ None,
                )? {
                    return Ok((outcome, resolved_package_directory));
                }
            }
            // nodeModuleNameResolverWorker gives optional settings, package
            // imports, and SelfName an opportunity to own URI-looking names
            // before suppressing the ordinary node_modules walk.
            if specifier.contains(':') {
                continue;
            }
            for ancestor in ancestor_directories(containing_directory) {
                if base_name(&ancestor) == "node_modules" {
                    continue;
                }
                let node_modules = join_normalized(&ancestor, "node_modules");
                if !self.host.directory_exists(Path::new(&node_modules))? {
                    continue;
                }
                let package_root = package_root_for_request(&node_modules, request);
                let specific = self.resolve_specific_package(
                    &package_root,
                    &request.exports_subpath,
                    probe_pass,
                    mode,
                    /* use_package_exports */ all_features,
                    None,
                    /* follow_realpath */ false,
                )?;
                // A nested-package early branch returns before tsc stamps
                // resolvedPackageDirectory; only the normal root path enables
                // the diagnostic-only Bundler retry.
                resolved_package_directory |= specific.root_package_observed;
                let outcome = specific.outcome;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok((outcome, resolved_package_directory));
                }

                if matches!(probe_pass, ExtensionProbePass::Preferred) {
                    if all_features {
                        let at_types = join_normalized(&node_modules, "@types");
                        if self.host.directory_exists(Path::new(&at_types))? {
                            let package_root = types_package_root_for_request(&at_types, request);
                            let specific = self.resolve_specific_package(
                                &package_root,
                                &request.exports_subpath,
                                ExtensionProbePass::Declaration,
                                mode,
                                /* use_package_exports */ true,
                                None,
                                /* follow_realpath */ false,
                            )?;
                            resolved_package_directory |= specific.root_package_observed;
                            if matches!(specific.outcome, ResolutionOutcome::Resolved(_)) {
                                return Ok((specific.outcome, resolved_package_directory));
                            }
                        }
                    } else {
                        let (outcome, at_types_package_observed) = self
                            .resolve_legacy_at_types_from_node_modules(
                                &node_modules,
                                request,
                                mode,
                                /* follow_realpath */ false,
                            )?;
                        resolved_package_directory |= at_types_package_observed;
                        if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                            return Ok((outcome, resolved_package_directory));
                        }
                    }
                }
            }
            if matches!(probe_pass, ExtensionProbePass::Preferred) {
                let outcome = self.resolve_module_from_type_roots(specifier, mode)?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok((outcome, resolved_package_directory));
                }
            }
        }
        Ok((ResolutionOutcome::NotFound, resolved_package_directory))
    }

    fn resolve_legacy_at_types(
        &mut self,
        containing_directory: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        for ancestor in ancestor_directories(containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }
            let (outcome, _) = self.resolve_legacy_at_types_from_node_modules(
                &node_modules,
                request,
                mode,
                /* follow_realpath */ true,
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn resolve_legacy_at_types_from_node_modules(
        &mut self,
        node_modules: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
        follow_realpath: bool,
    ) -> Result<(ResolutionOutcome<HostResolvedModule>, bool), ResolutionError> {
        let at_types = join_normalized(node_modules, "@types");
        if !self.host.directory_exists(Path::new(&at_types))? {
            return Ok((ResolutionOutcome::NotFound, false));
        }
        let package_root = types_package_root_for_request(&at_types, request);
        let specific = self.resolve_specific_package(
            &package_root,
            &request.exports_subpath,
            ExtensionProbePass::Declaration,
            mode,
            /* use_package_exports */ false,
            None,
            follow_realpath,
        )?;
        Ok((specific.outcome, specific.root_package_observed))
    }

    fn resolve_bundler_preferred_non_relative(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        request: &PackageRequest<'_>,
        probe_pass: ExtensionProbePass,
        enable_package_maps: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let diagnostic_mode = ResolutionMode::EsNext;
        let optional = self.resolve_using_optional_settings(
            containing_directory,
            specifier,
            probe_pass,
            diagnostic_mode,
            OptionalResolutionLoader::Node,
            /* follow_realpath */ false,
        )?;
        if matches!(optional, ResolutionOutcome::Resolved(_)) {
            return Ok(optional);
        }
        if enable_package_maps && specifier.starts_with('#') {
            if let Search::Terminal(outcome) = self.resolve_package_imports(
                containing_directory,
                specifier,
                diagnostic_mode,
                probe_pass,
                /* force_enabled */ true,
                /* use_package_exports */ true,
                /* resolution_kind */ Some(100),
            )? {
                return Ok(outcome);
            }
        }
        if enable_package_maps {
            if let Search::Terminal(outcome) = self.try_self_reference(
                containing_directory,
                request,
                diagnostic_mode,
                probe_pass,
                /* resolution_kind */ Some(100),
            )? {
                return Ok(outcome);
            }
        }
        if specifier.contains(':') {
            return Ok(ResolutionOutcome::NotFound);
        }
        if matches!(probe_pass, ExtensionProbePass::Empty) {
            return Ok(ResolutionOutcome::NotFound);
        }
        for ancestor in ancestor_directories(containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }
            let package_root = package_root_for_request(&node_modules, request);
            let outcome = self
                .resolve_specific_package(
                    &package_root,
                    &request.exports_subpath,
                    probe_pass,
                    diagnostic_mode,
                    /* use_package_exports */ true,
                    Some(100),
                    /* follow_realpath */ false,
                )?
                .outcome;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }

            if probe_pass_has_declaration(probe_pass) {
                let types_package = PackageRequest {
                    package_name: request.package_name,
                    exports_subpath: request.exports_subpath.clone(),
                    trailing_separator: request.trailing_separator,
                };
                let outcome = self.resolve_bundler_preferred_at_types(
                    &node_modules,
                    &types_package,
                    diagnostic_mode,
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            }
        }
        if probe_pass_has_declaration(probe_pass) {
            self.resolve_module_from_type_roots(specifier, diagnostic_mode)
        } else {
            Ok(ResolutionOutcome::NotFound)
        }
    }

    fn resolve_bundler_preferred_at_types(
        &mut self,
        node_modules: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let at_types = join_normalized(node_modules, "@types");
        if !self.host.directory_exists(Path::new(&at_types))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        let package_root = types_package_root_for_request(&at_types, request);
        self.resolve_specific_package(
            &package_root,
            &request.exports_subpath,
            ExtensionProbePass::Declaration,
            mode,
            /* use_package_exports */ true,
            Some(100),
            /* follow_realpath */ false,
        )
        .map(|result| result.outcome)
    }

    /// Resolve one source-owned or automatic type-reference directive.
    ///
    /// `type_roots` preserves the compiler option's three observable states:
    /// `None` computes the default ancestor `node_modules/@types` roots,
    /// `Some([])` has no primary roots, and a non-empty slice is searched in
    /// the declared order. A supported miss is `NotFound`; host and unsupported
    /// resolver failures remain typed errors.
    ///
    /// tsc-port: resolveTypeReferenceDirective @6.0.3
    /// tsc-hash: 5f070f09ecb058d7fdfc0df788d8305900e2ab87d4c0d7a6fc329e5ec4927519
    /// tsc-span: _tsc.js:40060-40250
    pub fn resolve_type_reference(
        &mut self,
        containing_file: &Path,
        specifier: &str,
        mode: ResolutionMode,
        type_roots: Option<&[ProgramPath]>,
    ) -> Result<ResolutionOutcome<HostResolvedTypeReferenceDirective>, ResolutionError> {
        self.validate_supported_type_reference_configuration(mode)?;

        let current_directory = self.current_directory_text()?.to_owned();
        let containing_file = normalize_absolute_path(containing_file, Some(&current_directory))?;
        let custom_type_roots = type_roots.is_some();
        let effective_type_roots = self.effective_type_roots(type_roots)?;

        for type_root in effective_type_roots {
            let outcome = self.resolve_type_reference_from_root(
                &type_root,
                specifier,
                mode,
                custom_type_roots,
                /* follow_realpath */ true,
            )?;
            if let ResolutionOutcome::Resolved(mut module) = outcome {
                self.classify_selected_path_external(&mut module)?;
                return Ok(ResolutionOutcome::Resolved(
                    HostResolvedTypeReferenceDirective::from_module(module, true),
                ));
            }
        }

        // Vendored createProgram uses this synthetic containing file for
        // automatic names. Explicit custom roots make that primary search
        // authoritative and suppress the ordinary node_modules fallback.
        if custom_type_roots && base_name(&containing_file) == "__inferred type names__.ts" {
            return Ok(ResolutionOutcome::NotFound);
        }

        let outcome = if is_relative_specifier(specifier) {
            self.resolve_relative_type_reference(&containing_file, specifier, mode)?
        } else {
            let Ok(request) = parse_package_request(specifier) else {
                return Ok(ResolutionOutcome::NotFound);
            };
            self.resolve_type_reference_from_node_modules(
                &directory_name(&containing_file),
                &request,
                mode,
                self.type_reference_uses_package_exports(mode),
            )?
        };
        Ok(match outcome {
            ResolutionOutcome::Resolved(mut module) => {
                self.classify_selected_path_external(&mut module)?;
                ResolutionOutcome::Resolved(HostResolvedTypeReferenceDirective::from_module(
                    module, false,
                ))
            }
            ResolutionOutcome::NotFound => ResolutionOutcome::NotFound,
        })
    }

    fn resolve_non_relative(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let containing_directory = normalize_absolute_path(
            Path::new(containing_directory),
            Some(self.current_directory_text()?),
        )?;
        let active = ActiveResolution {
            containing_directory: canonical_text(
                &containing_directory,
                self.path_context.use_case_sensitive_file_names(),
            ),
            specifier: specifier.to_owned(),
            mode,
        };
        if self.active_resolutions.contains(&active) {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.active_resolutions.push(active);
        let result = self.resolve_non_relative_inner(&containing_directory, specifier, mode);
        self.active_resolutions.pop();
        let mut outcome = result?;
        if let ResolutionOutcome::Resolved(module) = &mut outcome {
            if module.is_external_library_import() {
                self.follow_module_realpath(module)?;
            }
        }
        Ok(outcome)
    }

    fn resolve_bare_import_target(
        &mut self,
        owner_package: &CachedPackage,
        specifier: &str,
        mut context: ExportProbeContext,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        // A bare target from a config `imports` map re-enters the NodeNext
        // resolver with the JSON extension mask, but `isConfigLookup=false`.
        // It may therefore use JSON exports and written JSON subpaths, while
        // package `tsconfig`, default `tsconfig.json`, and extensionless JSON
        // probing stay disabled.
        if matches!(context.pass, ExtensionProbePass::JsonConfig) {
            context.pass = ExtensionProbePass::JsonModule;
        }
        let resolution_depth = self.active_resolutions.len();
        let package_map_depth = self.active_package_maps.len();
        let result = self.resolve_bare_import_target_worker(owner_package, specifier, context);
        // Host/probe failures remain observable errors, but a failed iterative
        // walk must not poison a resolver which the caller reuses afterwards.
        self.active_resolutions.truncate(resolution_depth);
        self.active_package_maps.truncate(package_map_depth);
        result
    }

    /// Evaluate package-import rewrites as an explicit continuation stack.
    ///
    /// `nodeModuleNameResolverWorker` recursively re-enters package imports
    /// for every bare target. A valid package.json can contain thousands of
    /// such redirects, which should not be limited by the Rust thread stack.
    /// Conditions and arrays are continuations here, so misses retain their
    /// exact fallback order while `paths`/`baseUrl` are still probed before
    /// each rewritten specifier.
    fn resolve_bare_import_target_worker(
        &mut self,
        owner_package: &CachedPackage,
        specifier: &str,
        context: ExportProbeContext,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let features = context.bare_features.ok_or_else(|| {
            ResolutionError::invalid_data("bare imports target is missing resolver features")
        })?;
        let owner_key = owner_package
            .metadata
            .package_json()
            .canonical()
            .as_path()
            .to_string_lossy()
            .into_owned();
        let mut budget_sources = BTreeSet::from([owner_key]);
        let mut work_budget = owner_package
            .metadata
            .text()
            .len()
            .saturating_mul(PACKAGE_MAP_REWRITE_INPUT_MULTIPLIER)
            .saturating_add(specifier.len())
            .max(MIN_PACKAGE_MAP_REWRITE_WORK_BUDGET);
        let mut work = 0usize;
        let mut previous_specifier_len = None;
        let mut frames = Vec::new();
        let mut state = ImportsTargetState::Bare {
            containing_directory: owner_package.root.clone(),
            specifier: specifier.to_owned(),
        };

        loop {
            state = match state {
                ImportsTargetState::Bare {
                    containing_directory,
                    specifier,
                } => {
                    let containing_directory = normalize_absolute_path(
                        Path::new(&containing_directory),
                        Some(self.current_directory_text()?),
                    )?;
                    // Count redirects plus only newly-created specifier bytes.
                    // A long caller-owned name is valid input, and a finite
                    // pattern chain may preserve all of it. Growing wildcard
                    // cycles, however, consume their cumulative positive
                    // expansion against the package-derived budget.
                    work = work.saturating_add(1).saturating_add(
                        previous_specifier_len
                            .map_or(0, |previous| specifier.len().saturating_sub(previous)),
                    );
                    previous_specifier_len = Some(specifier.len());
                    if work > work_budget {
                        return Err(ResolutionError::resource_limit(format!(
                            "package-import rewrite work exceeded the {work_budget}-unit budget derived from observed package.json input"
                        )));
                    }

                    let active = ActiveResolution {
                        containing_directory: canonical_text(
                            &containing_directory,
                            self.path_context.use_case_sensitive_file_names(),
                        ),
                        specifier: specifier.clone(),
                        mode: context.mode,
                    };
                    if self.active_resolutions.contains(&active) {
                        ImportsTargetState::Result(Search::Continue)
                    } else {
                        self.active_resolutions.push(active);
                        let relative = is_relative_specifier(&specifier);
                        let preliminary = if relative {
                            self.resolve_relative_with_passes(
                                &containing_directory,
                                &specifier,
                                context.mode,
                                &[context.pass],
                                /* optional_follow_realpath */ false,
                            )?
                        } else {
                            self.resolve_using_optional_settings(
                                &containing_directory,
                                &specifier,
                                context.pass,
                                context.mode,
                                OptionalResolutionLoader::Node,
                                /* follow_realpath */ false,
                            )?
                        };
                        if matches!(preliminary, ResolutionOutcome::Resolved(_)) || relative {
                            self.active_resolutions.pop();
                            ImportsTargetState::Result(self.finish_bare_import_target(
                                &containing_directory,
                                &specifier,
                                context,
                                features,
                                preliminary,
                                /* resolved_package_directory */ false,
                            )?)
                        } else {
                            let selected = if features.enable_imports
                                && specifier.starts_with('#')
                                && specifier != "#"
                                && !(features.resolution_kind == 3 && specifier.starts_with("#/"))
                            {
                                if let Some(package) =
                                    self.find_nearest_package_scope(&containing_directory)?
                                {
                                    let package_source = package
                                        .metadata
                                        .package_json()
                                        .canonical()
                                        .as_path()
                                        .to_string_lossy()
                                        .into_owned();
                                    if budget_sources.insert(package_source) {
                                        work_budget = work_budget.saturating_add(
                                            package.metadata.text().len().saturating_mul(
                                                PACKAGE_MAP_REWRITE_INPUT_MULTIPLIER,
                                            ),
                                        );
                                    }
                                    if let Some(table) =
                                        package.imports.as_ref().and_then(Value::as_object)
                                    {
                                        let selected = select_package_map_target(
                                            table,
                                            &specifier,
                                            context.exports_pattern_trailers,
                                        )
                                        .map(|selected| {
                                            (
                                                selected.target.clone(),
                                                selected.subpath,
                                                selected.pattern,
                                            )
                                        });
                                        selected.map(|(target, subpath, pattern)| {
                                            (package, target, subpath, pattern)
                                        })
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            if let Some((package, target, subpath, pattern)) = selected {
                                let package_key = canonical_text(
                                    &package.root,
                                    self.path_context.use_case_sensitive_file_names(),
                                );
                                self.active_package_maps.push(package_key);
                                frames.push(ImportsTargetFrame::BareAfterPackageMap {
                                    containing_directory,
                                    specifier,
                                    features,
                                });
                                ImportsTargetState::Target {
                                    package,
                                    target,
                                    subpath,
                                    pattern,
                                }
                            } else {
                                let (outcome, resolved_package_directory) = self
                                    .resolve_bare_import_target_tail(
                                        &containing_directory,
                                        &specifier,
                                        context.pass,
                                        context.mode,
                                        features,
                                    )?;
                                self.active_resolutions.pop();
                                ImportsTargetState::Result(self.finish_bare_import_target(
                                    &containing_directory,
                                    &specifier,
                                    context,
                                    features,
                                    outcome,
                                    resolved_package_directory,
                                )?)
                            }
                        }
                    }
                }
                ImportsTargetState::Target {
                    package,
                    target,
                    subpath,
                    pattern,
                } => match target {
                    Value::Null => {
                        ImportsTargetState::Result(Search::Terminal(ResolutionOutcome::NotFound))
                    }
                    Value::String(raw_target) => {
                        if !pattern && !subpath.is_empty() && !raw_target.ends_with('/') {
                            ImportsTargetState::Result(Search::Continue)
                        } else if !raw_target.starts_with("./") {
                            match expand_imports_bare_target(&raw_target, &subpath, pattern)? {
                                Some(specifier) => ImportsTargetState::Bare {
                                    containing_directory: package.root.clone(),
                                    specifier,
                                },
                                None => ImportsTargetState::Result(Search::Continue),
                            }
                        } else {
                            let Some(target) = expand_export_target(
                                &package.root,
                                &raw_target,
                                &subpath,
                                pattern,
                            )?
                            else {
                                state = ImportsTargetState::Result(Search::Continue);
                                continue;
                            };
                            let candidate = normalize_absolute_path(Path::new(&target), None)?;
                            if !path_is_within(&candidate, &package.root) {
                                ImportsTargetState::Result(Search::Continue)
                            } else {
                                let resolved = self.probe_export_target(
                                    &package,
                                    &candidate,
                                    context,
                                    /* attach_package_id */ true,
                                    /* raw_package_target */ Some(&raw_target),
                                )?;
                                if matches!(resolved, ResolutionOutcome::Resolved(_)) {
                                    ImportsTargetState::Result(Search::Terminal(resolved))
                                } else {
                                    ImportsTargetState::Result(Search::Continue)
                                }
                            }
                        }
                    }
                    Value::Object(conditions) => {
                        let mut remaining = js_own_property_entries(&conditions)
                            .into_iter()
                            .filter(|(condition, _)| {
                                self.package_condition_matches(
                                    condition,
                                    context.mode,
                                    context.resolution_kind,
                                )
                            })
                            .map(|(_, target)| target.clone())
                            .collect::<Vec<_>>()
                            .into_iter();
                        if let Some(target) = remaining.next() {
                            frames.push(ImportsTargetFrame::Sequence {
                                package: Rc::clone(&package),
                                remaining,
                                subpath: subpath.clone(),
                                pattern,
                            });
                            ImportsTargetState::Target {
                                package,
                                target,
                                subpath,
                                pattern,
                            }
                        } else {
                            ImportsTargetState::Result(Search::Continue)
                        }
                    }
                    Value::Array(targets) => {
                        let mut remaining = targets.into_iter();
                        if let Some(target) = remaining.next() {
                            frames.push(ImportsTargetFrame::Sequence {
                                package: Rc::clone(&package),
                                remaining,
                                subpath: subpath.clone(),
                                pattern,
                            });
                            ImportsTargetState::Target {
                                package,
                                target,
                                subpath,
                                pattern,
                            }
                        } else {
                            ImportsTargetState::Result(Search::Continue)
                        }
                    }
                    Value::Bool(_) | Value::Number(_) => {
                        ImportsTargetState::Result(Search::Continue)
                    }
                },
                ImportsTargetState::Result(result) => {
                    let Some(frame) = frames.pop() else {
                        return Ok(result);
                    };
                    match frame {
                        ImportsTargetFrame::Sequence {
                            package,
                            mut remaining,
                            subpath,
                            pattern,
                        } => match result {
                            Search::Terminal(outcome) => {
                                ImportsTargetState::Result(Search::Terminal(outcome))
                            }
                            Search::Continue => {
                                if let Some(target) = remaining.next() {
                                    frames.push(ImportsTargetFrame::Sequence {
                                        package: Rc::clone(&package),
                                        remaining,
                                        subpath: subpath.clone(),
                                        pattern,
                                    });
                                    ImportsTargetState::Target {
                                        package,
                                        target,
                                        subpath,
                                        pattern,
                                    }
                                } else {
                                    ImportsTargetState::Result(Search::Continue)
                                }
                            }
                        },
                        ImportsTargetFrame::BareAfterPackageMap {
                            containing_directory,
                            specifier,
                            features,
                        } => {
                            self.active_package_maps.pop();
                            let (outcome, resolved_package_directory) = match result {
                                Search::Terminal(outcome) => (outcome, false),
                                Search::Continue => self.resolve_bare_import_target_tail(
                                    &containing_directory,
                                    &specifier,
                                    context.pass,
                                    context.mode,
                                    features,
                                )?,
                            };
                            self.active_resolutions.pop();
                            ImportsTargetState::Result(self.finish_bare_import_target(
                                &containing_directory,
                                &specifier,
                                context,
                                features,
                                outcome,
                                resolved_package_directory,
                            )?)
                        }
                    }
                }
            };
        }
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the nested worker's observable result state.
    fn finish_bare_import_target(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        context: ExportProbeContext,
        features: BareResolutionFeatures,
        mut outcome: ResolutionOutcome<HostResolvedModule>,
        resolved_package_directory: bool,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        self.run_nested_diagnostic_retry(
            containing_directory,
            specifier,
            context.pass,
            context.mode,
            features,
            resolved_package_directory,
            &outcome,
        )?;
        if let ResolutionOutcome::Resolved(module) = &mut outcome {
            if module.is_external_library_import() {
                self.follow_module_realpath(module)?;
            }
            module.is_external_library_import = false;
            module.alternate_result = None;
            return Ok(Search::Terminal(outcome));
        }
        Ok(Search::Continue)
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the nested worker's observable retry state.
    fn run_nested_diagnostic_retry(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        features: BareResolutionFeatures,
        resolved_package_directory: bool,
        outcome: &ResolutionOutcome<HostResolvedModule>,
    ) -> Result<(), ResolutionError> {
        if !resolved_package_directory {
            return Ok(());
        }
        let wanted_types_but_got_other = matches!(
            outcome,
            ResolutionOutcome::Resolved(module)
                if probe_pass_has_declaration(probe_pass)
                    && !is_typescript_module_extension(module.extension())
        );
        let retry_without_exports = features.use_package_exports
            && self.package_condition_matches("import", mode, features.resolution_kind)
            && matches!(
                outcome,
                ResolutionOutcome::Resolved(module)
                    if module.is_external_library_import() && wanted_types_but_got_other
            );
        let retry_with_bundler = !retry_without_exports
            && features.resolution_kind == 2
            && (matches!(outcome, ResolutionOutcome::NotFound) || wanted_types_but_got_other);
        if !retry_without_exports && !retry_with_bundler {
            return Ok(());
        }
        let request = parse_package_request(specifier)?;
        let diagnostic_pass = preferred_diagnostic_pass(probe_pass);
        if retry_without_exports {
            let _ = self.resolve_modern_preferred_without_exports(
                containing_directory,
                specifier,
                &request,
                mode,
                diagnostic_pass,
                features.enable_imports,
                features.resolution_kind,
            )?;
        } else {
            let _ = self.resolve_bundler_preferred_non_relative(
                containing_directory,
                specifier,
                &request,
                diagnostic_pass,
                features.enable_imports || features.enable_self_name,
            )?;
        }
        Ok(())
    }

    fn resolve_bare_import_target_tail(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        features: BareResolutionFeatures,
    ) -> Result<(ResolutionOutcome<HostResolvedModule>, bool), ResolutionError> {
        let request = parse_package_request(specifier)?;
        if features.enable_self_name {
            if let Search::Terminal(outcome) = self.try_self_reference(
                containing_directory,
                &request,
                mode,
                probe_pass,
                Some(features.resolution_kind),
            )? {
                return Ok((outcome, false));
            }
        }
        if specifier.contains(':') {
            return Ok((ResolutionOutcome::NotFound, false));
        }
        if matches!(probe_pass, ExtensionProbePass::Empty) {
            return Ok((ResolutionOutcome::NotFound, false));
        }
        let all_passes = [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback];
        let one_pass = [probe_pass];
        let passes = if matches!(probe_pass, ExtensionProbePass::All) {
            all_passes.as_slice()
        } else {
            one_pass.as_slice()
        };
        let mut resolved_package_directory = false;
        for &node_modules_pass in passes {
            let (outcome, observed_package_directory) = self.resolve_from_node_modules_pass(
                containing_directory,
                &request,
                node_modules_pass,
                mode,
                features,
            )?;
            resolved_package_directory |= observed_package_directory;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok((outcome, resolved_package_directory));
            }
        }
        if probe_pass_has_declaration(probe_pass) {
            let outcome = self.resolve_module_from_type_roots(specifier, mode)?;
            Ok((outcome, resolved_package_directory))
        } else {
            Ok((ResolutionOutcome::NotFound, resolved_package_directory))
        }
    }

    fn resolve_from_node_modules_pass(
        &mut self,
        containing_directory: &str,
        request: &PackageRequest<'_>,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        features: BareResolutionFeatures,
    ) -> Result<(ResolutionOutcome<HostResolvedModule>, bool), ResolutionError> {
        let mut resolved_package_directory = false;
        for ancestor in ancestor_directories(containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }
            let package_root = package_root_for_request(&node_modules, request);
            let specific = self.resolve_specific_package(
                &package_root,
                &request.exports_subpath,
                probe_pass,
                mode,
                features.use_package_exports,
                Some(features.resolution_kind),
                /* follow_realpath */ false,
            )?;
            resolved_package_directory |= specific.root_package_observed;
            if matches!(specific.outcome, ResolutionOutcome::Resolved(_)) {
                return Ok((specific.outcome, resolved_package_directory));
            }

            if probe_pass_has_declaration(probe_pass) {
                let at_types = join_normalized(&node_modules, "@types");
                if self.host.directory_exists(Path::new(&at_types))? {
                    let package_root = types_package_root_for_request(&at_types, request);
                    let specific = self.resolve_specific_package(
                        &package_root,
                        &request.exports_subpath,
                        ExtensionProbePass::Declaration,
                        mode,
                        features.use_package_exports,
                        Some(features.resolution_kind),
                        /* follow_realpath */ false,
                    )?;
                    resolved_package_directory |= specific.root_package_observed;
                    if matches!(specific.outcome, ResolutionOutcome::Resolved(_)) {
                        return Ok((specific.outcome, resolved_package_directory));
                    }
                }
            }
        }
        Ok((ResolutionOutcome::NotFound, resolved_package_directory))
    }

    fn resolve_non_relative_inner(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let optional = self.resolve_using_optional_settings(
            containing_directory,
            specifier,
            ExtensionProbePass::All,
            mode,
            OptionalResolutionLoader::Node,
            /* follow_realpath */ false,
        )?;
        if matches!(optional, ResolutionOutcome::Resolved(_)) {
            return Ok(optional);
        }
        if specifier.starts_with('#') {
            if let Search::Terminal(outcome) = self.resolve_package_imports(
                containing_directory,
                specifier,
                mode,
                ExtensionProbePass::All,
                /* force_enabled */ self.module_imports_feature_is_hardcoded(),
                /* use_package_exports */ self.module_exports_feature_enabled(),
                /* resolution_kind */ None,
            )? {
                return Ok(outcome);
            }
        }
        let request = parse_package_request(specifier)?;

        if let Search::Terminal(outcome) = self.try_self_reference(
            containing_directory,
            &request,
            mode,
            ExtensionProbePass::All,
            /* resolution_kind */ None,
        )? {
            return Ok(outcome);
        }

        if specifier.contains(':') {
            return Ok(ResolutionOutcome::NotFound);
        }

        self.resolve_from_node_modules(containing_directory, specifier, &request, mode)
    }

    fn validate_supported_type_reference_configuration(
        &self,
        _mode: ResolutionMode,
    ) -> Result<(), ResolutionError> {
        let resolution_kind = self.options.emit_module_resolution_kind();
        if !matches!(resolution_kind, 1 | 2 | 3 | 99 | 100) {
            return Err(ResolutionError::unsupported(
                "module-resolution-kind",
                format!(
                    "type-reference resolution is implemented only for Classic, Node10, Node16, NodeNext, and Bundler; got {resolution_kind}"
                ),
            ));
        }
        self.validate_common_configuration()
    }

    fn module_exports_feature_enabled(&self) -> bool {
        match self.options.emit_module_resolution_kind() {
            // Node16 and NodeNext wrappers pass their fixed feature masks;
            // only Bundler computes feature overrides from compiler options.
            3 | 99 => true,
            100 => self.options.resolve_package_json_exports != Some(false),
            _ => self.options.resolve_package_json_exports == Some(true),
        }
    }

    fn module_imports_feature_is_hardcoded(&self) -> bool {
        matches!(self.options.emit_module_resolution_kind(), 3 | 99)
    }

    /// tsc-port: getNodeResolutionFeatures @6.0.3
    /// tsc-hash: 0f196c9d68f11eb9044f8a8b91dc3932ce0282c7825241c05ed817faab092b98
    /// tsc-span: _tsc.js:40251-40274
    ///
    /// `resolveTypeReferenceDirective` adds `AllFeatures` after applying the
    /// ordinary option overrides whenever the directive carries an explicit
    /// resolution mode. That deliberately re-enables package exports even
    /// when `resolvePackageJsonExports` is false. With an unspecified mode,
    /// Classic and Node10 remain legacy unless exports were explicitly
    /// enabled, while the modern resolvers use their computed defaults.
    fn type_reference_uses_package_exports(&self, mode: ResolutionMode) -> bool {
        if mode != ResolutionMode::Unspecified {
            return true;
        }
        match self.options.emit_module_resolution_kind() {
            3 | 99 | 100 => self.options.resolve_package_json_exports != Some(false),
            1 | 2 => self.options.resolve_package_json_exports == Some(true),
            _ => false,
        }
    }

    fn validate_supported_module_configuration(
        &self,
        _mode: ResolutionMode,
    ) -> Result<(), ResolutionError> {
        let resolution_kind = self.options.emit_module_resolution_kind();
        if !matches!(resolution_kind, 1 | 2 | 3 | 99 | 100) {
            return Err(ResolutionError::unsupported(
                "module-resolution-kind",
                format!(
                    "module resolution is implemented only for Classic, Node10, Node16, NodeNext, and Bundler; got {resolution_kind}"
                ),
            ));
        }
        self.validate_common_configuration()
    }

    fn validate_common_configuration(&self) -> Result<(), ResolutionError> {
        if self.options.no_dts_resolution == Some(true) {
            return Err(ResolutionError::unsupported(
                "no-dts-resolution",
                "implementation-only exports probing is outside the H0.2b slice",
            ));
        }
        Ok(())
    }

    fn current_directory_text(&self) -> Result<&str, ResolutionError> {
        self.path_context
            .current_directory()
            .display()
            .to_str()
            .ok_or_else(|| {
                ResolutionError::canonicalization(
                    Some(
                        self.path_context
                            .current_directory()
                            .display()
                            .to_path_buf(),
                    ),
                    "current directory is not valid Unicode",
                )
            })
    }

    fn try_self_reference(
        &mut self,
        containing_directory: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
        probe_pass: ExtensionProbePass,
        resolution_kind: Option<i32>,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let Some(package) = self.find_nearest_package_scope(containing_directory)? else {
            return Ok(Search::Continue);
        };
        let package_key = canonical_text(
            &package.root,
            self.path_context.use_case_sensitive_file_names(),
        );
        // loadModuleFromImports re-enters the Node resolver from
        // `scope.packageDirectory + "/"`. At a filesystem root that spelling
        // gains a trailing separator (`//` at the POSIX root), so the upstream
        // package-scope walk does not rediscover the same package; preserve
        // that boundary without blocking valid non-root self references.
        if directory_name(&package.root) == package.root
            && self.active_package_maps.contains(&package_key)
        {
            return Ok(Search::Continue);
        }
        if package.metadata.name() != Some(request.package_name)
            || !package
                .exports
                .as_ref()
                .is_some_and(js_json_value_is_truthy)
        {
            return Ok(Search::Continue);
        }

        // This one fast-path uses pathContainsNodeModules' literal
        // `/node_modules/` substring test upstream. A containing directory
        // ending exactly in `node_modules` therefore remains local here.
        let preserve_combined_pass =
            self.options.allow_js && !containing_directory.contains("/node_modules/");
        let split_passes = match probe_pass {
            ExtensionProbePass::All => {
                [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback]
            }
            ExtensionProbePass::Preferred => {
                [ExtensionProbePass::Preferred, ExtensionProbePass::Empty]
            }
            ExtensionProbePass::Declaration => {
                [ExtensionProbePass::Declaration, ExtensionProbePass::Empty]
            }
            ExtensionProbePass::Fallback => {
                [ExtensionProbePass::Empty, ExtensionProbePass::Fallback]
            }
            ExtensionProbePass::JsonConfig => {
                [ExtensionProbePass::JsonConfig, ExtensionProbePass::Empty]
            }
            ExtensionProbePass::JsonModule => {
                [ExtensionProbePass::JsonModule, ExtensionProbePass::Empty]
            }
            ExtensionProbePass::Empty => [ExtensionProbePass::Empty, ExtensionProbePass::Empty],
        };
        let one_pass = [probe_pass];
        let passes = if preserve_combined_pass {
            one_pass.as_slice()
        } else {
            split_passes.as_slice()
        };
        for &self_name_pass in passes {
            let result = self.search_package_exports(
                &package,
                &request.exports_subpath,
                /* is_external_library_import */ false,
                self_name_pass,
                mode,
                resolution_kind.unwrap_or_else(|| self.options.emit_module_resolution_kind()),
                /* follow_realpath */ false,
            )?;
            if !matches!(result, Search::Continue) {
                return Ok(result);
            }
        }
        Ok(Search::Continue)
    }

    /// tsc-port: loadModuleFromImports @6.0.3
    /// tsc-hash: 4f4510daf578be52814574369949af61fa39b610fef58eadc272282bfd77f6d5
    /// tsc-span: _tsc.js:41534-41586
    #[allow(clippy::too_many_arguments)] // Keeps the upstream package-map feature mask explicit.
    fn resolve_package_imports(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
        probe_pass: ExtensionProbePass,
        force_enabled: bool,
        use_package_exports: bool,
        resolution_kind: Option<i32>,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        if !force_enabled && self.options.resolve_package_json_imports == Some(false) {
            return Ok(Search::Continue);
        }
        let resolution_kind =
            resolution_kind.unwrap_or_else(|| self.options.emit_module_resolution_kind());
        if specifier == "#" || (specifier.starts_with("#/") && resolution_kind == 3) {
            return Ok(Search::Continue);
        }
        let Some(package) = self.find_nearest_package_scope(containing_directory)? else {
            return Ok(Search::Continue);
        };
        let Some(imports) = package.imports.as_ref() else {
            return Ok(Search::Continue);
        };
        let Some(table) = imports.as_object() else {
            return Ok(Search::Continue);
        };

        let package_key = canonical_text(
            &package.root,
            self.path_context.use_case_sensitive_file_names(),
        );
        self.active_package_maps.push(package_key);
        let search = self.search_exports_table(
            &package,
            table,
            specifier,
            ExportProbeContext {
                is_external_library_import: false,
                follow_realpath: false,
                pass: probe_pass,
                mode,
                resolution_kind,
                exports_pattern_trailers: exports_pattern_trailers_enabled(mode, resolution_kind),
                kind: PackageMapKind::Imports,
                bare_features: Some(BareResolutionFeatures {
                    use_package_exports,
                    enable_imports: true,
                    enable_self_name: true,
                    resolution_kind,
                }),
            },
        );
        self.active_package_maps.pop();
        search
    }

    fn resolve_from_node_modules(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let mut resolved_package_directory = false;
        for probe_pass in [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback] {
            for ancestor in ancestor_directories(containing_directory) {
                if base_name(&ancestor) == "node_modules" {
                    continue;
                }
                let node_modules = join_normalized(&ancestor, "node_modules");
                if !self.host.directory_exists(Path::new(&node_modules))? {
                    continue;
                }
                let package_root = package_root_for_request(&node_modules, request);
                let specific = self.resolve_specific_package(
                    &package_root,
                    &request.exports_subpath,
                    probe_pass,
                    mode,
                    self.module_exports_feature_enabled(),
                    None,
                    /* follow_realpath */ false,
                )?;
                resolved_package_directory |= specific.root_package_observed;
                if matches!(specific.outcome, ResolutionOutcome::Resolved(_)) {
                    return self.attach_modern_alternate(
                        containing_directory,
                        specifier,
                        request,
                        mode,
                        resolved_package_directory,
                        specific.outcome,
                    );
                }

                if matches!(probe_pass, ExtensionProbePass::Preferred) {
                    let at_types = join_normalized(&node_modules, "@types");
                    if self.host.directory_exists(Path::new(&at_types))? {
                        let types_package = types_package_root_for_request(&at_types, request);
                        let specific = self.resolve_specific_package(
                            &types_package,
                            &request.exports_subpath,
                            ExtensionProbePass::Declaration,
                            mode,
                            self.module_exports_feature_enabled(),
                            None,
                            /* follow_realpath */ false,
                        )?;
                        resolved_package_directory |= specific.root_package_observed;
                        if matches!(specific.outcome, ResolutionOutcome::Resolved(_)) {
                            return self.attach_modern_alternate(
                                containing_directory,
                                specifier,
                                request,
                                mode,
                                resolved_package_directory,
                                specific.outcome,
                            );
                        }
                    }
                }
            }
        }
        let outcome = self.resolve_module_from_type_roots(specifier, mode)?;
        self.attach_modern_alternate(
            containing_directory,
            specifier,
            request,
            mode,
            resolved_package_directory,
            outcome,
        )
    }

    fn attach_modern_alternate(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
        resolved_package_directory: bool,
        mut outcome: ResolutionOutcome<HostResolvedModule>,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let should_retry = resolved_package_directory
            && self.module_exports_feature_enabled()
            && self.package_condition_matches(
                "import",
                mode,
                self.options.emit_module_resolution_kind(),
            )
            && matches!(
                &outcome,
                ResolutionOutcome::Resolved(module)
                    if module.is_external_library_import()
                        && !is_typescript_module_extension(module.extension())
            );
        if !should_retry {
            return Ok(outcome);
        }
        let alternate = self.resolve_modern_preferred_without_exports(
            containing_directory,
            specifier,
            request,
            mode,
            ExtensionProbePass::Preferred,
            /* force_package_maps */ self.module_imports_feature_is_hardcoded(),
            self.options.emit_module_resolution_kind(),
        )?;
        if let (ResolutionOutcome::Resolved(primary), ResolutionOutcome::Resolved(alternate)) =
            (&mut outcome, alternate)
        {
            if alternate.is_external_library_import() {
                primary.alternate_result = Some(alternate.resolved_file().clone());
            }
        }
        Ok(outcome)
    }

    /// Re-run the complete preferred-extension non-relative search with only
    /// the package-exports feature disabled. This is diagnostic-only: its
    /// lexical result becomes `alternateResult`, while the primary result is
    /// realpathed only after this attempt has completed.
    #[allow(clippy::too_many_arguments)] // Diagnostic re-entry owns an independent resolver profile.
    fn resolve_modern_preferred_without_exports(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
        probe_pass: ExtensionProbePass,
        force_package_maps: bool,
        resolution_kind: i32,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let optional = self.resolve_using_optional_settings(
            containing_directory,
            specifier,
            probe_pass,
            mode,
            OptionalResolutionLoader::Node,
            /* follow_realpath */ false,
        )?;
        if matches!(optional, ResolutionOutcome::Resolved(_)) {
            return Ok(optional);
        }

        if specifier.starts_with('#') {
            if let Search::Terminal(outcome) = self.resolve_package_imports(
                containing_directory,
                specifier,
                mode,
                probe_pass,
                force_package_maps,
                /* use_package_exports */ false,
                /* resolution_kind */ Some(resolution_kind),
            )? {
                return Ok(outcome);
            }
        }

        // Clearing Exports does not clear SelfName. A successful self-name
        // result is local and therefore will not be published as an alternate,
        // but it still owns this diagnostic attempt just as it does upstream.
        if let Search::Terminal(outcome) = self.try_self_reference(
            containing_directory,
            request,
            mode,
            probe_pass,
            /* resolution_kind */ Some(resolution_kind),
        )? {
            return Ok(outcome);
        }

        if specifier.contains(':') {
            return Ok(ResolutionOutcome::NotFound);
        }
        if matches!(probe_pass, ExtensionProbePass::Empty) {
            return Ok(ResolutionOutcome::NotFound);
        }

        for ancestor in ancestor_directories(containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }
            let package_root = package_root_for_request(&node_modules, request);
            let outcome = self
                .resolve_specific_package(
                    &package_root,
                    &request.exports_subpath,
                    probe_pass,
                    mode,
                    /* use_package_exports */ false,
                    Some(resolution_kind),
                    /* follow_realpath */ false,
                )?
                .outcome;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }

            if probe_pass_has_declaration(probe_pass) {
                let at_types = join_normalized(&node_modules, "@types");
                if !self.host.directory_exists(Path::new(&at_types))? {
                    continue;
                }
                let package_root = types_package_root_for_request(&at_types, request);
                let outcome = self
                    .resolve_specific_package(
                        &package_root,
                        &request.exports_subpath,
                        ExtensionProbePass::Declaration,
                        mode,
                        /* use_package_exports */ false,
                        Some(resolution_kind),
                        /* follow_realpath */ false,
                    )?
                    .outcome;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            }
        }
        if probe_pass_has_declaration(probe_pass) {
            self.resolve_module_from_type_roots(specifier, mode)
        } else {
            Ok(ResolutionOutcome::NotFound)
        }
    }

    /// Resolve one package candidate with declaration extensions only. This is
    /// the shared worker for the module resolver's `@types` fallback and the
    /// type-reference resolver's secondary node_modules search.
    ///
    /// tsc-port: loadModuleFromSpecificNodeModulesDirectory @6.0.3
    /// tsc-hash: cea26d829ab986a3959897a336dc743f1787f9b7880bb1c5d6f6849c6ea69153
    /// tsc-span: _tsc.js:41979-42035
    fn resolve_declaration_package(
        &mut self,
        package_root: &str,
        exports_subpath: &str,
        mode: ResolutionMode,
        use_package_exports: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.resolve_specific_package(
            package_root,
            exports_subpath,
            ExtensionProbePass::Declaration,
            mode,
            use_package_exports,
            None,
            /* follow_realpath */ true,
        )
        .map(|result| result.outcome)
    }

    /// tsc-port: loadModuleFromSpecificNodeModulesDirectory @6.0.3
    /// tsc-hash: cea26d829ab986a3959897a336dc743f1787f9b7880bb1c5d6f6849c6ea69153
    /// tsc-span: _tsc.js:41979-42035
    ///
    /// The candidate package.json (a nested subpath boundary) is observed
    /// before the root manifest. Every ordinary, @types, diagnostic Bundler,
    /// and type-reference node_modules path must pass through this worker so
    /// those observations and the root direct-file phase stay identical.
    #[allow(clippy::too_many_arguments)] // The specific-package worker preserves each tsc latch.
    fn resolve_specific_package(
        &mut self,
        package_root: &str,
        exports_subpath: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        use_package_exports: bool,
        exports_resolution_kind: Option<i32>,
        follow_realpath: bool,
    ) -> Result<SpecificPackageResolution, ResolutionError> {
        let rest = package_subpath(exports_subpath)?;
        let mut root_package = None;
        let mut root_package_loaded = false;
        let candidate = rest
            .map(|rest| {
                normalize_absolute_path(Path::new(&join_normalized(package_root, rest)), None)
            })
            .transpose()?
            .unwrap_or_else(|| package_root.to_owned());
        let candidate_package = self.load_package(&join_normalized(&candidate, "package.json"))?;

        // A package subpath is itself allowed to be a package boundary. The
        // candidate manifest is observed before the root manifest, even when
        // the root exports map will ultimately own the request.
        if rest.is_some() {
            if let Some(nested_package) = candidate_package.as_ref() {
                let root_exports_govern = if use_package_exports {
                    root_package =
                        self.load_package(&join_normalized(package_root, "package.json"))?;
                    root_package_loaded = true;
                    root_package
                        .as_ref()
                        .is_some_and(|package| package.has_own_exports)
                } else {
                    false
                };
                if !root_exports_govern {
                    let outcome = self.resolve_nested_legacy_package(
                        &candidate,
                        nested_package,
                        probe_pass,
                        mode,
                        follow_realpath,
                    )?;
                    return Ok(SpecificPackageResolution {
                        outcome,
                        root_package_observed: false,
                    });
                }
            }
        }

        let package = if root_package_loaded {
            root_package
        } else if rest.is_none() {
            candidate_package
        } else {
            self.load_package(&join_normalized(package_root, "package.json"))?
        };
        if let Some(package) = package {
            let uses_exports = use_package_exports
                && package
                    .exports
                    .as_ref()
                    .is_some_and(js_json_value_is_truthy);
            if !uses_exports && exports_subpath == "." && !self.is_node_esm_mode(mode) {
                let direct = self.probe_legacy_file(
                    Some(&package),
                    package_root,
                    probe_pass,
                    /* allow_implicit */ true,
                    LegacyResolutionContext {
                        is_external_library_import: true,
                        attach_package_id: true,
                        resolved_using_ts_extension: false,
                        follow_realpath,
                    },
                )?;
                if matches!(direct, ResolutionOutcome::Resolved(_)) {
                    return Ok(SpecificPackageResolution {
                        outcome: direct,
                        root_package_observed: true,
                    });
                }
            }
            let outcome = if uses_exports {
                if let Some(resolution_kind) = exports_resolution_kind {
                    self.resolve_package_exports_with_resolution_kind(
                        &package,
                        exports_subpath,
                        /* is_external_library_import */ true,
                        probe_pass,
                        mode,
                        resolution_kind,
                        follow_realpath,
                    )?
                } else {
                    self.resolve_package_exports(
                        &package,
                        exports_subpath,
                        /* is_external_library_import */ true,
                        probe_pass,
                        mode,
                        follow_realpath,
                    )?
                }
            } else {
                self.resolve_legacy_package(
                    &package,
                    exports_subpath,
                    probe_pass,
                    mode,
                    LegacyResolutionContext {
                        is_external_library_import: true,
                        attach_package_id: true,
                        resolved_using_ts_extension: false,
                        follow_realpath,
                    },
                    Some(&candidate),
                    /* allow_node_esm_index_fallback */ true,
                )?
            };
            Ok(SpecificPackageResolution {
                outcome,
                root_package_observed: true,
            })
        } else {
            let outcome = self.resolve_manifestless_package(
                &candidate,
                rest.is_some(),
                probe_pass,
                mode,
                follow_realpath,
            )?;
            Ok(SpecificPackageResolution {
                outcome,
                root_package_observed: false,
            })
        }
    }

    fn resolve_nested_legacy_package(
        &self,
        candidate: &str,
        package: &CachedPackage,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let direct = self.probe_legacy_file(
            None,
            candidate,
            probe_pass,
            /* allow_implicit */ !self.is_node_esm_mode(mode),
            LegacyResolutionContext {
                is_external_library_import: true,
                attach_package_id: false,
                resolved_using_ts_extension: is_typescript_family_specifier(candidate),
                follow_realpath,
            },
        )?;
        if matches!(direct, ResolutionOutcome::Resolved(_)) {
            return Ok(direct);
        }
        self.resolve_legacy_package(
            package,
            ".",
            probe_pass,
            mode,
            LegacyResolutionContext {
                is_external_library_import: true,
                attach_package_id: true,
                resolved_using_ts_extension: false,
                follow_realpath,
            },
            Some(candidate),
            /* allow_node_esm_index_fallback */ false,
        )
    }

    fn normalized_type_root(&self, root: &ProgramPath) -> Result<String, ResolutionError> {
        let normalized =
            normalize_absolute_path(root.display(), Some(self.current_directory_text()?))?;
        let expected = canonical_text(
            &normalized,
            self.path_context.use_case_sensitive_file_names(),
        );
        if root.canonical().as_path().to_str() != Some(expected.as_str()) {
            return Err(ResolutionError::canonicalization(
                Some(root.display().to_path_buf()),
                "typeRoots display and canonical paths do not match the resolver path profile",
            ));
        }
        Ok(normalized)
    }

    pub(crate) fn effective_type_roots(
        &self,
        type_roots: Option<&[ProgramPath]>,
    ) -> Result<Vec<String>, ResolutionError> {
        match type_roots {
            Some(roots) => roots
                .iter()
                .map(|root| self.normalized_type_root(root))
                .collect(),
            None => Ok(ancestor_directories(&self.type_root_base_directory)
                .into_iter()
                .map(|ancestor| {
                    join_normalized(&join_normalized(&ancestor, "node_modules"), "@types")
                })
                .collect()),
        }
    }

    pub(crate) fn type_root_base_directory(&self) -> &str {
        &self.type_root_base_directory
    }

    fn resolve_module_from_type_roots(
        &mut self,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let Some(configured_type_roots) = self.type_roots.clone() else {
            return Ok(ResolutionOutcome::NotFound);
        };
        let effective_type_roots = self.effective_type_roots(Some(&configured_type_roots))?;
        for type_root in effective_type_roots {
            let outcome = self.resolve_type_reference_from_root(
                &type_root, specifier, mode, /* custom_type_roots */ true,
                /* follow_realpath */ false,
            )?;
            if let ResolutionOutcome::Resolved(mut module) = outcome {
                // resolveFromTypeRoot is entered from the non-relative node
                // resolver and wraps even a custom local root as an external
                // library import. Primary realpath is deferred to the caller.
                module.is_external_library_import = true;
                return Ok(ResolutionOutcome::Resolved(module));
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn resolve_type_reference_from_root(
        &mut self,
        type_root: &str,
        specifier: &str,
        mode: ResolutionMode,
        custom_type_roots: bool,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        if !self.host.directory_exists(Path::new(type_root))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        let name_for_lookup = if type_root.ends_with("/node_modules/@types") {
            mangle_scoped_package_name(specifier)
        } else {
            specifier.to_owned()
        };
        // getCandidateFromTypeRoot uses combinePaths rather than
        // normalizePath. Preserve dot components, duplicate separators, and
        // rooted child replacement for the host probes in this primary pass.
        let candidate = combine_paths_spelling(type_root, &name_for_lookup)?;
        let external = path_contains_node_modules(&candidate);
        let context = LegacyResolutionContext {
            is_external_library_import: external,
            attach_package_id: external,
            resolved_using_ts_extension: false,
            follow_realpath,
        };

        // An explicitly configured type root first receives the declaration
        // file probe that default node_modules/@types roots deliberately omit.
        if custom_type_roots {
            let direct =
                self.probe_direct_type_reference_file(&candidate, mode, follow_realpath)?;
            if matches!(direct, ResolutionOutcome::Resolved(_)) {
                return Ok(direct);
            }
        }

        if !self.host.directory_exists(Path::new(&candidate))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        let package_json = join_normalized(&candidate, "package.json");
        if let Some(package) = self.load_package(&package_json)? {
            return self.resolve_legacy_package(
                &package,
                ".",
                ExtensionProbePass::Declaration,
                mode,
                LegacyResolutionContext {
                    attach_package_id: true,
                    ..context
                },
                Some(&candidate),
                /* allow_node_esm_index_fallback */ false,
            );
        }
        if self.is_node_esm_mode(mode) {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.probe_legacy_file(
            None,
            &join_normalized(&candidate, "index"),
            ExtensionProbePass::Declaration,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                attach_package_id: false,
                ..context
            },
        )
    }

    fn resolve_type_reference_from_node_modules(
        &mut self,
        containing_directory: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
        use_package_exports: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        for ancestor in ancestor_directories(containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }

            let package_root = package_root_for_request(&node_modules, request);
            let outcome = self.resolve_declaration_package(
                &package_root,
                &request.exports_subpath,
                mode,
                use_package_exports,
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }

            let at_types = join_normalized(&node_modules, "@types");
            if !self.host.directory_exists(Path::new(&at_types))? {
                continue;
            }
            let types_package = types_package_root_for_request(&at_types, request);
            let outcome = self.resolve_declaration_package(
                &types_package,
                &request.exports_subpath,
                mode,
                use_package_exports,
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn resolve_relative_type_reference(
        &mut self,
        containing_file: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let directory_spelling = has_node_directory_spelling(specifier);
        let target = preserve_node_directory_spelling(
            normalize_absolute_path(Path::new(specifier), Some(&directory_name(containing_file)))?,
            directory_spelling,
        );
        let external = path_contains_node_modules(&target);
        let context = LegacyResolutionContext {
            is_external_library_import: external,
            attach_package_id: external,
            resolved_using_ts_extension: false,
            follow_realpath: true,
        };
        let allow_implicit = !self.is_node_esm_mode(mode);
        if !directory_spelling {
            // nodeLoadModuleByRelativeName latches its outer parent
            // observation before the file loader performs stage-specific
            // observations of the same directory.
            if !self
                .host
                .directory_exists(Path::new(&directory_name(&target)))?
            {
                return Ok(ResolutionOutcome::NotFound);
            }
            let direct = self
                .probe_direct_type_reference_file(&target, mode, /* follow_realpath */ true)?;
            if matches!(direct, ResolutionOutcome::Resolved(_)) {
                return Ok(direct);
            }
        }
        // ESM mode disables the directory loader only after observing the
        // candidate directory.
        let target_exists = self.host.directory_exists(Path::new(&target))?;
        if !allow_implicit || !target_exists {
            return Ok(ResolutionOutcome::NotFound);
        }
        if let Some(directory_package) =
            self.load_package(&join_normalized(&target, "package.json"))?
        {
            return self.resolve_legacy_package(
                &directory_package,
                ".",
                ExtensionProbePass::Declaration,
                mode,
                LegacyResolutionContext {
                    attach_package_id: true,
                    ..context
                },
                Some(&target),
                /* allow_node_esm_index_fallback */ true,
            );
        }
        self.probe_legacy_file(
            None,
            &join_normalized(&target, "index"),
            ExtensionProbePass::Declaration,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                attach_package_id: false,
                ..context
            },
        )
    }

    /// Resolve a relative request through the legacy file/directory loader.
    /// A directory package is reconsidered from its own package root so a
    /// versioned declaration can intentionally refer back to itself.
    fn resolve_relative(
        &mut self,
        containing_file: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        if specifier.contains('\0') {
            return Err(ResolutionError::invalid_data(format!(
                "invalid relative module specifier {specifier:?}"
            )));
        }
        let containing_directory = directory_name(containing_file);
        let resolution_kind = self.options.emit_module_resolution_kind();
        // nodeModuleNameResolverWorker splits Node10 into priority and
        // secondary extension passes, but invokes its modern resolvers once
        // with every admitted extension. Keeping optional and ordinary
        // candidates inside the same outer pass preserves both rootDirs
        // ordering and the duplicate original probe after a rootDirs miss.
        let legacy_passes = [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback];
        let modern_passes = [ExtensionProbePass::All];
        let probe_passes = if resolution_kind == 2 {
            legacy_passes.as_slice()
        } else {
            modern_passes.as_slice()
        };
        self.resolve_relative_with_passes(
            &containing_directory,
            specifier,
            mode,
            probe_passes,
            /* optional_follow_realpath */ true,
        )
    }

    fn resolve_relative_with_passes(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
        probe_passes: &[ExtensionProbePass],
        optional_follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let directory_spelling = has_node_directory_spelling(specifier);
        // nodeModuleNameResolverWorker classifies the unnormalized path
        // components produced by combinePaths. In particular,
        // `./node_modules/../x` remains an external-library request even
        // though normalization later produces a lexical path outside
        // node_modules.
        let raw_target = combine_paths_spelling(containing_directory, specifier)?;
        let target = preserve_node_directory_spelling(
            normalize_absolute_path(Path::new(specifier), Some(containing_directory))?,
            directory_spelling,
        );
        let external = path_contains_node_modules(&raw_target);
        let allow_implicit = !self.is_node_esm_mode(mode);

        for &probe_pass in probe_passes {
            let optional = self.resolve_using_optional_settings(
                containing_directory,
                specifier,
                probe_pass,
                mode,
                OptionalResolutionLoader::Node,
                optional_follow_realpath,
            )?;
            if matches!(optional, ResolutionOutcome::Resolved(_)) {
                return Ok(optional);
            }
            if !directory_spelling {
                // nodeLoadModuleByRelativeName converts a missing candidate
                // parent into `onlyRecordFailures` before any package or
                // candidate-directory work. The file loader checks the same
                // parent again when this preflight succeeds.
                if !self
                    .host
                    .directory_exists(Path::new(&directory_name(&target)))?
                {
                    continue;
                }
                let outcome = self.probe_legacy_file(
                    None,
                    &target,
                    probe_pass,
                    allow_implicit,
                    LegacyResolutionContext {
                        is_external_library_import: external,
                        attach_package_id: false,
                        resolved_using_ts_extension: is_typescript_family_specifier(&target),
                        follow_realpath: false,
                    },
                )?;
                if let ResolutionOutcome::Resolved(mut module) = outcome {
                    self.attach_direct_node_package(&mut module)?;
                    return Ok(ResolutionOutcome::Resolved(module));
                }
            }

            let target_exists = self.host.directory_exists(Path::new(&target))?;
            if !allow_implicit || !target_exists {
                continue;
            }
            let package_json = join_normalized(&target, "package.json");
            if let Some(directory_package) = self.load_package(&package_json)? {
                let outcome = self.resolve_legacy_package(
                    &directory_package,
                    ".",
                    probe_pass,
                    mode,
                    LegacyResolutionContext {
                        is_external_library_import: external,
                        attach_package_id: true,
                        resolved_using_ts_extension: false,
                        follow_realpath: false,
                    },
                    Some(&target),
                    /* allow_node_esm_index_fallback */ true,
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            } else {
                let index_name = if matches!(probe_pass, ExtensionProbePass::JsonConfig) {
                    "tsconfig"
                } else {
                    "index"
                };
                let index = join_normalized(&target, index_name);
                let outcome = self.probe_legacy_file(
                    None,
                    &index,
                    probe_pass,
                    /* allow_implicit */ true,
                    LegacyResolutionContext {
                        is_external_library_import: external,
                        attach_package_id: false,
                        resolved_using_ts_extension: false,
                        follow_realpath: false,
                    },
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn resolve_manifestless_package(
        &self,
        candidate: &str,
        has_subpath: bool,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let allow_implicit = !self.is_node_esm_mode(mode);
        if has_subpath || allow_implicit {
            let outcome = self.probe_legacy_file(
                None,
                candidate,
                probe_pass,
                allow_implicit,
                LegacyResolutionContext {
                    is_external_library_import: true,
                    attach_package_id: false,
                    resolved_using_ts_extension: is_typescript_family_specifier(candidate),
                    follow_realpath,
                },
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        let candidate_exists = self.host.directory_exists(Path::new(candidate))?;
        if !allow_implicit || !candidate_exists {
            return Ok(ResolutionOutcome::NotFound);
        }
        let index_name = if matches!(probe_pass, ExtensionProbePass::JsonConfig) {
            "tsconfig"
        } else {
            "index"
        };
        self.probe_legacy_file(
            None,
            &join_normalized(candidate, index_name),
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                is_external_library_import: true,
                attach_package_id: false,
                resolved_using_ts_extension: false,
                follow_realpath,
            },
        )
    }

    /// Resolve the root package directory or one root-owned subpath after the
    /// specific-node_modules worker has selected the root package metadata.
    fn rewrite_package_id_for_directory_spelling(
        &self,
        package: &CachedPackage,
        package_directory: &str,
        context: LegacyResolutionContext,
        outcome: &mut ResolutionOutcome<HostResolvedModule>,
    ) -> Result<(), ResolutionError> {
        if !context.attach_package_id || package_directory == package.root {
            return Ok(());
        }
        let ResolutionOutcome::Resolved(module) = outcome else {
            return Ok(());
        };
        let lexical_path = module.resolved_file.display().to_str().ok_or_else(|| {
            ResolutionError::canonicalization(
                Some(module.resolved_file.display().to_path_buf()),
                "resolved module path is not valid Unicode",
            )
        })?;
        module.package_id = package_id_for_legacy_path_from_directory(
            package,
            package_directory,
            lexical_path,
            true,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Package-directory state is intentionally non-implicit.
    fn resolve_legacy_package(
        &self,
        package: &CachedPackage,
        exports_subpath: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        context: LegacyResolutionContext,
        root_directory_spelling: Option<&str>,
        allow_node_esm_index_fallback: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let rest = package_subpath(exports_subpath)?;
        if let Some(rest) = rest {
            // The outer package loader applies a root-relative mapping to the
            // package subpath before invoking its file/directory loader. A
            // matched all-target miss owns this package candidate.
            match self.search_package_types_versions(
                package,
                rest,
                probe_pass,
                mode,
                TypesVersionsResolutionContext {
                    legacy: context,
                    base_directory: &package.root,
                    loader: TypesVersionsLoader::PackageSubpath,
                    attach_exact_package_id: false,
                    only_record_failures: false,
                },
            )? {
                Search::Terminal(outcome) => return Ok(outcome),
                Search::Continue => {}
            }
            let candidate = match root_directory_spelling {
                Some(candidate) => candidate.to_owned(),
                None => normalize_package_subpath(package, rest)?,
            };
            return self.probe_package_subpath_path(package, &candidate, probe_pass, mode, context);
        }

        let root_directory = root_directory_spelling.unwrap_or(&package.root);
        let mut outcome =
            self.probe_legacy_directory_worker(package, root_directory, probe_pass, mode, context)?;
        self.rewrite_package_id_for_directory_spelling(
            package,
            root_directory,
            context,
            &mut outcome,
        )?;
        if matches!(outcome, ResolutionOutcome::Resolved(_)) {
            return Ok(outcome);
        }

        // Node ESM's assumed root index.js lives outside the directory worker
        // and therefore survives an owned typesVersions miss.
        if !self.is_node_esm_mode(mode)
            || !allow_node_esm_index_fallback
            || !matches!(package.exports.as_ref(), None | Some(Value::Null))
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        let index = join_normalized(root_directory, "index.js");
        let mut outcome = self.probe_legacy_file(
            Some(package),
            &index,
            probe_pass,
            /* allow_implicit */ false,
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )?;
        self.rewrite_package_id_for_directory_spelling(
            package,
            root_directory,
            context,
            &mut outcome,
        )?;
        Ok(outcome)
    }

    /// The non-recursive package-directory worker. A subpath directory keeps
    /// the root package's typesVersions table, but does not re-read a nested
    /// package.json or re-enter the outer subpath loader.
    fn probe_legacy_directory_worker(
        &self,
        package: &CachedPackage,
        candidate_directory: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let candidate_key = canonical_text(
            candidate_directory,
            self.path_context.use_case_sensitive_file_names(),
        );
        let package_root_key = canonical_text(
            &package.root,
            self.path_context.use_case_sensitive_file_names(),
        );
        // `combinePaths` preserves a caller's directory spelling, so `.`,
        // `..`, and an explicit trailing slash can reach the same package
        // root as either `/pkg` or `/pkg/`. TypeScript's contains-path check
        // treats those spellings as the same directory before consulting the
        // package entry field and its typesVersions logical name.
        let is_package_root =
            path_relative_to_directory(&candidate_key, &package_root_key) == Some("");
        let package_field = is_package_root
            .then(|| selected_package_entry_field(package, probe_pass))
            .flatten();
        let package_field_candidate = package_field
            .map(|field| normalize_legacy_package_target(package, field))
            .transpose()?;
        let package_field_parent_exists = package_field_candidate
            .as_ref()
            .map(|candidate| {
                self.host
                    .directory_exists(Path::new(&directory_name(candidate)))
            })
            .transpose()?;
        let only_record_failures_for_index =
            !self.host.directory_exists(Path::new(candidate_directory))?;
        let only_record_failures_for_types_versions =
            package_field_parent_exists == Some(false) || only_record_failures_for_index;
        let types_versions_eligible = package_field_candidate
            .as_deref()
            .is_none_or(|candidate| path_is_within(candidate, candidate_directory));
        let default_entry = if matches!(probe_pass, ExtensionProbePass::JsonConfig) {
            "tsconfig"
        } else {
            "index"
        };
        let logical_name = if let Some(candidate) = package_field_candidate.as_deref() {
            if types_versions_eligible {
                path_relative_to_directory(candidate, candidate_directory)
                    .ok_or_else(|| {
                        ResolutionError::invalid_data(format!(
                            "package target {candidate} is outside directory {candidate_directory}"
                        ))
                    })?
                    .trim_end_matches('/')
                    .to_owned()
            } else {
                default_entry.to_owned()
            }
        } else {
            default_entry.to_owned()
        };

        if types_versions_eligible {
            match self.search_package_types_versions(
                package,
                &logical_name,
                probe_pass,
                mode,
                TypesVersionsResolutionContext {
                    legacy: context,
                    base_directory: candidate_directory,
                    loader: TypesVersionsLoader::PackageDirectory,
                    attach_exact_package_id: context.attach_package_id,
                    only_record_failures: only_record_failures_for_types_versions,
                },
            )? {
                Search::Terminal(outcome) => return Ok(outcome),
                Search::Continue => {}
            }
        }

        if let Some(candidate) = package_field_candidate {
            if package_field_parent_exists == Some(true) {
                let outcome = self.probe_package_field_path(
                    Some(package),
                    &candidate,
                    probe_pass,
                    !self.is_node_esm_mode(mode)
                        || package.metadata.module_type() != PackageJsonType::Module,
                    LegacyResolutionContext {
                        resolved_using_ts_extension: false,
                        ..context
                    },
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            }
        }

        if self.is_node_esm_mode(mode) || only_record_failures_for_index {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.probe_legacy_file(
            Some(package),
            &join_normalized(candidate_directory, default_entry),
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )
    }

    fn probe_package_subpath_path(
        &self,
        package: &CachedPackage,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let outcome = self.probe_legacy_file(
            Some(package),
            candidate,
            probe_pass,
            /* allow_implicit */ !self.is_node_esm_mode(mode),
            LegacyResolutionContext {
                resolved_using_ts_extension: is_typescript_family_specifier(candidate),
                ..context
            },
        )?;
        if matches!(outcome, ResolutionOutcome::Resolved(_)) {
            return Ok(outcome);
        }
        self.probe_legacy_directory_worker(package, candidate, probe_pass, mode, context)
    }

    /// A matching `typesVersions` key owns the result even when every target
    /// misses. No matching range or mapping key continues to ordinary legacy
    /// package loading.
    fn search_package_types_versions(
        &self,
        package: &CachedPackage,
        logical_name: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        context: TypesVersionsResolutionContext<'_>,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let TypesVersionsResolutionContext {
            legacy: context,
            base_directory,
            loader,
            attach_exact_package_id,
            only_record_failures,
        } = context;
        let Some(types_versions) = package.types_versions.as_ref() else {
            return Ok(Search::Continue);
        };
        let matching = js_json_object_entries(types_versions)
            .expect("CachedPackage retains only object-like typesVersions fields")
            .into_iter()
            .find(|(range, _)| compiler_version_satisfies(range) == Some(true));
        let Some((_, mappings)) = matching else {
            return Ok(Search::Continue);
        };
        match mappings {
            Value::Object(_) | Value::Array(_) => {}
            // TypeScript 6.0.3 reaches tryParsePatterns(null), whose WeakMap
            // access throws. Preserve that malformed-input failure as a typed
            // resolver error rather than silently falling back.
            Value::Null => {
                return Err(ResolutionError::invalid_data(format!(
                    "selected typesVersions paths in {} are null",
                    package.metadata.package_json().display().display()
                )));
            }
            // Other non-object values are rejected by
            // readPackageJsonTypesVersionPaths and legacy loading continues.
            Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                return Ok(Search::Continue);
            }
        }
        // The outer subpath paths phase re-observes the root package
        // directory only after an applicable version range was selected.
        // Directory-worker callers already supply their combined latch.
        let only_record_failures = if matches!(loader, TypesVersionsLoader::PackageSubpath) {
            only_record_failures || !self.host.directory_exists(Path::new(base_directory))?
        } else {
            only_record_failures
        };
        let Some((pattern, capture, targets)) =
            select_types_versions_mapping(mappings, logical_name)
        else {
            return Ok(Search::Continue);
        };
        // tsc calls its generic JavaScript `forEach` helper here rather than
        // validating an array. Preserve the observable array-like behavior:
        // strings iterate UTF-16 code units, objects iterate numeric keys up
        // to their JavaScript-coerced `length`, and primitive values without
        // a length perform no work. Callback values retain JavaScript's lazy
        // path coercion, so a successful early substitution never evaluates a
        // malformed later element.
        let outcome =
            try_for_each_types_versions_substitution(targets, &pattern, |substitution| {
                // tryLoadModuleUsingPaths treats an empty wildcard capture like
                // an exact mapping and retains a literal `*` in the target.
                let (expanded, written_extension) =
                    project_types_versions_substitution(substitution, capture, &pattern)?;
                let candidate = normalize_legacy_target_from_directory(base_directory, &expanded)?;
                if only_record_failures {
                    return Ok(None);
                }
                // tsc's paths loader first probes a substitution that already has
                // a recognized extension exactly, irrespective of the preferred
                // TypeScript/declaration pass. The paths loader itself returns an
                // exact hit without a package id; the outer package-root loader
                // may attach the root package id again. An exact miss falls
                // through to the ordinary package loader.
                if let Some(extension) = written_extension {
                    if let Some(resolved_path) = self.try_file(&candidate)? {
                        return self
                            .finish_legacy_resolution(
                                Some(package),
                                resolved_path.as_ref(),
                                extension,
                                LegacyResolutionContext {
                                    attach_package_id: attach_exact_package_id,
                                    resolved_using_ts_extension: false,
                                    ..context
                                },
                            )
                            .map(Some);
                    }
                }
                // tryLoadModuleUsingPaths latches onlyRecordFailures from the
                // expanded candidate's parent after its raw-substitution exact
                // shortcut. A later parent appearance must not revive the loader.
                if !self
                    .host
                    .directory_exists(Path::new(&directory_name(&candidate)))?
                {
                    return Ok(None);
                }
                let outcome = match loader {
                    TypesVersionsLoader::PackageDirectory => self.probe_package_field_path(
                        Some(package),
                        &candidate,
                        probe_pass,
                        !self.is_node_esm_mode(mode)
                            || package.metadata.module_type() != PackageJsonType::Module,
                        LegacyResolutionContext {
                            resolved_using_ts_extension: false,
                            ..context
                        },
                    )?,
                    TypesVersionsLoader::PackageSubpath => self.probe_package_subpath_path(
                        package, &candidate, probe_pass, mode, context,
                    )?,
                };
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(Some(outcome));
                }
                Ok(None)
            })?;
        Ok(Search::Terminal(
            outcome.unwrap_or(ResolutionOutcome::NotFound),
        ))
    }

    /// `loadNodeModuleFromDirectoryWorker` gives `types`, `typings`, `main`,
    /// and each root typesVersions substitution a package-json target phase
    /// before re-entering the ordinary relative-file loader. The phases
    /// intentionally retain duplicate probes: a transient host failure on the
    /// second exact lookup is observable upstream.
    fn probe_package_field_path(
        &self,
        package: Option<&CachedPackage>,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        allow_implicit: bool,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        // loadFileNameFromPackageJsonField always runs. A plain trailing
        // directory has no extension and performs no host work, while a
        // dotted trailing spelling has a path-bearing arbitrary declaration
        // twin (`dir.ext/` -> `dir.d.ext/.ts`).
        let outcome = self.probe_package_field_file_phase(
            package,
            candidate,
            probe_pass,
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )?;
        if matches!(outcome, ResolutionOutcome::Resolved(_)) {
            return Ok(outcome);
        }

        // A Declaration-only directory lookup expands to TypeScript plus
        // Declaration before invoking nodeLoadModuleByRelativeName. Other
        // extension masks pass through unchanged.
        let expanded_pass = if matches!(probe_pass, ExtensionProbePass::Declaration) {
            ExtensionProbePass::Preferred
        } else {
            probe_pass
        };
        if !candidate.ends_with('/') {
            // nodeLoadModuleByRelativeName preflights the candidate parent
            // before entering loadModuleFromFile, which then performs its own
            // per-stage directory observations. A trailing directory spelling
            // skips this complete file phase upstream.
            if !self
                .host
                .directory_exists(Path::new(&directory_name(candidate)))?
            {
                return Ok(ResolutionOutcome::NotFound);
            }
            let outcome = self.probe_legacy_file(
                package,
                candidate,
                expanded_pass,
                allow_implicit,
                LegacyResolutionContext {
                    // candidateIsFromPackageJsonField suppresses provenance
                    // in the expanded ordinary loader.
                    resolved_using_ts_extension: false,
                    ..context
                },
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }

        // Even in ESM mode, nodeLoadModuleByRelativeName observes the
        // candidate directory after a file miss before deciding that the
        // directory loader is disabled.
        let candidate_exists = self.host.directory_exists(Path::new(candidate))?;
        if !allow_implicit || !candidate_exists {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.probe_legacy_file(
            package,
            &join_normalized(candidate, "index"),
            expanded_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )
    }

    /// `loadFileNameFromPackageJsonField` without a raw package-map target.
    /// Exact TypeScript/declaration hits publish false provenance because no
    /// raw package-json value reaches this helper. Replacement hits preserve
    /// the ordinary written-extension provenance until the expanded loader
    /// explicitly suppresses it.
    ///
    /// tsc-port: loadFileNameFromPackageJsonField @6.0.3
    /// tsc-hash: 6ea552326abfc6171a6f748eff464c16f5f9de70fe1090df8251e7f3e41108fd
    /// tsc-span: _tsc.js:41184-41194
    fn probe_package_field_file_phase(
        &self,
        package: Option<&CachedPackage>,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        if let Some(extension) = package_json_target_exact_extension(candidate, probe_pass) {
            let Some(observed_path) = self.try_file(candidate)? else {
                return Ok(ResolutionOutcome::NotFound);
            };
            return self.finish_legacy_resolution_from_predicate(
                package,
                candidate,
                observed_path.as_ref(),
                extension,
                LegacyResolutionContext {
                    resolved_using_ts_extension: false,
                    ..context
                },
            );
        }

        let plan = match probe_pass {
            ExtensionProbePass::Declaration => declaration_extension_probe_plan(candidate),
            _ => extension_probe_plan(candidate, self.options.resolve_json_module_effective()),
        };
        let mut arbitrary_probe = None;
        let replacement = match plan {
            Ok((base, probes, preferred_len)) => Some((
                base,
                select_extension_probes(
                    probes,
                    preferred_len,
                    probe_pass,
                    recognized_module_extension(candidate),
                ),
            )),
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension" =>
            {
                if extension_pass_includes_declaration(probe_pass) {
                    arbitrary_probe = arbitrary_declaration_twin(candidate);
                }
                None
            }
            Err(error) => return Err(error),
        };
        if replacement
            .as_ref()
            .is_none_or(|(_, probes)| probes.is_empty())
            && arbitrary_probe.is_none()
        {
            if base_name(candidate).contains('.') {
                self.host
                    .directory_exists(Path::new(&directory_name(candidate)))?;
            }
            return Ok(ResolutionOutcome::NotFound);
        }
        if !self
            .host
            .directory_exists(Path::new(&directory_name(candidate)))?
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        if let Some((base, probes)) = replacement {
            for (extension, suffix) in probes {
                let path = format!("{base}{suffix}");
                if let Some(resolved_path) = self.try_file(&path)? {
                    let extension = materialize_module_extension(extension, suffix);
                    return self.finish_legacy_resolution(
                        package,
                        resolved_path.as_ref(),
                        extension.clone(),
                        LegacyResolutionContext {
                            resolved_using_ts_extension: is_typescript_family_specifier(candidate)
                                && is_typescript_module_extension(&extension),
                            ..context
                        },
                    );
                }
            }
        }
        if let Some((path, extension)) = arbitrary_probe {
            if let Some(resolved_path) = self.try_file(&path)? {
                return self.finish_legacy_resolution(
                    package,
                    resolved_path.as_ref(),
                    ModuleExtension::Arbitrary(extension),
                    LegacyResolutionContext {
                        resolved_using_ts_extension: false,
                        ..context
                    },
                );
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn probe_legacy_file(
        &self,
        package: Option<&CachedPackage>,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        allow_implicit: bool,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        // loadModuleFromFile has two distinct stages. A candidate with a
        // written extension first replaces that extension according to its
        // family. Outside Node ESM, a complete miss then appends the ordinary
        // extensionless family to the *whole* written candidate (`x.js.ts`,
        // for example). Package-map string targets call only the first stage
        // and are handled by probe_export_target.
        let has_written_extension = base_name(candidate).contains('.');
        let mut arbitrary_probe = None;
        let replacement = if has_written_extension {
            let plan = match probe_pass {
                ExtensionProbePass::Declaration => declaration_extension_probe_plan(candidate),
                _ => extension_probe_plan(candidate, self.options.resolve_json_module_effective()),
            };
            match plan {
                Ok((base, probes, preferred_len)) => Some((
                    base,
                    select_extension_probes(
                        probes,
                        preferred_len,
                        probe_pass,
                        recognized_module_extension(candidate),
                    ),
                )),
                Err(ResolutionError::Unsupported { feature, .. })
                    if feature == "module-target-extension" =>
                {
                    if extension_pass_includes_declaration(probe_pass) {
                        arbitrary_probe = arbitrary_declaration_twin(candidate);
                    }
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let implicit = allow_implicit.then(|| implicit_extension_probes(probe_pass));
        if !has_written_extension && implicit.is_none() {
            return Ok(ResolutionOutcome::NotFound);
        }

        // Each tryAddingExtensions call re-observes the parent. Do not merge
        // these preflights: disappearance, appearance, and host failures at
        // the second stage are part of the sequential resolver contract.
        if has_written_extension
            && self
                .host
                .directory_exists(Path::new(&directory_name(candidate)))?
        {
            if let Some((base, probes)) = replacement {
                for (extension, suffix) in probes {
                    let path = format!("{base}{suffix}");
                    if let Some(resolved_path) = self.try_file(&path)? {
                        let extension = materialize_module_extension(extension, suffix);
                        return self.finish_legacy_resolution(
                            package,
                            resolved_path.as_ref(),
                            extension.clone(),
                            LegacyResolutionContext {
                                resolved_using_ts_extension: context.resolved_using_ts_extension
                                    && is_typescript_family_specifier(candidate)
                                    && is_typescript_module_extension(&extension),
                                ..context
                            },
                        );
                    }
                }
            }
            if let Some((path, extension)) = arbitrary_probe {
                if let Some(resolved_path) = self.try_file(&path)? {
                    return self.finish_legacy_resolution(
                        package,
                        resolved_path.as_ref(),
                        ModuleExtension::Arbitrary(extension),
                        LegacyResolutionContext {
                            resolved_using_ts_extension: false,
                            ..context
                        },
                    );
                }
            }
        }
        if let Some(probes) = implicit {
            if !self
                .host
                .directory_exists(Path::new(&directory_name(candidate)))?
            {
                return Ok(ResolutionOutcome::NotFound);
            }
            for (extension, suffix) in probes {
                let path = format!("{candidate}{suffix}");
                if let Some(resolved_path) = self.try_file(&path)? {
                    return self.finish_legacy_resolution(
                        package,
                        resolved_path.as_ref(),
                        materialize_module_extension(extension, suffix),
                        LegacyResolutionContext {
                            // tryAddingExtensions receives an empty original
                            // extension for this second stage, so even a TS
                            // result does not carry TS-extension provenance.
                            resolved_using_ts_extension: false,
                            ..context
                        },
                    );
                }
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn is_node_esm_mode(&self, mode: ResolutionMode) -> bool {
        matches!(self.options.emit_module_resolution_kind(), 3 | 99)
            && mode == ResolutionMode::EsNext
    }

    fn find_nearest_package_scope(
        &mut self,
        containing_directory: &str,
    ) -> Result<Option<Rc<CachedPackage>>, ResolutionError> {
        for ancestor in ancestor_directories(containing_directory) {
            let package_json = join_normalized(&ancestor, "package.json");
            if let Some(package) = self.load_package(&package_json)? {
                return Ok(Some(package));
            }
        }
        Ok(None)
    }

    fn load_package(
        &mut self,
        package_json: &str,
    ) -> Result<Option<Rc<CachedPackage>>, ResolutionError> {
        let cache_key = canonical_text(
            package_json,
            self.path_context.use_case_sensitive_file_names(),
        );
        if self.package_cache_enabled {
            if let Some(entry) = self.package_cache.get(&cache_key) {
                return Ok(match entry {
                    PackageCacheEntry::Missing => None,
                    PackageCacheEntry::Found(package) => Some(Rc::clone(package)),
                });
            }
        }

        let package_json_path = Path::new(package_json);
        let package_directory = directory_name(package_json);
        if !self.host.directory_exists(Path::new(&package_directory))?
            || !self.host.file_exists(package_json_path)?
        {
            if self.package_cache_enabled {
                self.package_cache
                    .insert(cache_key, PackageCacheEntry::Missing);
            }
            return Ok(None);
        }
        // TypeScript's readJson treats an absent read after a successful
        // file-existence probe as an empty object. This can occur across a
        // filesystem race; it remains a present cached package boundary.
        let bytes = self.host.read_file(package_json_path)?.unwrap_or_default();
        let text = decode_host_text(bytes).map_err(|error| {
            ResolutionError::invalid_data(format!(
                "cannot decode {}: {error}",
                Path::new(package_json).display()
            ))
        })?;
        let (text, object) = parse_json_object(package_json_path, text);

        let package_path = self.program_path(package_json)?;
        let name = json_object_get(&object, "name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let version = json_object_get(&object, "version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let module_type = match json_object_get(&object, "type").and_then(Value::as_str) {
            Some("module") => PackageJsonType::Module,
            Some("commonjs") => PackageJsonType::CommonJs,
            Some(_) => PackageJsonType::Other,
            None => PackageJsonType::Unspecified,
        };
        let metadata = Rc::new(PackageMetadata::from_trusted_parsed(
            package_path,
            text,
            name,
            version,
            module_type,
        ));
        let package = Rc::new(CachedPackage {
            root: directory_name(package_json),
            exports: json_object_get(&object, "exports").cloned(),
            has_own_exports: json_object_own_get(&object, "exports").is_some(),
            imports: json_object_get(&object, "imports").cloned(),
            // readPackageJsonField(..., "object") reports a malformed field
            // only through tracing and exposes it to resolution as absent.
            types_versions: json_object_own_get(&object, "typesVersions")
                // JavaScript's object-type test admits arrays; their numeric
                // own keys participate in version selection.
                .filter(|value| value.is_object() || value.is_array())
                .cloned(),
            typings: non_empty_string_field(&object, "typings"),
            types: non_empty_string_field(&object, "types"),
            main: non_empty_string_field(&object, "main"),
            tsconfig: non_empty_string_field(&object, "tsconfig"),
            metadata,
        });
        if self.package_cache_enabled {
            self.package_cache
                .insert(cache_key, PackageCacheEntry::Found(Rc::clone(&package)));
        }
        Ok(Some(package))
    }

    /// tsc-port: loadModuleFromExports @6.0.3
    /// tsc-hash: d64ca654fc853b01792ee9ffc748787fc9f080c30386c42e9f2bf20f5b4bf5bc
    /// tsc-span: _tsc.js:41471-41533
    fn resolve_package_exports(
        &mut self,
        package: &CachedPackage,
        subpath: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let search = self.search_package_exports(
            package,
            subpath,
            is_external_library_import,
            probe_pass,
            mode,
            self.options.emit_module_resolution_kind(),
            follow_realpath,
        )?;
        Ok(match search {
            // A present exports map suppresses every legacy package fallback.
            Search::Continue => ResolutionOutcome::NotFound,
            Search::Terminal(outcome) => outcome,
        })
    }

    #[allow(clippy::too_many_arguments)] // Conditions and extension masks vary independently.
    fn resolve_package_exports_with_resolution_kind(
        &mut self,
        package: &CachedPackage,
        subpath: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        resolution_kind: i32,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let search = self.search_package_exports(
            package,
            subpath,
            is_external_library_import,
            probe_pass,
            mode,
            resolution_kind,
            follow_realpath,
        )?;
        Ok(match search {
            Search::Continue => ResolutionOutcome::NotFound,
            Search::Terminal(outcome) => outcome,
        })
    }

    /// Preserve the upstream SearchResult distinction for self references:
    /// an ordinary target miss continues to node_modules, while an explicit
    /// null target is terminal.
    #[allow(clippy::too_many_arguments)] // Mirrors loadModuleFromExports' explicit state tuple.
    fn search_package_exports(
        &mut self,
        package: &CachedPackage,
        subpath: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        resolution_kind: i32,
        follow_realpath: bool,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let exports = package.exports.as_ref().ok_or_else(|| {
            ResolutionError::unsupported(
                "legacy-node-package-entry",
                format!(
                    "{} has no exports field",
                    package.metadata.package_json().display().display()
                ),
            )
        })?;

        let context = ExportProbeContext {
            is_external_library_import,
            follow_realpath,
            pass: probe_pass,
            mode,
            resolution_kind,
            exports_pattern_trailers: exports_pattern_trailers_enabled(mode, resolution_kind),
            kind: PackageMapKind::Exports,
            bare_features: None,
        };

        let search = match exports {
            Value::String(_) if subpath == "." => {
                self.resolve_selected_export(package, exports, "", false, context)?
            }
            Value::String(_) => Search::Continue,
            Value::Object(table) => {
                let mut own_keys = table.keys().filter_map(|key| decode_user_object_key(key));
                let no_key_starts_with_dot = own_keys.clone().all(|key| !key.starts_with('.'));
                let all_keys_start_with_dot = own_keys.all(|key| key.starts_with('.'));

                if subpath == "." {
                    if no_key_starts_with_dot {
                        self.resolve_selected_export(package, exports, "", false, context)?
                    } else if let Some(main_export) = json_object_own_get(table, ".") {
                        if js_json_value_is_truthy(main_export) {
                            self.resolve_selected_export(package, main_export, "", false, context)?
                        } else {
                            Search::Continue
                        }
                    } else {
                        Search::Continue
                    }
                } else if all_keys_start_with_dot {
                    self.search_exports_table(package, table, subpath, context)?
                } else {
                    Search::Continue
                }
            }
            Value::Array(_) if subpath == "." => {
                self.resolve_selected_export(package, exports, "", false, context)?
            }
            Value::Array(_) => Search::Continue,
            Value::Bool(_) | Value::Number(_) => Search::Continue,
            // Falsy exports never enter this worker, but retaining Continue
            // here keeps the helper fail-safe if an internal caller changes.
            Value::Null => Search::Continue,
        };

        Ok(search)
    }

    /// tsc-port: loadModuleFromExportsOrImports @6.0.3
    /// tsc-hash: a0b5d92673856e6203a41bb797a6a332ed2fd142e7777d6ad148f19dd189af4e
    /// tsc-span: _tsc.js:41600-41654
    fn search_exports_table(
        &mut self,
        package: &CachedPackage,
        table: &Map<String, Value>,
        subpath: &str,
        context: ExportProbeContext,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let Some(selected) =
            select_package_map_target(table, subpath, context.exports_pattern_trailers)
        else {
            return Ok(Search::Continue);
        };
        self.resolve_selected_export(
            package,
            selected.target,
            &selected.subpath,
            selected.pattern,
            context,
        )
    }

    /// tsc-port: getLoadModuleFromTargetExportOrImport @6.0.3
    /// tsc-hash: 53140e49d3d9c87a08a45ee1da483817e6da6a64062106b27a96bc0ad9d64717
    /// tsc-span: _tsc.js:41659-41883
    fn resolve_selected_export(
        &mut self,
        package: &CachedPackage,
        target: &Value,
        subpath: &str,
        pattern: bool,
        context: ExportProbeContext,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        match target {
            Value::Null => Ok(Search::Terminal(ResolutionOutcome::NotFound)),
            Value::String(raw_target) => {
                if !pattern && !subpath.is_empty() && !raw_target.ends_with('/') {
                    return Ok(Search::Continue);
                }
                if context.kind == PackageMapKind::Imports && !raw_target.starts_with("./") {
                    let Some(target) = expand_imports_bare_target(raw_target, subpath, pattern)?
                    else {
                        return Ok(Search::Continue);
                    };
                    return self.resolve_bare_import_target(package, &target, context);
                }
                let Some(target) =
                    expand_export_target(&package.root, raw_target, subpath, pattern)?
                else {
                    return Ok(Search::Continue);
                };
                let candidate = normalize_absolute_path(Path::new(&target), None)?;
                if !path_is_within(&candidate, &package.root) {
                    return Ok(Search::Continue);
                }
                let resolved = self.probe_export_target(
                    package,
                    &candidate,
                    context,
                    /* attach_package_id */ true,
                    /* raw_package_target */ Some(raw_target),
                )?;
                Ok(if matches!(resolved, ResolutionOutcome::Resolved(_)) {
                    Search::Terminal(resolved)
                } else {
                    Search::Continue
                })
            }
            Value::Object(conditions) => {
                for (condition, target) in js_own_property_entries(conditions) {
                    if !self.package_condition_matches(
                        condition,
                        context.mode,
                        context.resolution_kind,
                    ) {
                        continue;
                    }
                    let result =
                        self.resolve_selected_export(package, target, subpath, pattern, context)?;
                    if !matches!(result, Search::Continue) {
                        return Ok(result);
                    }
                }
                Ok(Search::Continue)
            }
            Value::Array(targets) => {
                if targets.is_empty() {
                    return Ok(Search::Continue);
                }
                for target in targets {
                    let result =
                        self.resolve_selected_export(package, target, subpath, pattern, context)?;
                    if !matches!(result, Search::Continue) {
                        return Ok(result);
                    }
                }
                Ok(Search::Continue)
            }
            Value::Bool(_) | Value::Number(_) => Ok(Search::Continue),
        }
    }

    /// tsc-port: getConditions @6.0.3
    /// tsc-hash: 91d1ed5895417f7bdcea21d875a24ebbf5b54004698e26c46a9b89e7a0808140
    /// tsc-span: _tsc.js:40276-40291
    ///
    /// tsc-port: isApplicableVersionedTypesKey @6.0.3
    /// tsc-hash: 9af5528adbf587055e813a06f658baa9b9b865f96672f1f2c05c85669b4e7222
    /// tsc-span: _tsc.js:41884-41890
    fn package_condition_matches(
        &self,
        condition: &str,
        mode: ResolutionMode,
        resolution_kind: i32,
    ) -> bool {
        if condition == "default" {
            return true;
        }
        // getConditions returns an empty set for Node10 when package exports
        // were explicitly enabled without a per-directive resolution mode.
        if mode == ResolutionMode::Unspecified && resolution_kind == 2 {
            return false;
        }
        let mode = if mode == ResolutionMode::Unspecified && resolution_kind == 100 {
            ResolutionMode::EsNext
        } else {
            mode
        };
        if (condition == "import" && mode == ResolutionMode::EsNext)
            || (condition == "require" && mode != ResolutionMode::EsNext)
        {
            return true;
        }
        if condition == "types" {
            return true;
        }
        if condition == "node" {
            return resolution_kind != 100;
        }
        if condition
            .strip_prefix("types@")
            .is_some_and(|range| compiler_version_satisfies(range) == Some(true))
        {
            return true;
        }
        self.options
            .custom_conditions
            .as_ref()
            .is_some_and(|conditions| conditions.iter().any(|candidate| candidate == condition))
    }

    /// tsc-port: tryAddingExtensions @6.0.3
    /// tsc-hash: ce6ede18162cf6d430e57f3971a4f9018eca6a56b790d28b7d46ab3e6310ec2b
    /// tsc-span: _tsc.js:41196-41229
    fn probe_export_target(
        &self,
        package: &CachedPackage,
        target: &str,
        context: ExportProbeContext,
        attach_package_id: bool,
        raw_package_target: Option<&str>,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        // loadFileNameFromPackageJsonField performs an exact-only fast path
        // for an admitted TS implementation or declaration extension. An
        // exact miss must not fall through to sibling TS/declaration probes
        // in the same pass.
        if let Some(extension) = package_json_target_exact_extension(target, context.pass) {
            let Some(observed_path) = self.try_file(target)? else {
                return Ok(ResolutionOutcome::NotFound);
            };
            let resolved_using_ts_extension =
                raw_package_target.is_some_and(|raw| !raw.ends_with(extension.as_str()));
            return self.finish_legacy_resolution_from_predicate(
                Some(package),
                target,
                observed_path.as_ref(),
                extension,
                LegacyResolutionContext {
                    is_external_library_import: context.is_external_library_import,
                    attach_package_id,
                    resolved_using_ts_extension,
                    follow_realpath: context.is_external_library_import && context.follow_realpath,
                },
            );
        }

        let plan = match context.pass {
            ExtensionProbePass::Declaration => declaration_extension_probe_plan(target),
            _ => extension_probe_plan(target, self.options.resolve_json_module_effective()),
        };
        let mut arbitrary_probe = None;
        let replacement = match plan {
            Ok((base, probes, preferred_len)) => Some((
                base,
                select_extension_probes(
                    probes,
                    preferred_len,
                    context.pass,
                    recognized_module_extension(target),
                ),
            )),
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension" =>
            {
                if extension_pass_includes_declaration(context.pass) {
                    arbitrary_probe = arbitrary_declaration_twin(target);
                }
                None
            }
            Err(error) => return Err(error),
        };
        if replacement
            .as_ref()
            .is_none_or(|(_, probes)| probes.is_empty())
            && arbitrary_probe.is_none()
        {
            if base_name(target).contains('.') {
                self.host
                    .directory_exists(Path::new(&directory_name(target)))?;
            }
            return Ok(ResolutionOutcome::NotFound);
        }
        let target_directory = directory_name(target);
        if !self.host.directory_exists(Path::new(&target_directory))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        if let Some((base, probes)) = replacement {
            for (extension, suffix) in probes {
                let candidate = format!("{base}{suffix}");
                if let Some(resolved_path) = self.try_file(&candidate)? {
                    let extension = materialize_module_extension(extension, suffix);
                    let resolved_using_ts_extension = candidate != target
                        && is_typescript_family_specifier(target)
                        && is_typescript_module_extension(&extension);
                    return self.finish_resolution(
                        package,
                        resolved_path.as_ref(),
                        extension,
                        context.is_external_library_import,
                        attach_package_id,
                        resolved_using_ts_extension,
                        context.follow_realpath,
                    );
                }
            }
        }
        if let Some((candidate, extension)) = arbitrary_probe {
            if let Some(resolved_path) = self.try_file(&candidate)? {
                return self.finish_resolution(
                    package,
                    resolved_path.as_ref(),
                    ModuleExtension::Arbitrary(extension),
                    context.is_external_library_import,
                    attach_package_id,
                    false,
                    context.follow_realpath,
                );
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    /// tsc-port: withPackageId @6.0.3
    /// tsc-hash: 714c67b6e906e185d5b4f85b128147b60ec24d8a1bd1c82b386103fc5ddf3eb0
    /// tsc-span: _tsc.js:39824-39838
    #[allow(clippy::too_many_arguments)] // Resolution provenance fields are independently observable.
    fn finish_resolution(
        &self,
        package: &CachedPackage,
        lexical_path: &str,
        extension: ModuleExtension,
        is_external_library_import: bool,
        attach_package_id: bool,
        resolved_using_ts_extension: bool,
        follow_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.finish_legacy_resolution(
            Some(package),
            lexical_path,
            extension,
            LegacyResolutionContext {
                is_external_library_import,
                attach_package_id,
                resolved_using_ts_extension,
                follow_realpath: is_external_library_import && follow_realpath,
            },
        )
    }

    fn finish_legacy_resolution(
        &self,
        package: Option<&CachedPackage>,
        lexical_path: &str,
        extension: ModuleExtension,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.finish_legacy_resolution_worker(
            package,
            lexical_path,
            extension,
            context,
            /* allow_missing_realpath */ false,
        )
    }

    /// `loadFileNameFromPackageJsonField` uses `tryFile` only as a predicate:
    /// a suffix hit still publishes the unsuffixed input path. Its later
    /// realpath observation therefore may legitimately return no entry even
    /// though the different suffixed path passed `fileExists`.
    fn finish_legacy_resolution_from_predicate(
        &self,
        package: Option<&CachedPackage>,
        lexical_path: &str,
        observed_path: &str,
        extension: ModuleExtension,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.finish_legacy_resolution_worker(
            package,
            lexical_path,
            extension,
            context,
            observed_path != lexical_path,
        )
    }

    fn finish_legacy_resolution_worker(
        &self,
        package: Option<&CachedPackage>,
        lexical_path: &str,
        extension: ModuleExtension,
        context: LegacyResolutionContext,
        allow_missing_realpath: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let (resolved_file, original_path) = if context.follow_realpath {
            self.realpath_program_path(lexical_path, allow_missing_realpath)?
        } else {
            (self.selected_program_path(lexical_path)?, None)
        };

        let package_id = package
            .map(|package| {
                package_id_for_legacy_path(package, lexical_path, context.attach_package_id)
            })
            .transpose()?
            .flatten();

        Ok(ResolutionOutcome::Resolved(HostResolvedModule {
            resolved_file,
            extension,
            original_path,
            is_external_library_import: context.is_external_library_import,
            resolved_using_ts_extension: context.resolved_using_ts_extension,
            package_id,
            alternate_result: None,
            package_metadata: package.map(|package| Rc::clone(&package.metadata)),
            realpath_may_be_missing_after_suffix_predicate: allow_missing_realpath
                && !context.follow_realpath,
        }))
    }

    /// tsc-port: createResolvedModuleWithFailedLookupLocationsHandlingSymlink @6.0.3
    /// tsc-hash: cab205c9a3d51deba209d25ffd81c71f1b1f11303380d04b0d2554fd89f5096a
    /// tsc-span: _tsc.js:39869-39885
    /// tsc-port: resolveTypeReferenceDirective @6.0.3
    /// tsc-hash: 3af6ebb2bcaac43b8fd32ed3ae31fb8840d2db68a2facd74f6ab560ed8f1fb22
    /// tsc-span: _tsc.js:40130-40144
    fn realpath_program_path(
        &self,
        lexical_path: &str,
        allow_missing: bool,
    ) -> Result<(ProgramPath, Option<ProgramPath>), ResolutionError> {
        let lexical = self.selected_program_path(lexical_path)?;
        if self.preserve_symlinks {
            return Ok((lexical, None));
        }
        let Some(real_path) = self.host.realpath(Path::new(lexical_path))? else {
            if allow_missing {
                return Ok((lexical, None));
            }
            return Err(ResolutionError::invalid_data(format!(
                "host reported {} as a file but returned no realpath",
                Path::new(lexical_path).display()
            )));
        };
        let normalized_real_path =
            normalize_absolute_path(&real_path, Some(self.current_directory_text()?))?;
        let real = self.program_path(&normalized_real_path)?;
        if real.canonical() == lexical.canonical() {
            Ok((lexical, None))
        } else {
            Ok((real, Some(lexical)))
        }
    }

    fn program_path(&self, normalized_path: &str) -> Result<ProgramPath, ResolutionError> {
        make_program_path(
            normalized_path,
            self.path_context.use_case_sensitive_file_names(),
        )
    }

    /// Preserve the resolver/host-facing selected spelling while deriving the
    /// program/cache identity through TypeScript's normalized `toPath` shape.
    ///
    /// tsc-port: toPath @6.0.3
    /// tsc-hash: 5cdd1b7580ac2e90008c10ad0aa3e12c568dc15f993d8a8eb61c5f00c93a1456
    /// tsc-span: _tsc.js:5600-5602
    fn selected_program_path(&self, selected_path: &str) -> Result<ProgramPath, ResolutionError> {
        let normalized = normalize_absolute_path(
            Path::new(selected_path),
            Some(self.current_directory_text()?),
        )?;
        let canonical = canonical_text(
            &normalized,
            self.path_context.use_case_sensitive_file_names(),
        );
        ProgramPath::from_trusted_parts(selected_path, canonical).map_err(|error| {
            ResolutionError::canonicalization(Some(PathBuf::from(selected_path)), error.to_string())
        })
    }
}

fn normalize_base_url(
    base_url: Option<&str>,
    current_directory: &str,
) -> Result<Option<String>, ResolutionError> {
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    validate_owned_path_text(base_url, "baseUrl", /* allow_empty */ false)?;
    normalize_absolute_path(Path::new(base_url), Some(current_directory)).map(Some)
}

fn normalize_paths_base_path(
    paths_base_path: &str,
    current_directory: &str,
) -> Result<String, ResolutionError> {
    validate_owned_path_text(
        paths_base_path,
        "pathsBasePath",
        /* allow_empty */ false,
    )?;
    normalize_absolute_path(Path::new(paths_base_path), Some(current_directory))
}

fn validate_and_clone_root_dirs(
    root_dirs: Option<&[ProgramPath]>,
    current_directory: &str,
    case_sensitive: bool,
) -> Result<Option<Vec<String>>, ResolutionError> {
    let Some(root_dirs) = root_dirs else {
        return Ok(None);
    };
    let mut normalized_roots = Vec::with_capacity(root_dirs.len());
    for root_dir in root_dirs {
        let display = root_dir.display();
        let text = display.to_str().ok_or_else(|| {
            ResolutionError::canonicalization(
                Some(display.to_path_buf()),
                "rootDirs entry is not valid Unicode",
            )
        })?;
        validate_owned_path_text(text, "rootDirs entry", /* allow_empty */ false)?;
        let normalized = normalize_absolute_path(display, Some(current_directory))?;
        let expected = make_program_path(&normalized, case_sensitive)?;
        if &expected != root_dir {
            return Err(ResolutionError::canonicalization(
                Some(display.to_path_buf()),
                "rootDirs entry does not match the resolver's normalized display and canonical path profile",
            ));
        }
        normalized_roots.push(normalized);
    }
    Ok(Some(normalized_roots))
}

fn normalize_optional_candidate(
    candidate: &str,
    base_directory: &str,
) -> Result<String, ResolutionError> {
    if candidate.is_empty() {
        return Ok(base_directory.to_owned());
    }
    validate_owned_path_text(
        candidate,
        "optional resolution candidate",
        /* allow_empty */ true,
    )?;
    normalize_absolute_path(Path::new(candidate), Some(base_directory))
        .map(|normalized| preserve_trailing_directory_separator(normalized, candidate))
}

fn preserve_trailing_directory_separator(mut normalized: String, source: &str) -> String {
    if source.ends_with(['/', '\\']) && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn preserve_node_directory_spelling(mut normalized: String, directory_spelling: bool) -> String {
    if directory_spelling && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn combine_paths_spelling(parent: &str, child: &str) -> Result<String, ResolutionError> {
    if child.contains('\0') {
        return Err(ResolutionError::invalid_data(
            "type-reference directive contains a NUL byte",
        ));
    }
    let child = child.replace('\\', "/");
    if child.is_empty() {
        return Ok(parent.to_owned());
    }
    if is_normalized_rooted_text(&child) || child.starts_with("//") {
        return Ok(child);
    }
    if parent.ends_with('/') {
        Ok(format!("{parent}{child}"))
    } else {
        Ok(format!("{parent}/{child}"))
    }
}

fn validate_paths(
    paths: Option<Arc<ProgramPathMappings>>,
) -> Result<Option<Arc<ProgramPathMappings>>, ResolutionError> {
    let Some(paths) = paths else {
        return Ok(None);
    };
    if let Some(error) = paths.validation_error() {
        return Err(error.clone());
    }
    Ok(Some(paths))
}

pub(crate) fn validate_owned_path_text(
    value: &str,
    role: &str,
    allow_empty: bool,
) -> Result<(), ResolutionError> {
    if (!allow_empty && value.is_empty()) || value.contains('\0') {
        return Err(ResolutionError::invalid_data(format!(
            "{role} is empty or contains a NUL byte"
        )));
    }
    if value.is_empty() {
        return Ok(());
    }
    let slashed = value.replace('\\', "/");
    let drive_relative = slashed.len() >= 2
        && slashed.as_bytes()[0].is_ascii_alphabetic()
        && slashed.as_bytes()[1] == b':'
        && slashed.as_bytes().get(2) != Some(&b'/');
    let windows_root_relative = value.starts_with('\\') && !value.starts_with("\\\\");
    if slashed.starts_with("//") || drive_relative || windows_root_relative {
        return Err(ResolutionError::unsupported(
            "windows-path-form",
            format!(
                "{role} {value:?} uses an unowned UNC, extended-length, root-relative, or drive-relative path form"
            ),
        ));
    }
    Ok(())
}

fn validate_path_context(
    host: &dyn CompilerHost,
    path_context: &PathContext,
) -> Result<(), ResolutionError> {
    if host.use_case_sensitive_file_names() != path_context.use_case_sensitive_file_names() {
        return Err(ResolutionError::invalid_data(
            "path context case-sensitivity does not match the compiler host",
        ));
    }
    let host_current_directory = host.current_directory()?;
    let normalized_host_current_directory = normalize_absolute_path(&host_current_directory, None)?;
    let display = path_context.current_directory().display();
    let normalized = normalize_absolute_path(display, None)?;
    if display.to_str() != Some(normalized.as_str()) {
        return Err(ResolutionError::canonicalization(
            Some(display.to_path_buf()),
            "path context current directory is not lexically normalized",
        ));
    }
    if normalized != normalized_host_current_directory {
        return Err(ResolutionError::canonicalization(
            Some(display.to_path_buf()),
            format!(
                "path context current directory does not match host current directory {}",
                host_current_directory.display()
            ),
        ));
    }
    let expected = canonical_text(&normalized, path_context.use_case_sensitive_file_names());
    if path_context
        .current_directory()
        .canonical()
        .as_path()
        .to_str()
        != Some(expected.as_str())
    {
        return Err(ResolutionError::canonicalization(
            Some(display.to_path_buf()),
            "path context canonical current directory does not match the host profile",
        ));
    }
    Ok(())
}

fn non_empty_string_field(object: &Map<String, Value>, field: &str) -> Option<String> {
    json_object_own_get(object, field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn package_subpath(exports_subpath: &str) -> Result<Option<&str>, ResolutionError> {
    if exports_subpath == "." {
        return Ok(None);
    }
    exports_subpath
        .strip_prefix("./")
        .filter(|subpath| !subpath.is_empty())
        .map(Some)
        .ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "invalid normalized package subpath {exports_subpath:?}"
            ))
        })
}

fn normalize_legacy_package_target(
    package: &CachedPackage,
    target: &str,
) -> Result<String, ResolutionError> {
    normalize_legacy_target_from_directory(&package.root, target)
}

fn normalize_legacy_target_from_directory(
    base_directory: &str,
    target: &str,
) -> Result<String, ResolutionError> {
    if target.is_empty() {
        return Ok(base_directory.to_owned());
    }
    normalize_absolute_path(Path::new(target), Some(base_directory))
        .map(|candidate| preserve_trailing_directory_separator(candidate, target))
}

fn normalize_package_subpath(
    package: &CachedPackage,
    target: &str,
) -> Result<String, ResolutionError> {
    let target = target.strip_prefix("./").unwrap_or(target);
    if target.is_empty() || target.starts_with(['/', '\\']) || target.contains(['\\', '\0', ':']) {
        return Err(ResolutionError::invalid_data(format!(
            "package target {target:?} is not package-relative"
        )));
    }
    let candidate =
        normalize_absolute_path(Path::new(&join_normalized(&package.root, target)), None)?;
    if !path_is_within(&candidate, &package.root) {
        return Err(ResolutionError::invalid_data(format!(
            "package target {candidate} escapes package {}",
            package.root
        )));
    }
    Ok(candidate)
}

fn selected_package_entry_field(
    package: &CachedPackage,
    probe_pass: ExtensionProbePass,
) -> Option<&str> {
    match probe_pass {
        ExtensionProbePass::Empty => None,
        ExtensionProbePass::JsonConfig => package.tsconfig.as_deref(),
        ExtensionProbePass::JsonModule => None,
        ExtensionProbePass::All
        | ExtensionProbePass::Preferred
        | ExtensionProbePass::Declaration => package
            .typings
            .as_deref()
            .or(package.types.as_deref())
            .or(package.main.as_deref()),
        ExtensionProbePass::Fallback => package.main.as_deref(),
    }
}

fn parse_package_request(specifier: &str) -> Result<PackageRequest<'_>, ResolutionError> {
    if is_relative_specifier(specifier)
        || specifier.starts_with(['/', '\\'])
        || specifier.contains('\0')
    {
        return Err(ResolutionError::unsupported(
            "non-bare-module-specifier",
            format!("the H0.2b exports resolver cannot resolve {specifier:?}"),
        ));
    }

    let package_end = if specifier.starts_with('@') {
        // parsePackageName treats the second separator as the scoped package
        // boundary. With no second separator, even malformed spellings such
        // as `@scope` remain one observable package name.
        specifier
            .find('/')
            .and_then(|scope_end| {
                specifier[scope_end + 1..]
                    .find('/')
                    .map(|relative| scope_end + 1 + relative)
            })
            .unwrap_or(specifier.len())
    } else {
        specifier.find('/').unwrap_or(specifier.len())
    };

    let package_name = &specifier[..package_end];
    let has_subpath_separator = package_end < specifier.len();
    let rest = specifier
        .get(package_end + usize::from(has_subpath_separator)..)
        .unwrap_or("");
    Ok(PackageRequest {
        package_name,
        exports_subpath: if rest.is_empty() {
            ".".to_owned()
        } else {
            format!("./{rest}")
        },
        trailing_separator: has_subpath_separator && rest.is_empty(),
    })
}

fn is_relative_specifier(specifier: &str) -> bool {
    is_path_relative_specifier(specifier) || is_supported_rooted_specifier(specifier)
}

fn is_path_relative_specifier(specifier: &str) -> bool {
    matches!(specifier, "." | "..")
        || specifier.starts_with("./")
        || specifier.starts_with(".\\")
        || specifier.starts_with("../")
        || specifier.starts_with("..\\")
}

/// tsc-port: isExternalModuleNameRelative @6.0.3
/// tsc-hash: e5546324dce58e277ab9df485e26bb2c9cafa5a7e7b154366be6fc45784ad14d
/// tsc-span: _tsc.js:11234-11236
pub(crate) fn is_external_module_name_relative(module_name: &str) -> bool {
    is_path_relative_specifier(module_name) || is_rooted_disk_path(module_name)
}

fn is_supported_rooted_specifier(specifier: &str) -> bool {
    (specifier.starts_with('/') && !specifier.starts_with("//"))
        || (specifier.len() >= 3
            && specifier.as_bytes()[0].is_ascii_alphabetic()
            && specifier.as_bytes()[1] == b':'
            && matches!(specifier.as_bytes()[2], b'/' | b'\\'))
}

fn has_node_directory_spelling(specifier: &str) -> bool {
    specifier.ends_with(['/', '\\'])
        || matches!(specifier.rsplit(['/', '\\']).next(), Some("." | ".."))
}

fn is_typescript_family_specifier(specifier: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| specifier.ends_with(extension))
}

fn is_typescript_module_extension(extension: &ModuleExtension) -> bool {
    matches!(
        extension,
        ModuleExtension::Ts
            | ModuleExtension::Tsx
            | ModuleExtension::Dts
            | ModuleExtension::Mts
            | ModuleExtension::Dmts
            | ModuleExtension::Cts
            | ModuleExtension::Dcts
    )
}

fn path_contains_node_modules(path: &str) -> bool {
    path.split('/').any(|component| component == "node_modules")
}

fn node_modules_package_root(path: &str) -> Option<String> {
    const MARKER: &str = "/node_modules/";
    let marker = path.rfind(MARKER)?;
    let package_start = marker + MARKER.len();
    let move_to_separator = |previous: usize| {
        path.get(previous + 1..)
            .and_then(|suffix| suffix.find('/'))
            .map_or(previous, |relative| previous + 1 + relative)
    };
    let mut package_end = move_to_separator(package_start);
    if path.as_bytes().get(package_start) == Some(&b'@') {
        package_end = move_to_separator(package_end);
    }
    Some(path[..package_end].to_owned())
}

fn package_id_for_legacy_path(
    package: &CachedPackage,
    lexical_path: &str,
    attach_package_id: bool,
) -> Result<Option<PackageId>, ResolutionError> {
    package_id_for_legacy_path_from_directory(
        package,
        &package.root,
        lexical_path,
        attach_package_id,
    )
}

fn package_id_for_legacy_path_from_directory(
    package: &CachedPackage,
    package_directory: &str,
    lexical_path: &str,
    attach_package_id: bool,
) -> Result<Option<PackageId>, ResolutionError> {
    let (Some(name), Some(version)) = (package.metadata.name(), package.metadata.version()) else {
        return Ok(None);
    };
    if !attach_package_id {
        return Ok(None);
    }
    // withPackageId slices one UTF-16 code unit after the package directory
    // length without a containment check. Legacy package fields and
    // typesVersions substitutions may intentionally escape the package root,
    // so even their odd sliced spelling is observable.
    let start = package_directory.encode_utf16().count().saturating_add(1);
    let units = lexical_path.encode_utf16().skip(start).collect::<Vec<_>>();
    let submodule_name = String::from_utf16_lossy(&units);
    Ok(Some(PackageId::new(name, submodule_name, version)))
}

fn arbitrary_declaration_twin(candidate: &str) -> Option<(String, String)> {
    let file_name = base_name(candidate);
    let dot = file_name.rfind('.')?;
    let mut original_extension = file_name[dot..].to_owned();
    if candidate.ends_with('/') {
        original_extension.push('/');
    }
    let base = candidate.get(..candidate.len().checked_sub(original_extension.len())?)?;
    let extension = format!(".d{original_extension}.ts");
    Some((format!("{base}{extension}"), extension))
}

fn select_extension_probes(
    probes: &'static [ExtensionProbe],
    preferred_len: usize,
    pass: ExtensionProbePass,
    written_extension: Option<ModuleExtension>,
) -> &'static [ExtensionProbe] {
    match pass {
        ExtensionProbePass::All | ExtensionProbePass::Declaration => probes,
        ExtensionProbePass::Preferred => &probes[..preferred_len],
        ExtensionProbePass::Fallback => &probes[preferred_len..],
        ExtensionProbePass::Empty => &probes[..0],
        // With the JSON-only mask, tryAddingExtensions reaches its config
        // fallback only for the .ts/.d.ts/.js family (and exact .json). Other
        // written families miss this replacement phase and may only reach the
        // later whole-candidate implicit append.
        ExtensionProbePass::JsonConfig
            if matches!(
                written_extension,
                Some(
                    ModuleExtension::Ts
                        | ModuleExtension::Dts
                        | ModuleExtension::Js
                        | ModuleExtension::Json
                )
            ) =>
        {
            JSON_CONFIG_PROBES
        }
        // A bare imports target re-enters with isConfigLookup=false. Its JSON
        // mask therefore admits only an already-written .json extension.
        ExtensionProbePass::JsonModule
            if matches!(written_extension, Some(ModuleExtension::Json)) =>
        {
            JSON_CONFIG_PROBES
        }
        ExtensionProbePass::JsonConfig | ExtensionProbePass::JsonModule => &JSON_CONFIG_PROBES[..0],
    }
}

fn implicit_extension_probes(pass: ExtensionProbePass) -> &'static [ExtensionProbe] {
    match pass {
        ExtensionProbePass::Empty => &JS_PROBES[..0],
        ExtensionProbePass::All => JS_PROBES,
        ExtensionProbePass::Preferred => &JS_PROBES[..3],
        ExtensionProbePass::Declaration => DECLARATION_DTS_PROBES,
        ExtensionProbePass::Fallback => &JS_PROBES[3..],
        ExtensionProbePass::JsonConfig => JSON_CONFIG_PROBES,
        ExtensionProbePass::JsonModule => &JSON_CONFIG_PROBES[..0],
    }
}

fn extension_pass_includes_declaration(pass: ExtensionProbePass) -> bool {
    matches!(
        pass,
        ExtensionProbePass::All | ExtensionProbePass::Preferred | ExtensionProbePass::Declaration
    )
}

fn extension_pass_includes_typescript(pass: ExtensionProbePass) -> bool {
    matches!(
        pass,
        ExtensionProbePass::All | ExtensionProbePass::Preferred
    )
}

fn materialize_module_extension(extension: &ModuleExtension, suffix: &str) -> ModuleExtension {
    if suffix == ".d.json.ts" {
        ModuleExtension::Arbitrary(suffix.to_owned())
    } else {
        extension.clone()
    }
}

fn package_json_target_exact_extension(
    target: &str,
    pass: ExtensionProbePass,
) -> Option<ModuleExtension> {
    let extension = recognized_module_extension(target)?;
    let supported = match extension {
        ModuleExtension::Ts
        | ModuleExtension::Tsx
        | ModuleExtension::Mts
        | ModuleExtension::Cts => extension_pass_includes_typescript(pass),
        ModuleExtension::Dts | ModuleExtension::Dmts | ModuleExtension::Dcts => {
            extension_pass_includes_typescript(pass) || extension_pass_includes_declaration(pass)
        }
        ModuleExtension::Json => matches!(pass, ExtensionProbePass::JsonConfig),
        _ => false,
    };
    supported.then_some(extension)
}

fn extension_probe_plan(
    target: &str,
    resolve_json_module: bool,
) -> Result<ExtensionProbePlan<'_>, ResolutionError> {
    let plan = if let Some(base) = target.strip_suffix(".d.cts") {
        (base, DCTS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".d.mts") {
        (base, DMTS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".d.ts") {
        (base, DTS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".cjs") {
        (base, CJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".mjs") {
        (base, MJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".jsx") {
        (base, JSX_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".js") {
        (base, JS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".tsx") {
        (base, TSX_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".ts") {
        (base, TS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".mts") {
        (base, MTS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".cts") {
        (base, CTS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".json") {
        if resolve_json_module {
            (base, JSON_PROBES, 1)
        } else {
            (base, JSON_DISABLED_PROBES, 1)
        }
    } else {
        return Err(ResolutionError::unsupported(
            "module-target-extension",
            format!("target has no supported written extension: {target}"),
        ));
    };
    Ok(plan)
}

/// `tryAddingExtensions` with the vendored `Declaration` extension bit only.
/// Package-json root entries deliberately expand to the ordinary preferred
/// pass at their call site, matching `loadNodeModuleFromDirectoryWorker`.
fn declaration_extension_probe_plan(
    target: &str,
) -> Result<ExtensionProbePlan<'_>, ResolutionError> {
    let plan = if let Some(base) = target.strip_suffix(".d.cts") {
        (base, DECLARATION_DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".d.mts") {
        (base, DECLARATION_DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".d.ts") {
        (base, DECLARATION_DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".cjs") {
        (base, DECLARATION_DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".cts") {
        (base, DECLARATION_DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".mjs") {
        (base, DECLARATION_DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".mts") {
        (base, DECLARATION_DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".jsx") {
        (base, DECLARATION_DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".tsx") {
        (base, DECLARATION_DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".js") {
        (base, DECLARATION_DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".ts") {
        (base, DECLARATION_DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".json") {
        (base, DJSON_PROBES, 1)
    } else {
        return Err(ResolutionError::unsupported(
            "module-target-extension",
            format!("target has no supported written declaration extension: {target}"),
        ));
    };
    Ok(plan)
}

/// tsc-port: extensionsToRemove @6.0.3
/// tsc-hash: c6e27d3d5107b27a56e63c94807691f97f41bcefd4ec2ca937cac9061099c118
/// tsc-span: _tsc.js:18748-18762
/// tsc-port: tryGetExtensionFromPath2 @6.0.3
/// tsc-hash: e55cb27a72b2c3a1c1166eea4a6e580868ebebc996c692e2a07ae6a82aa17da2
/// tsc-span: _tsc.js:18824-18826
fn module_suffix_extension(path: &str) -> &'static str {
    [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ]
    .into_iter()
    .find(|extension| path.len() > extension.len() && path.ends_with(extension))
    .unwrap_or("")
}

/// tsrs-native: the recognized-extension projection consumed by the
/// typesVersions exact-substitution probe.
fn recognized_module_extension(path: &str) -> Option<ModuleExtension> {
    Some(match module_suffix_extension(path) {
        ".d.ts" => ModuleExtension::Dts,
        ".d.mts" => ModuleExtension::Dmts,
        ".d.cts" => ModuleExtension::Dcts,
        ".mjs" => ModuleExtension::Mjs,
        ".mts" => ModuleExtension::Mts,
        ".cjs" => ModuleExtension::Cjs,
        ".cts" => ModuleExtension::Cts,
        ".ts" => ModuleExtension::Ts,
        ".js" => ModuleExtension::Js,
        ".tsx" => ModuleExtension::Tsx,
        ".jsx" => ModuleExtension::Jsx,
        ".json" => ModuleExtension::Json,
        "" => return None,
        _ => unreachable!("module suffix extension table is closed"),
    })
}

/// ECMAScript own-property ordering used by JSON.parse results: canonical
/// array-index keys first in ascending order, then other strings in source
/// insertion order. serde_json's `preserve_order` covers only the second
/// group, so version/condition objects need this projection explicitly.
fn js_own_property_entries(object: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut indices = Vec::new();
    let mut strings = Vec::new();
    for (key, value) in object {
        let Some(key) = decode_user_object_key(key) else {
            continue;
        };
        if let Some(index) = js_array_index(key) {
            indices.push((index, key, value));
        } else {
            strings.push((key, value));
        }
    }
    indices.sort_by_key(|(index, _, _)| *index);
    indices
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .chain(strings)
        .collect()
}

fn js_json_object_entries(value: &Value) -> Option<Vec<(String, &Value)>> {
    match value {
        Value::Object(object) => Some(
            js_own_property_entries(object)
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        ),
        Value::Array(array) => Some(
            array
                .iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value))
                .collect(),
        ),
        _ => None,
    }
}

fn js_array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    let index = key.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

/// Project the value passed to TypeScript's generic `forEach` helper in
/// `tryLoadModuleUsingPaths`.
///
/// tsc-port: forEach @6.0.3
/// tsc-hash: 8efa7fabfe639253b0004be7e4cf536dd28e0425554f481edec429d0a7508ca7
/// tsc-span: _tsc.js:29-39
fn try_for_each_types_versions_substitution<T>(
    targets: &Value,
    pattern: &str,
    mut callback: impl FnMut(&Value) -> Result<Option<T>, ResolutionError>,
) -> Result<Option<T>, ResolutionError> {
    match targets {
        // JavaScript string indexing observes UTF-16 code units. Rust strings
        // cannot retain an unpaired surrogate, while Node's filesystem path
        // conversion replaces one as well, so use the same lossy scalar here.
        Value::String(target) => {
            for unit in target.encode_utf16() {
                let substitution = Value::String(String::from_utf16_lossy(&[unit]));
                if let Some(result) = callback(&substitution)? {
                    return Ok(Some(result));
                }
            }
            Ok(None)
        }
        Value::Array(targets) => {
            for target in targets {
                if let Some(result) = callback(target)? {
                    return Ok(Some(result));
                }
            }
            Ok(None)
        }
        Value::Object(targets) => {
            let length = js_json_object_array_like_length(targets)?;
            let mut index = 0_usize;
            while (index as f64) < length {
                let substitution =
                    json_object_get(targets, &index.to_string()).ok_or_else(|| {
                        ResolutionError::invalid_data(format!(
                            "typesVersions mapping {pattern:?} is missing array-like index {index}"
                        ))
                    })?;
                if let Some(result) = callback(substitution)? {
                    return Ok(Some(result));
                }
                index = index.checked_add(1).ok_or_else(|| {
                    ResolutionError::resource_limit(format!(
                        "typesVersions mapping {pattern:?} has an unbounded array-like length"
                    ))
                })?;
            }
            Ok(None)
        }
        // `forEach(null, ...)` reads `null.length` and throws. Other JSON
        // primitives expose no `length`, so the JavaScript loop executes zero
        // times and the selected mapping owns a miss.
        Value::Null => Err(ResolutionError::invalid_data(format!(
            "typesVersions mapping {pattern:?} is null"
        ))),
        Value::Bool(_) | Value::Number(_) => Ok(None),
    }
}

/// Apply the callback-local JavaScript coercions from
/// `tryLoadModuleUsingPaths`. A nonempty wildcard capture goes through
/// `String.prototype.replace.call`, while an exact match passes its raw value
/// to `combinePaths`: false, zero, and an empty string are skipped there, but
/// other truthy non-string JSON values fail during slash normalization.
fn project_types_versions_substitution(
    substitution: &Value,
    capture: &str,
    pattern: &str,
) -> Result<(String, Option<ModuleExtension>), ResolutionError> {
    let invalid_target = || {
        ResolutionError::invalid_data(format!(
            "typesVersions mapping {pattern:?} contains a target that cannot be used as a path"
        ))
    };
    if !capture.is_empty() {
        let target = js_json_to_string(substitution)?;
        let expanded = js_replace_first_star(target.as_ref(), capture)?;
        let extension = recognized_types_versions_raw_extension(substitution, pattern)?;
        return Ok((expanded, extension));
    }

    match substitution {
        Value::String(target) => Ok((target.clone(), recognized_module_extension(target))),
        Value::Bool(false) => Ok((String::new(), None)),
        Value::Number(target) if json_number_as_f64(target).is_some_and(|target| target == 0.0) => {
            Ok((String::new(), None))
        }
        Value::Null | Value::Bool(true) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            Err(invalid_target())
        }
    }
}

fn recognized_types_versions_raw_extension(
    substitution: &Value,
    pattern: &str,
) -> Result<Option<ModuleExtension>, ResolutionError> {
    let invalid_target = || {
        ResolutionError::invalid_data(format!(
            "typesVersions mapping {pattern:?} contains a target with invalid path operations"
        ))
    };
    match substitution {
        Value::String(substitution) => Ok(recognized_module_extension(substitution)),
        Value::Null => Err(invalid_target()),
        Value::Bool(_) | Value::Number(_) => Ok(None),
        Value::Object(substitution) => {
            let length = js_json_object_array_like_length(substitution)?;
            for (text, extension) in [
                (".d.ts", ModuleExtension::Dts),
                (".d.mts", ModuleExtension::Dmts),
                (".d.cts", ModuleExtension::Dcts),
                (".mjs", ModuleExtension::Mjs),
                (".mts", ModuleExtension::Mts),
                (".cjs", ModuleExtension::Cjs),
                (".cts", ModuleExtension::Cts),
                (".ts", ModuleExtension::Ts),
                (".js", ModuleExtension::Js),
                (".tsx", ModuleExtension::Tsx),
                (".jsx", ModuleExtension::Jsx),
                (".json", ModuleExtension::Json),
            ] {
                if length.is_nan() || length <= text.len() as f64 {
                    continue;
                }
                if !js_json_object_inherits_array_method(substitution, "indexOf") {
                    return Err(invalid_target());
                }
                let expected = length - text.len() as f64;
                if js_json_object_array_index_of_starts_with_match(substitution, text, expected)? {
                    return Ok(Some(extension));
                }
            }
            Ok(None)
        }
        Value::Array(substitution) => {
            for (text, extension) in [
                (".d.ts", ModuleExtension::Dts),
                (".d.mts", ModuleExtension::Dmts),
                (".d.cts", ModuleExtension::Dcts),
                (".mjs", ModuleExtension::Mjs),
                (".mts", ModuleExtension::Mts),
                (".cjs", ModuleExtension::Cjs),
                (".cts", ModuleExtension::Cts),
                (".ts", ModuleExtension::Ts),
                (".js", ModuleExtension::Js),
                (".tsx", ModuleExtension::Tsx),
                (".jsx", ModuleExtension::Jsx),
                (".json", ModuleExtension::Json),
            ] {
                if substitution.len() <= text.len() {
                    continue;
                }
                let expected = substitution.len() - text.len();
                if substitution
                    .iter()
                    .skip(expected)
                    .position(|value| value.as_str() == Some(text))
                    == Some(0)
                {
                    return Ok(Some(extension));
                }
            }
            Ok(None)
        }
    }
}

#[derive(Clone, Copy)]
enum JsJsonObjectToStringMethod {
    Object,
    ArrayJoin,
}

fn js_json_object_to_string_method(
    object: &Map<String, Value>,
) -> Result<JsJsonObjectToStringMethod, ResolutionError> {
    js_json_object_to_string_method_worker(object, false)
}

fn js_json_object_to_string_method_worker(
    object: &Map<String, Value>,
    inherited_join_is_shadowed: bool,
) -> Result<JsJsonObjectToStringMethod, ResolutionError> {
    // JSON cannot carry a function. Any own `toString` therefore shadows the
    // inherited callable with a non-callable value and JavaScript throws.
    if json_object_own_get(object, "toString").is_some() {
        return Err(ResolutionError::invalid_data(
            "JSON object shadows its inherited JavaScript toString method",
        ));
    }
    let inherited_join_is_shadowed =
        inherited_join_is_shadowed || json_object_own_get(object, "join").is_some();
    match jsonc_prototype(object) {
        Some(Value::Object(prototype)) => {
            js_json_object_to_string_method_worker(prototype, inherited_join_is_shadowed)
        }
        // `convertToJson` assigns through `result["__proto__"]`. An array
        // value consequently lends Array.prototype.toString to the result;
        // that method invokes a callable `this.join`, or falls back to
        // Object.prototype.toString when a JSON value shadows `join`.
        Some(Value::Array(_)) if inherited_join_is_shadowed => {
            Ok(JsJsonObjectToStringMethod::Object)
        }
        Some(Value::Array(_)) => Ok(JsJsonObjectToStringMethod::ArrayJoin),
        Some(Value::Null) => Err(ResolutionError::invalid_data(
            "JSON object has no inherited JavaScript toString method",
        )),
        None => Ok(JsJsonObjectToStringMethod::Object),
        Some(_) => {
            unreachable!("the JSONC converter stores only object, array, or null prototypes")
        }
    }
}

fn js_json_object_inherits_array_method(object: &Map<String, Value>, method: &str) -> bool {
    if json_object_own_get(object, method).is_some() {
        return false;
    }
    match jsonc_prototype(object) {
        Some(Value::Object(prototype)) => js_json_object_inherits_array_method(prototype, method),
        Some(Value::Array(_)) => true,
        Some(Value::Null) | None => false,
        Some(_) => {
            unreachable!("the JSONC converter stores only object, array, or null prototypes")
        }
    }
}

fn js_json_object_array_index_of_starts_with_match(
    object: &Map<String, Value>,
    needle: &str,
    expected: f64,
) -> Result<bool, ResolutionError> {
    // `tryGetExtensionFromPath2` compares Array#indexOf's integer return
    // value with the raw subtraction result. A fractional or infinite value
    // can therefore never match, though the inherited method lookup above is
    // still observable and must already have succeeded.
    if !expected.is_finite() || expected < 0.0 || expected.fract() != 0.0 {
        return Ok(false);
    }
    let element_count = js_array_like_to_length(js_json_object_array_like_length(object)?);
    if expected >= element_count as f64 {
        return Ok(false);
    }
    let index = format!("{expected:.0}");
    Ok(json_object_get(object, &index).and_then(Value::as_str) == Some(needle))
}

fn js_array_like_to_length(length: f64) -> u64 {
    if length.is_nan() || length <= 0.0 {
        0
    } else if !length.is_finite() {
        9_007_199_254_740_991
    } else {
        length.floor().min(9_007_199_254_740_991.0) as u64
    }
}

fn js_json_object_array_like_length(object: &Map<String, Value>) -> Result<f64, ResolutionError> {
    if let Some(length) = json_object_own_get(object, "length") {
        return js_json_to_number(length);
    }
    match jsonc_prototype(object) {
        Some(Value::Object(prototype)) => js_json_object_array_like_length(prototype),
        Some(Value::Array(prototype)) => Ok(prototype.len() as f64),
        Some(Value::Null) | None => Ok(f64::NAN),
        Some(_) => {
            unreachable!("the JSONC converter stores only object, array, or null prototypes")
        }
    }
}

fn js_json_to_string(value: &Value) -> Result<Cow<'_, str>, ResolutionError> {
    match value {
        Value::Null => Err(ResolutionError::invalid_data(
            "null cannot be used as a JavaScript string receiver",
        )),
        Value::Bool(value) => Ok(Cow::Borrowed(if *value { "true" } else { "false" })),
        Value::Number(value) => json_number_as_f64(value)
            .map(js_number_to_string)
            .map(Cow::Owned)
            .ok_or_else(|| {
                ResolutionError::invalid_data(
                    "JSON number cannot be represented as a JavaScript number",
                )
            }),
        Value::String(value) => Ok(Cow::Borrowed(value)),
        Value::Array(values) => js_json_array_to_string(values).map(Cow::Owned),
        Value::Object(value) => match js_json_object_to_string_method(value)? {
            JsJsonObjectToStringMethod::Object => Ok(Cow::Borrowed("[object Object]")),
            JsJsonObjectToStringMethod::ArrayJoin => {
                js_json_object_array_join_to_string(value).map(Cow::Owned)
            }
        },
    }
}

fn js_json_value_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(value) => {
            json_number_as_f64(value).is_some_and(|value| value != 0.0 && !value.is_nan())
        }
        Value::String(value) => !value.is_empty(),
        Value::Bool(true) | Value::Array(_) | Value::Object(_) => true,
    }
}

/// JavaScript Number coercion for JSON values used as an array-like `length`.
fn js_json_to_number(value: &Value) -> Result<f64, ResolutionError> {
    match value {
        Value::Null => Ok(0.0),
        Value::Bool(value) => Ok(f64::from(u8::from(*value))),
        Value::Number(value) => json_number_as_f64(value).ok_or_else(|| {
            ResolutionError::invalid_data(
                "JSON number cannot be represented as a JavaScript number",
            )
        }),
        Value::String(value) => Ok(js_number_from_text(value)),
        Value::Array(values) => Ok(js_number_from_text(&js_json_array_to_string(values)?)),
        Value::Object(value) => match js_json_object_to_string_method(value)? {
            JsJsonObjectToStringMethod::Object => Ok(f64::NAN),
            JsJsonObjectToStringMethod::ArrayJoin => Ok(js_number_from_text(
                &js_json_object_array_join_to_string(value)?,
            )),
        },
    }
}

fn js_number_from_text(value: &str) -> f64 {
    let value = value.trim_matches(is_ecmascript_string_numeric_whitespace);
    if value.is_empty() {
        return 0.0;
    }
    match value {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefixes, radix) in [(["0x", "0X"], 16_u32), (["0o", "0O"], 8), (["0b", "0B"], 2)] {
        if let Some(digits) = prefixes
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
        {
            if digits.is_empty() {
                return f64::NAN;
            }
            let mut number = 0.0_f64;
            for digit in digits.chars() {
                let Some(digit) = digit.to_digit(radix) else {
                    return f64::NAN;
                };
                number = number.mul_add(f64::from(radix), f64::from(digit));
            }
            return number;
        }
    }
    if !is_ecmascript_decimal_number(value) {
        return f64::NAN;
    }
    value
        .strip_prefix('+')
        .unwrap_or(value)
        .parse()
        .unwrap_or(f64::NAN)
}

fn is_ecmascript_string_numeric_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn is_ecmascript_decimal_number(value: &str) -> bool {
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    if unsigned.is_empty() {
        return false;
    }
    let mut exponent_parts = unsigned.split(['e', 'E']);
    let mantissa = exponent_parts
        .next()
        .expect("split always yields a mantissa");
    if let Some(exponent) = exponent_parts.next() {
        if exponent_parts.next().is_some() {
            return false;
        }
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !exponent.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
    }
    let mut decimal_parts = mantissa.split('.');
    let whole = decimal_parts
        .next()
        .expect("split always yields a whole part");
    let fraction = decimal_parts.next();
    if decimal_parts.next().is_some() {
        return false;
    }
    let whole_is_digits = whole.bytes().all(|byte| byte.is_ascii_digit());
    let fraction_is_digits =
        fraction.is_none_or(|fraction| fraction.bytes().all(|byte| byte.is_ascii_digit()));
    whole_is_digits
        && fraction_is_digits
        && (!whole.is_empty() || fraction.is_some_and(|fraction| !fraction.is_empty()))
}

fn js_json_array_to_string(values: &[Value]) -> Result<String, ResolutionError> {
    let output_length = js_json_array_string_length(values)?;
    if output_length > MAX_JS_JSON_COERCION_OUTPUT_BUDGET {
        return Err(ResolutionError::resource_limit(format!(
            "JavaScript array string coercion would produce {output_length} bytes (budget {MAX_JS_JSON_COERCION_OUTPUT_BUDGET})"
        )));
    }
    let mut result = String::new();
    result.try_reserve_exact(output_length).map_err(|error| {
        ResolutionError::resource_limit(format!(
            "could not reserve {output_length} bytes for JavaScript array string coercion: {error}"
        ))
    })?;
    append_js_json_array_string(values, &mut result)?;
    debug_assert_eq!(result.len(), output_length);
    Ok(result)
}

fn js_json_array_string_length(values: &[Value]) -> Result<usize, ResolutionError> {
    let mut length = values.len().saturating_sub(1);
    for value in values {
        let value_length = js_json_join_element_string_length(value)?;
        length = length.checked_add(value_length).ok_or_else(|| {
            ResolutionError::resource_limit(
                "JavaScript array string coercion output length overflowed usize",
            )
        })?;
    }
    Ok(length)
}

fn append_js_json_array_string(
    values: &[Value],
    result: &mut String,
) -> Result<(), ResolutionError> {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            result.push(',');
        }
        append_js_json_join_element(value, result)?;
    }
    Ok(())
}

fn append_js_json_join_element(value: &Value, result: &mut String) -> Result<(), ResolutionError> {
    match value {
        Value::Null => {}
        Value::Bool(value) => result.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => {
            let value = json_number_as_f64(value).ok_or_else(|| {
                ResolutionError::invalid_data(
                    "JSON number cannot be represented as a JavaScript number",
                )
            })?;
            result.push_str(&js_number_to_string(value));
        }
        Value::String(value) => result.push_str(value),
        Value::Array(values) => append_js_json_array_string(values, result)?,
        Value::Object(value) => append_js_json_object_string(value, result)?,
    }
    Ok(())
}

fn js_json_join_element_string_length(value: &Value) -> Result<usize, ResolutionError> {
    match value {
        // Array#join renders null and an absent/undefined element as empty.
        Value::Null => Ok(0),
        Value::Bool(true) => Ok(4),
        Value::Bool(false) => Ok(5),
        Value::Number(value) => json_number_as_f64(value)
            .map(js_number_to_string)
            .map(|value| value.len())
            .ok_or_else(|| {
                ResolutionError::invalid_data(
                    "JSON number cannot be represented as a JavaScript number",
                )
            }),
        Value::String(value) => Ok(value.len()),
        Value::Array(values) => js_json_array_string_length(values),
        Value::Object(value) => match js_json_object_to_string_method(value)? {
            JsJsonObjectToStringMethod::Object => Ok("[object Object]".len()),
            JsJsonObjectToStringMethod::ArrayJoin => js_json_object_array_join_string_length(value),
        },
    }
}

fn append_js_json_object_string(
    object: &Map<String, Value>,
    result: &mut String,
) -> Result<(), ResolutionError> {
    match js_json_object_to_string_method(object)? {
        JsJsonObjectToStringMethod::Object => result.push_str("[object Object]"),
        JsJsonObjectToStringMethod::ArrayJoin => {
            append_js_json_object_array_join_string(object, result)?;
        }
    }
    Ok(())
}

fn js_json_object_array_join_to_string(
    object: &Map<String, Value>,
) -> Result<String, ResolutionError> {
    let (element_count, elements) = js_json_object_array_join_projection(object)?;
    let output_length = js_json_object_array_join_projection_length(element_count, &elements)?;
    if output_length > MAX_JS_JSON_COERCION_OUTPUT_BUDGET {
        return Err(ResolutionError::resource_limit(format!(
            "JavaScript generic array string coercion would produce {output_length} bytes (budget {MAX_JS_JSON_COERCION_OUTPUT_BUDGET})"
        )));
    }
    let mut result = String::new();
    result.try_reserve_exact(output_length).map_err(|error| {
        ResolutionError::resource_limit(format!(
            "could not reserve {output_length} bytes for JavaScript generic array string coercion: {error}"
        ))
    })?;
    append_js_json_object_array_join_projection(element_count, &elements, &mut result)?;
    debug_assert_eq!(result.len(), output_length);
    Ok(result)
}

fn js_json_object_array_join_string_length(
    object: &Map<String, Value>,
) -> Result<usize, ResolutionError> {
    let (element_count, elements) = js_json_object_array_join_projection(object)?;
    js_json_object_array_join_projection_length(element_count, &elements)
}

fn js_json_object_array_join_projection_length(
    element_count: usize,
    elements: &BTreeMap<usize, &Value>,
) -> Result<usize, ResolutionError> {
    let mut length = element_count.saturating_sub(1);
    if length > MAX_JS_JSON_COERCION_OUTPUT_BUDGET {
        return Err(ResolutionError::resource_limit(format!(
            "JavaScript generic array string coercion requires {length} separator bytes (budget {MAX_JS_JSON_COERCION_OUTPUT_BUDGET})"
        )));
    }
    for value in elements.values() {
        length = length
            .checked_add(js_json_join_element_string_length(value)?)
            .ok_or_else(|| {
                ResolutionError::resource_limit(
                    "JavaScript generic array string coercion output length overflowed usize",
                )
            })?;
        if length > MAX_JS_JSON_COERCION_OUTPUT_BUDGET {
            return Err(ResolutionError::resource_limit(format!(
                "JavaScript generic array string coercion would exceed the {MAX_JS_JSON_COERCION_OUTPUT_BUDGET}-byte budget"
            )));
        }
    }
    Ok(length)
}

fn append_js_json_object_array_join_string(
    object: &Map<String, Value>,
    result: &mut String,
) -> Result<(), ResolutionError> {
    let (element_count, elements) = js_json_object_array_join_projection(object)?;
    append_js_json_object_array_join_projection(element_count, &elements, result)
}

fn js_json_object_array_join_projection(
    object: &Map<String, Value>,
) -> Result<(usize, BTreeMap<usize, &Value>), ResolutionError> {
    let element_count = js_array_join_element_count(js_json_object_array_like_length(object)?)?;
    let mut elements = BTreeMap::new();
    let mut current = object;
    loop {
        for (key, value) in current {
            let Some(index) = js_generic_array_property_index(key) else {
                continue;
            };
            if index < element_count {
                elements.entry(index).or_insert(value);
            }
        }
        match jsonc_prototype(current) {
            Some(Value::Object(prototype)) => current = prototype,
            Some(Value::Array(prototype)) => {
                for (index, value) in prototype.iter().take(element_count).enumerate() {
                    elements.entry(index).or_insert(value);
                }
                break;
            }
            Some(Value::Null) | None => break,
            Some(_) => {
                unreachable!("the JSONC converter stores only object, array, or null prototypes")
            }
        }
    }
    Ok((element_count, elements))
}

fn js_generic_array_property_index(key: &str) -> Option<usize> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    let index = key.parse::<usize>().ok()?;
    (index.to_string() == key).then_some(index)
}

fn append_js_json_object_array_join_projection(
    element_count: usize,
    elements: &BTreeMap<usize, &Value>,
    result: &mut String,
) -> Result<(), ResolutionError> {
    let mut rendered_elements = 0usize;
    for (&index, value) in elements {
        let commas = if rendered_elements == 0 {
            index
        } else {
            index - rendered_elements + 1
        };
        append_commas(result, commas);
        append_js_json_join_element(value, result)?;
        rendered_elements = index + 1;
    }
    let trailing_commas = if rendered_elements == 0 {
        element_count.saturating_sub(1)
    } else {
        element_count - rendered_elements
    };
    append_commas(result, trailing_commas);
    Ok(())
}

fn append_commas(result: &mut String, mut count: usize) {
    const COMMA_BLOCK: &str = ",,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,";
    while count >= COMMA_BLOCK.len() {
        result.push_str(COMMA_BLOCK);
        count -= COMMA_BLOCK.len();
    }
    result.push_str(&COMMA_BLOCK[..count]);
}

fn js_array_join_element_count(length: f64) -> Result<usize, ResolutionError> {
    if length.is_nan() || length <= 0.0 {
        return Ok(0);
    }
    if !length.is_finite() {
        return Err(ResolutionError::resource_limit(
            "JavaScript generic array string coercion has an infinite length",
        ));
    }
    let length = length.floor().min(9_007_199_254_740_991.0);
    if length > (MAX_JS_JSON_COERCION_OUTPUT_BUDGET + 1) as f64 {
        return Err(ResolutionError::resource_limit(format!(
            "JavaScript generic array string coercion length {length} exceeds the output budget"
        )));
    }
    Ok(length as usize)
}

/// JavaScript replacement-string semantics for `replaceFirstStar` and the
/// package-map `/\*/g` replacement. There are no capture groups, so only the
/// four context-independent/context tokens and `$$` are active; `$1` and
/// `$<name>` remain literal.
fn js_replace_first_star(target: &str, replacement: &str) -> Result<String, ResolutionError> {
    js_replace_stars(target, replacement, false)
}

fn js_replace_all_stars(target: &str, replacement: &str) -> Result<String, ResolutionError> {
    js_replace_stars(target, replacement, true)
}

fn js_replace_stars(
    target: &str,
    replacement: &str,
    replace_all: bool,
) -> Result<String, ResolutionError> {
    let output_length = js_star_replacement_output_length(target, replacement, replace_all)
        .ok_or_else(|| {
            ResolutionError::resource_limit(
                "JavaScript star replacement output length overflowed usize",
            )
        })?;
    let input_length = target.len().checked_add(replacement.len()).ok_or_else(|| {
        ResolutionError::resource_limit("JavaScript star replacement input length overflowed usize")
    })?;
    let output_budget = input_length
        .saturating_mul(JS_REPLACEMENT_INPUT_MULTIPLIER)
        .clamp(
            MIN_JS_REPLACEMENT_OUTPUT_BUDGET,
            MAX_JS_REPLACEMENT_OUTPUT_BUDGET,
        )
        .max(target.len());
    if output_length > output_budget {
        return Err(ResolutionError::resource_limit(format!(
            "JavaScript star replacement would expand {input_length} input bytes to {output_length} bytes (budget {output_budget})"
        )));
    }
    let mut result = String::new();
    result.try_reserve_exact(output_length).map_err(|error| {
        ResolutionError::resource_limit(format!(
            "could not reserve {output_length} bytes for JavaScript star replacement: {error}"
        ))
    })?;
    let mut search_start = 0;
    let mut replaced = false;
    while let Some(relative_star) = target[search_start..].find('*') {
        let star = search_start + relative_star;
        result.push_str(&target[search_start..star]);
        append_js_star_replacement(
            &mut result,
            replacement,
            &target[..star],
            &target[star + 1..],
        );
        search_start = star + 1;
        replaced = true;
        if !replace_all {
            break;
        }
    }
    if !replaced {
        result.push_str(target);
        return Ok(result);
    }
    result.push_str(&target[search_start..]);
    debug_assert_eq!(result.len(), output_length);
    Ok(result)
}

fn js_star_replacement_output_length(
    target: &str,
    replacement: &str,
    replace_all: bool,
) -> Option<usize> {
    let replacement = js_star_replacement_length_summary(replacement)?;
    let mut output_length = 0_usize;
    let mut search_start = 0;
    let mut replaced = false;
    while let Some(relative_star) = target[search_start..].find('*') {
        let star = search_start + relative_star;
        output_length = output_length.checked_add(star - search_start)?;
        output_length = output_length
            .checked_add(replacement.expanded_length(star, target.len() - star - 1)?)?;
        search_start = star + 1;
        replaced = true;
        if !replace_all {
            break;
        }
    }
    if !replaced {
        return Some(target.len());
    }
    output_length.checked_add(target.len() - search_start)
}

#[derive(Clone, Copy)]
struct JsStarReplacementLengthSummary {
    fixed_length: usize,
    prefix_tokens: usize,
    suffix_tokens: usize,
}

impl JsStarReplacementLengthSummary {
    fn expanded_length(self, prefix_length: usize, suffix_length: usize) -> Option<usize> {
        self.fixed_length
            .checked_add(self.prefix_tokens.checked_mul(prefix_length)?)?
            .checked_add(self.suffix_tokens.checked_mul(suffix_length)?)
    }
}

fn js_star_replacement_length_summary(replacement: &str) -> Option<JsStarReplacementLengthSummary> {
    let mut summary = JsStarReplacementLengthSummary {
        fixed_length: 0,
        prefix_tokens: 0,
        suffix_tokens: 0,
    };
    let mut cursor = 0;
    while let Some(relative_dollar) = replacement[cursor..].find('$') {
        let dollar = cursor + relative_dollar;
        summary.fixed_length = summary.fixed_length.checked_add(dollar - cursor)?;
        let Some(token) = replacement.as_bytes().get(dollar + 1).copied() else {
            summary.fixed_length = summary.fixed_length.checked_add(1)?;
            cursor = dollar + 1;
            break;
        };
        match token {
            b'$' | b'&' => {
                summary.fixed_length = summary.fixed_length.checked_add(1)?;
            }
            b'`' => summary.prefix_tokens = summary.prefix_tokens.checked_add(1)?,
            b'\'' => summary.suffix_tokens = summary.suffix_tokens.checked_add(1)?,
            _ => {
                summary.fixed_length = summary.fixed_length.checked_add(1)?;
                cursor = dollar + 1;
                continue;
            }
        }
        cursor = dollar + 2;
    }
    summary.fixed_length = summary
        .fixed_length
        .checked_add(replacement.len() - cursor)?;
    Some(summary)
}

fn append_js_star_replacement(result: &mut String, replacement: &str, prefix: &str, suffix: &str) {
    let mut cursor = 0;
    while let Some(relative_dollar) = replacement[cursor..].find('$') {
        let dollar = cursor + relative_dollar;
        result.push_str(&replacement[cursor..dollar]);
        let Some(token) = replacement.as_bytes().get(dollar + 1).copied() else {
            result.push('$');
            cursor = dollar + 1;
            break;
        };
        match token {
            b'$' => result.push('$'),
            b'&' => result.push('*'),
            b'`' => result.push_str(prefix),
            b'\'' => result.push_str(suffix),
            _ => {
                result.push('$');
                cursor = dollar + 1;
                continue;
            }
        }
        cursor = dollar + 2;
    }
    result.push_str(&replacement[cursor..]);
}

fn select_types_versions_mapping<'a, 'b>(
    table: &'a Value,
    request: &'b str,
) -> Option<(String, &'b str, &'a Value)> {
    let entries = js_json_object_entries(table)?;
    if let Some((key, targets)) = entries.iter().find(|(key, _)| key == request) {
        return Some((key.clone(), "", *targets));
    }
    let mut best = None;
    for (pattern, targets) in entries {
        if !has_one_asterisk(&pattern) {
            continue;
        }
        let star = pattern
            .find('*')
            .expect("typesVersions pattern was filtered to one asterisk");
        let prefix = &pattern[..star];
        let suffix = &pattern[star + 1..];
        if request.starts_with(prefix)
            && request.ends_with(suffix)
            && request.len() >= prefix.len() + suffix.len()
        {
            // findBestPatternMatch updates only for a strictly longer prefix;
            // equal-prefix matches retain package.json insertion order.
            if best
                .as_ref()
                .is_some_and(|(_, _, _, longest_prefix)| *longest_prefix >= prefix.len())
            {
                continue;
            }
            let capture = &request[prefix.len()..request.len() - suffix.len()];
            let prefix_len = prefix.len();
            best = Some((pattern, capture, targets, prefix_len));
        }
    }
    best.map(|(pattern, capture, targets, _)| (pattern, capture, targets))
}

fn mangle_scoped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(scoped) if scoped.contains('/') => scoped.replacen('/', "__", 1),
        None => package_name.to_owned(),
        Some(_) => package_name.to_owned(),
    }
}

fn package_root_for_request(node_modules: &str, request: &PackageRequest<'_>) -> String {
    retain_request_trailing_separator(
        join_normalized(node_modules, &request.package_name.replace('\\', "/")),
        request.trailing_separator,
    )
}

fn types_package_root_for_request(
    node_modules_at_types: &str,
    request: &PackageRequest<'_>,
) -> String {
    retain_request_trailing_separator(
        join_normalized(
            node_modules_at_types,
            &mangle_scoped_package_name(request.package_name).replace('\\', "/"),
        ),
        request.trailing_separator,
    )
}

fn retain_request_trailing_separator(mut path: String, trailing_separator: bool) -> String {
    if trailing_separator && !path.ends_with('/') {
        path.push('/');
    }
    path
}

/// tsc-port: comparePatternKeys @6.0.3
/// tsc-hash: fb7ad1b471b8e090c418cfe7c2c9a7aec4c2988b831bb272387a83f1dac8387c
/// tsc-span: _tsc.js:41587-41598
fn compare_pattern_keys(left: &str, right: &str) -> Ordering {
    let left_star = left.find('*');
    let right_star = right.find('*');
    let left_base_length = left_star.map_or(left.len(), |index| index + 1);
    let right_base_length = right_star.map_or(right.len(), |index| index + 1);
    match right_base_length.cmp(&left_base_length) {
        Ordering::Equal => {}
        order => return order,
    }
    match (left_star, right_star) {
        (None, Some(_)) => return Ordering::Greater,
        (Some(_), None) => return Ordering::Less,
        _ => {}
    }
    right.len().cmp(&left.len())
}

fn has_one_asterisk(key: &str) -> bool {
    key.find('*')
        .is_some_and(|first| key.rfind('*') == Some(first))
}

fn select_package_map_target<'a>(
    table: &'a Map<String, Value>,
    specifier: &str,
    exports_pattern_trailers: bool,
) -> Option<SelectedPackageMapTarget<'a>> {
    if !specifier.ends_with('/') && !specifier.contains('*') {
        if let Some(target) = json_object_own_get(table, specifier) {
            return Some(SelectedPackageMapTarget {
                target,
                subpath: String::new(),
                pattern: false,
            });
        }
    }

    let mut expanding_keys = table
        .keys()
        .filter_map(|key| decode_user_object_key(key))
        .filter(|key| has_one_asterisk(key) || key.ends_with('/'))
        .collect::<Vec<_>>();
    expanding_keys.sort_by(|left, right| compare_pattern_keys(left, right));
    for key in expanding_keys {
        let target =
            json_object_own_get(table, key).expect("expanding key was collected from this table");
        if let Some(star) = key.find('*') {
            let prefix = &key[..star];
            let suffix = &key[star + 1..];
            if exports_pattern_trailers
                && !suffix.is_empty()
                && specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
            {
                // JavaScript String#substring swaps its bounds when the
                // suffix overlaps the prefix. TypeScript relies on that exact
                // behavior instead of rejecting the key.
                let start = prefix.len();
                let end = specifier.len().saturating_sub(suffix.len());
                let (start, end) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                return Some(SelectedPackageMapTarget {
                    target,
                    subpath: specifier[start..end].to_owned(),
                    pattern: true,
                });
            }
            if suffix.is_empty() && specifier.starts_with(prefix) {
                return Some(SelectedPackageMapTarget {
                    target,
                    subpath: specifier[prefix.len()..].to_owned(),
                    pattern: true,
                });
            }
        }
        // The third upstream branch treats the complete key, including any
        // literal `*`, as a directory prefix when neither pattern won.
        if let Some(subpath) = specifier.strip_prefix(key) {
            return Some(SelectedPackageMapTarget {
                target,
                subpath: subpath.to_owned(),
                pattern: false,
            });
        }
    }
    None
}

fn expand_export_target(
    package_root: &str,
    target: &str,
    subpath: &str,
    pattern: bool,
) -> Result<Option<String>, ResolutionError> {
    let Some(target) = target.strip_prefix("./") else {
        return Ok(None);
    };
    if target.contains('\0')
        || subpath.contains('\0')
        || contains_forbidden_package_segment(target)
        || contains_forbidden_package_segment(subpath)
    {
        return Ok(None);
    }
    let resolved_target = join_normalized(package_root, target);
    Ok(Some(if pattern {
        js_replace_all_stars(&resolved_target, subpath)?
    } else {
        format!("{resolved_target}{subpath}")
    }))
}

fn expand_imports_bare_target(
    target: &str,
    subpath: &str,
    pattern: bool,
) -> Result<Option<String>, ResolutionError> {
    if target.starts_with("../")
        || is_rooted_disk_path(target)
        || target.contains('\0')
        || subpath.contains('\0')
    {
        return Ok(None);
    }
    Ok(Some(if pattern {
        js_replace_all_stars(target, subpath)?
    } else {
        format!("{target}{subpath}")
    }))
}

/// tsc-port: isRootedDiskPath @6.0.3
/// tsc-hash: 8b2dd2a22675acbfa2df9f0725c62de7f4d94457736242b226b4785ab69834a9
/// tsc-span: _tsc.js:5304-5306
///
/// tsc-port: getEncodedRootLength @6.0.3
/// tsc-hash: 538f15da938ce9f7bcd6aa26f945cffe1cadbc12095e8666dab9ca62320a13e2
/// tsc-span: _tsc.js:5349-5378
fn is_rooted_disk_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    matches!(bytes.first(), Some(b'/' | b'\\'))
        || (bytes.first().is_some_and(u8::is_ascii_alphabetic)
            && bytes.get(1) == Some(&b':')
            && (bytes.len() == 2 || matches!(bytes.get(2), Some(b'/' | b'\\'))))
}

fn contains_forbidden_package_segment(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|part| matches!(part, "." | ".." | "node_modules"))
}

fn path_is_within(path: &str, directory: &str) -> bool {
    path_relative_to_directory(path, directory).is_some()
}

fn path_relative_to_directory<'a>(path: &'a str, directory: &str) -> Option<&'a str> {
    let remainder = if path == directory {
        return Some("");
    } else if directory == "/" {
        path.strip_prefix('/')?
    } else if is_drive_root(directory) {
        let path_bytes = path.as_bytes();
        let directory_bytes = directory.as_bytes();
        if path_bytes.len() < 3
            || !path_bytes[0].eq_ignore_ascii_case(&directory_bytes[0])
            || path_bytes.get(1..3) != directory_bytes.get(1..3)
        {
            return None;
        }
        path.get(3..)?
    } else {
        let same_drive_ignoring_case = path.len() >= 3
            && directory.len() >= 3
            && path.as_bytes()[1] == b':'
            && directory.as_bytes()[1] == b':'
            && path.as_bytes()[0].eq_ignore_ascii_case(&directory.as_bytes()[0]);
        if same_drive_ignoring_case {
            path.get(1..)?.strip_prefix(directory.get(1..)?)?
        } else {
            path.strip_prefix(directory)?
        }
    };
    if remainder.is_empty() || directory.ends_with('/') {
        Some(remainder)
    } else {
        remainder.strip_prefix('/')
    }
}

fn canonical_text(path: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        path.to_owned()
    } else {
        to_file_name_lower_case(path)
    }
}

pub(crate) fn make_program_path(
    normalized_path: &str,
    case_sensitive: bool,
) -> Result<ProgramPath, ResolutionError> {
    let canonical = canonical_text(normalized_path, case_sensitive);
    ProgramPath::from_trusted_parts(normalized_path, canonical).map_err(|error| {
        ResolutionError::canonicalization(Some(PathBuf::from(normalized_path)), error.to_string())
    })
}

pub(crate) fn normalize_absolute_path(
    path: &Path,
    base: Option<&str>,
) -> Result<String, ResolutionError> {
    normalize_absolute_path_worker(path, base, true)
}

pub(crate) fn normalize_absolute_path_lexical(
    path: &Path,
    base: Option<&str>,
) -> Result<String, ResolutionError> {
    normalize_absolute_path_worker(path, base, false)
}

fn normalize_absolute_path_worker(
    path: &Path,
    base: Option<&str>,
    reject_nul: bool,
) -> Result<String, ResolutionError> {
    let text = path.to_str().ok_or_else(|| {
        ResolutionError::canonicalization(Some(path.to_path_buf()), "path is not valid Unicode")
    })?;
    if text.is_empty() || (reject_nul && text.contains('\0')) {
        return Err(ResolutionError::canonicalization(
            Some(path.to_path_buf()),
            "path is empty or contains a NUL byte",
        ));
    }
    let slashed = text.replace('\\', "/");
    let absolute = if is_normalized_rooted_text(&slashed) {
        slashed
    } else {
        let base = base.ok_or_else(|| {
            ResolutionError::canonicalization(
                Some(path.to_path_buf()),
                "an absolute path is required",
            )
        })?;
        if reject_nul && base.contains('\0') {
            return Err(ResolutionError::canonicalization(
                Some(path.to_path_buf()),
                "path base contains a NUL byte",
            ));
        }
        join_normalized(&base.replace('\\', "/"), &slashed)
    };
    normalize_rooted_text(&absolute)
        .map_err(|detail| ResolutionError::canonicalization(Some(path.to_path_buf()), detail))
}

fn is_normalized_rooted_text(path: &str) -> bool {
    normalized_root_parts(path).is_some()
}

fn normalize_rooted_text(path: &str) -> Result<String, &'static str> {
    let Some((root, _tail)) = normalized_root_parts(path) else {
        return Err("path has no supported absolute root");
    };
    let root_length = root.len();

    // Mirrors TypeScript 6.0.3 getNormalizedAbsolutePath/simpleNormalizePath.
    // Preserve TypeScript's observable trailing-separator behavior. In
    // particular, it removes only one trailing slash from an otherwise
    // simple path and retains the exact spelling produced by its slower
    // dot-segment worker.
    if let Some(simple) = simple_normalize_path(path) {
        return Ok(remove_trailing_separator_once(&simple, root_length));
    }

    let bytes = path.as_bytes();
    let mut normalized = None::<String>;
    let mut index = root_length;
    let mut normalized_up_to = index;
    let mut seen_non_dot_dot_segment = root_length != 0;
    while index < bytes.len() {
        let mut segment_start = index;
        while bytes[index] == b'/' && index + 1 < bytes.len() {
            index += 1;
        }
        if index > segment_start {
            normalized.get_or_insert_with(|| path[..segment_start.saturating_sub(1)].to_owned());
            segment_start = index;
        }

        let mut segment_end = index + 1;
        while segment_end < bytes.len() && bytes[segment_end] != b'/' {
            segment_end += 1;
        }
        let segment = &path[segment_start..segment_end];
        if segment == "." {
            normalized.get_or_insert_with(|| path[..normalized_up_to].to_owned());
        } else if segment == ".." {
            if !seen_non_dot_dot_segment {
                if let Some(normalized) = &mut normalized {
                    if normalized.len() == root_length {
                        normalized.push_str("..");
                    } else {
                        normalized.push_str("/..");
                    }
                } else {
                    normalized_up_to = index + 2;
                }
            } else if normalized.is_none() {
                let end = if normalized_up_to >= 2 {
                    path.as_bytes()[..=normalized_up_to - 2]
                        .iter()
                        .rposition(|byte| *byte == b'/')
                        .unwrap_or(root_length)
                        .max(root_length)
                } else {
                    normalized_up_to
                };
                normalized = Some(path[..end].to_owned());
            } else if let Some(normalized) = &mut normalized {
                if let Some(last_slash) = normalized.rfind('/') {
                    normalized.truncate(last_slash.max(root_length));
                } else {
                    normalized.replace_range(.., root);
                }
                if normalized.len() == root_length {
                    seen_non_dot_dot_segment = root_length != 0;
                }
            }
        } else if let Some(normalized) = &mut normalized {
            if normalized.len() != root_length {
                normalized.push('/');
            }
            seen_non_dot_dot_segment = true;
            normalized.push_str(segment);
        } else {
            seen_non_dot_dot_segment = true;
            normalized_up_to = segment_end;
        }
        index = segment_end + 1;
    }

    Ok(normalized.unwrap_or_else(|| remove_trailing_separator_once(path, root_length)))
}

fn simple_normalize_path(path: &str) -> Option<String> {
    if !has_relative_path_segment(path) {
        return Some(path.to_owned());
    }
    let mut simplified = path.replace("/./", "/");
    if simplified.starts_with("./") {
        simplified.drain(..2);
    }
    (simplified != path && !has_relative_path_segment(&simplified)).then_some(simplified)
}

fn has_relative_path_segment(path: &str) -> bool {
    path.contains("//")
        || path
            .split('/')
            .any(|component| matches!(component, "." | ".."))
}

fn remove_trailing_separator_once(path: &str, root_length: usize) -> String {
    if path.len() > root_length && path.ends_with('/') {
        path[..path.len() - 1].to_owned()
    } else {
        path.to_owned()
    }
}

pub(crate) fn normalized_root_parts(path: &str) -> Option<(&str, &str)> {
    if let Some(server_and_tail) = path.strip_prefix("//") {
        return match server_and_tail.find('/') {
            Some(separator) => {
                let root_end = 2 + separator + 1;
                Some((&path[..root_end], &path[root_end..]))
            }
            None => Some((path, "")),
        };
    }
    if let Some(tail) = path.strip_prefix('/') {
        return Some(("/", tail));
    }
    let bytes = path.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':') {
        if bytes.get(2) == Some(&b'/') {
            return Some((&path[..3], &path[3..]));
        }
        if bytes.len() == 2 {
            return Some((path, ""));
        }
    }
    let scheme_end = path.find("://")?;
    let authority_start = scheme_end + 3;
    match path[authority_start..].find('/') {
        Some(separator) => {
            let authority_end = authority_start + separator;
            if &path[..scheme_end] == "file"
                && matches!(&path[authority_start..authority_end], "" | "localhost")
                && bytes
                    .get(authority_end + 1)
                    .is_some_and(u8::is_ascii_alphabetic)
            {
                let volume_separator_start = authority_end + 2;
                let volume_separator_end = match bytes.get(volume_separator_start..) {
                    Some([b':', ..]) => Some(volume_separator_start + 1),
                    Some([b'%', b'3', b'a' | b'A', ..]) => Some(volume_separator_start + 3),
                    _ => None,
                };
                if let Some(volume_separator_end) = volume_separator_end {
                    if bytes.get(volume_separator_end) == Some(&b'/') {
                        let root_end = volume_separator_end + 1;
                        return Some((&path[..root_end], &path[root_end..]));
                    }
                    if volume_separator_end == bytes.len() {
                        return Some((path, ""));
                    }
                }
            }
            let root_end = authority_end + 1;
            Some((&path[..root_end], &path[root_end..]))
        }
        None => Some((path, "")),
    }
}

fn join_normalized(parent: &str, child: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{}", child.trim_start_matches('/'))
    } else {
        format!("{parent}/{}", child.trim_start_matches('/'))
    }
}

pub(crate) fn directory_name(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let root_length = normalized_root_parts(&slashed)
        .map(|(root, _)| root.len())
        .unwrap_or(0);
    if root_length == slashed.len() {
        return slashed;
    }
    let trimmed = slashed.strip_suffix('/').unwrap_or(slashed.as_str());
    let last_separator = trimmed.rfind('/').unwrap_or(0);
    trimmed[..root_length.max(last_separator)].to_owned()
}

fn base_name(path: &str) -> &str {
    if let Some((_root, tail)) = normalized_root_parts(path) {
        return tail.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    }
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

fn is_drive_root(path: &str) -> bool {
    path.len() == 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'/'
}

fn ancestor_directories(directory: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut current = directory.to_owned();
    loop {
        ancestors.push(current.clone());
        let parent = directory_name(&current);
        if parent == current {
            break;
        }
        current = parent;
    }
    ancestors
}

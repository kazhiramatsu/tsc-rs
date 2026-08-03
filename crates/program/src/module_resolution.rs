use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{Map, Value};
use tsc_host::{to_file_name_lower_case, CompilerHost};
use tsc_types::{compiler_version_satisfies, CompilerOptions};

use crate::json::parse_json_object;
use crate::path::ProgramPath;
use crate::prepared::{
    PackageJsonType, PackageMetadata, PathContext, PathMapping, ProgramOptions, SourceFileId,
};
use crate::resolution::{
    ModuleExtension, PackageId, ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModule,
    ResolvedModuleTarget, ResolvedTypeReferenceDirective,
};
use crate::text::decode_host_text;

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
    imports: Option<Value>,
    types_versions: Option<Value>,
    typings: Option<String>,
    types: Option<String>,
    main: Option<String>,
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

#[derive(Clone, Copy)]
enum ExtensionProbePass {
    All,
    Preferred,
    Declaration,
    DeclarationPackageField,
    Fallback,
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
    (ModuleExtension::Dts, ".d.ts"),
    (ModuleExtension::Jsx, ".jsx"),
];
const TS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Ts, ".ts")];
const TSX_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Tsx, ".tsx")];
const DTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dts, ".d.ts")];
const MTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Mts, ".mts")];
const DMTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dmts, ".d.mts")];
const CTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Cts, ".cts")];
const DCTS_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dcts, ".d.cts")];
const JSON_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dts, ".d.json.ts"),
    (ModuleExtension::Json, ".json"),
];
const JSON_DISABLED_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dts, ".d.json.ts"),
    (ModuleExtension::Ts, ".json.ts"),
    (ModuleExtension::Tsx, ".json.tsx"),
    (ModuleExtension::Dts, ".json.d.ts"),
    (ModuleExtension::Js, ".json.js"),
    (ModuleExtension::Jsx, ".json.jsx"),
];
const DECLARATION_PACKAGE_CJS_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dcts, ".d.cts"),
    (ModuleExtension::Cts, ".cts"),
];
const DECLARATION_PACKAGE_MJS_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dmts, ".d.mts"),
    (ModuleExtension::Mts, ".mts"),
];
const DECLARATION_PACKAGE_JS_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dts, ".d.ts"),
    (ModuleExtension::Ts, ".ts"),
    (ModuleExtension::Tsx, ".tsx"),
];
const DECLARATION_PACKAGE_JSX_PROBES: &[ExtensionProbe] = &[
    (ModuleExtension::Dts, ".d.ts"),
    (ModuleExtension::Tsx, ".tsx"),
    (ModuleExtension::Ts, ".ts"),
];
const DJSON_PROBES: &[ExtensionProbe] = &[(ModuleExtension::Dts, ".d.json.ts")];

#[derive(Clone, Copy)]
struct ExportProbeContext {
    is_external_library_import: bool,
    pass: ExtensionProbePass,
    mode: ResolutionMode,
    resolution_kind: i32,
    kind: PackageMapKind,
}

#[derive(Clone, Copy)]
struct LegacyResolutionContext {
    is_external_library_import: bool,
    attach_package_id: bool,
    resolved_using_ts_extension: bool,
    follow_realpath: bool,
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
    path_context: PathContext,
    type_root_base_directory: String,
    base_url: Option<String>,
    paths: Option<Vec<PathMapping>>,
    root_dirs: Option<Vec<String>>,
    package_cache: BTreeMap<String, PackageCacheEntry>,
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
        Self::new_with_owned_paths(host, options, None, None, None)
    }

    /// Construct a resolver with the ordered program-owned resolution options.
    ///
    /// `paths` mappings and `rootDirs` are cloned into this one-shot resolver
    /// so later resolution does not borrow the program configuration. The
    /// optional config identity also anchors default type roots. [`Self::new`]
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
            program_options.paths(),
            program_options.config_file_path(),
            program_options.root_dirs(),
        )
    }

    fn new_with_owned_paths(
        host: &'a dyn CompilerHost,
        options: &'a CompilerOptions,
        paths: Option<&[PathMapping]>,
        config_file_path: Option<&ProgramPath>,
        root_dirs: Option<&[ProgramPath]>,
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
        let base_url = normalize_base_url(options.base_url.as_deref(), &normalized)?;
        let paths = validate_and_clone_paths(paths)?;
        let root_dirs = validate_and_clone_root_dirs(root_dirs, &normalized, case_sensitive)?;
        Ok(Self {
            host,
            options,
            path_context: PathContext::new(current_directory, case_sensitive),
            type_root_base_directory,
            base_url,
            paths,
            root_dirs,
            package_cache: BTreeMap::new(),
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
        let base_url = normalize_base_url(options.base_url.as_deref(), current_directory)?;
        Ok(Self {
            host,
            options,
            type_root_base_directory: current_directory.to_owned(),
            path_context,
            base_url,
            paths: None,
            root_dirs: None,
            package_cache: BTreeMap::new(),
            active_resolutions: Vec::new(),
            active_package_maps: Vec::new(),
        })
    }

    pub fn path_context(&self) -> &PathContext {
        &self.path_context
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
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let has_paths = self.paths.as_ref().is_some_and(|paths| !paths.is_empty());
        let path_relative = is_path_relative_specifier(specifier);
        let external_relative = is_relative_specifier(specifier);
        if path_relative {
            return self.resolve_using_root_dirs(
                containing_directory,
                specifier,
                probe_pass,
                mode,
                loader,
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
                );
            }
            if self.base_url.is_none() {
                return Ok(ResolutionOutcome::NotFound);
            }
        }
        validate_owned_path_text(specifier, "module specifier", /* allow_empty */ false)?;

        if let Some((substitutions, capture)) = self.matching_paths(specifier) {
            let base_directory = self
                .base_url
                .clone()
                .unwrap_or(self.current_directory_text()?.to_owned());
            for substitution in substitutions {
                let expanded = match capture.as_deref() {
                    Some(capture) if !capture.is_empty() => substitution.replacen('*', capture, 1),
                    None => substitution.clone(),
                    Some(_) => substitution.clone(),
                };
                let candidate = normalize_optional_candidate(&expanded, &base_directory)?;

                // tryLoadModuleUsingPaths probes a substitution whose raw text
                // has a recognized extension exactly before invoking the
                // extension-family loader. The raw text is intentional: a
                // wildcard capture which happens to end in `.ts` does not
                // enable this shortcut.
                if let Some(extension) = recognized_module_extension(&substitution) {
                    if self.host.file_exists(Path::new(&candidate))? {
                        return self.finish_legacy_resolution(
                            None,
                            &candidate,
                            extension,
                            LegacyResolutionContext {
                                is_external_library_import: path_contains_node_modules(&candidate),
                                attach_package_id: false,
                                resolved_using_ts_extension: false,
                                follow_realpath: path_contains_node_modules(&candidate),
                            },
                        );
                    }
                }

                let outcome =
                    self.probe_optional_candidate(&candidate, &expanded, probe_pass, mode, loader)?;
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
            );
        }

        let Some(base_url) = self.base_url.clone() else {
            return Ok(ResolutionOutcome::NotFound);
        };
        let candidate = normalize_optional_candidate(specifier, &base_url)?;
        self.probe_optional_candidate(&candidate, specifier, probe_pass, mode, loader)
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
            let outcome =
                self.probe_optional_candidate(&candidate, specifier, probe_pass, mode, loader)?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn matching_paths(&self, specifier: &str) -> Option<(Vec<String>, Option<String>)> {
        let paths = self.paths.as_deref()?;
        if let Some(mapping) = paths.iter().find(|mapping| mapping.pattern() == specifier) {
            return Some((mapping.substitutions().to_vec(), None));
        }

        let mut best: Option<(&PathMapping, usize, String)> = None;
        for mapping in paths {
            let pattern = mapping.pattern();
            let Some(star) = pattern.find('*') else {
                continue;
            };
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
            let capture = specifier[prefix.len()..specifier.len() - suffix.len()].to_owned();
            best = Some((mapping, prefix.len(), capture));
        }
        best.map(|(mapping, _, capture)| (mapping.substitutions().to_vec(), Some(capture)))
    }

    fn probe_optional_candidate(
        &mut self,
        candidate: &str,
        written_candidate: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        loader: OptionalResolutionLoader,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        match loader {
            OptionalResolutionLoader::Classic => {
                self.probe_classic_file(candidate, written_candidate, probe_pass)
            }
            OptionalResolutionLoader::Node => {
                self.probe_optional_node_candidate(candidate, written_candidate, probe_pass, mode)
            }
        }
    }

    fn probe_optional_node_candidate(
        &mut self,
        candidate: &str,
        written_candidate: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let external = path_contains_node_modules(candidate);
        let allow_implicit = !self.is_node_esm_mode(mode);
        let context = LegacyResolutionContext {
            is_external_library_import: external,
            attach_package_id: false,
            resolved_using_ts_extension: is_typescript_family_specifier(written_candidate),
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
            if let ResolutionOutcome::Resolved(mut module) =
                self.probe_legacy_file(None, candidate, probe_pass, allow_implicit, context)?
            {
                self.attach_direct_node_package(&mut module)?;
                if external {
                    self.follow_module_realpath(&mut module)?;
                }
                return Ok(ResolutionOutcome::Resolved(module));
            }
        }
        let candidate_exists = self.host.directory_exists(Path::new(candidate))?;
        if !allow_implicit || !candidate_exists {
            return Ok(ResolutionOutcome::NotFound);
        }

        let package_json = join_normalized(candidate, "package.json");
        if let Some(directory_package) = self.load_package(&package_json)? {
            return self.resolve_legacy_package(
                &directory_package,
                ".",
                probe_pass,
                mode,
                LegacyResolutionContext {
                    attach_package_id: true,
                    resolved_using_ts_extension: false,
                    follow_realpath: external,
                    ..context
                },
                /* allow_node_esm_index_fallback */ true,
            );
        }
        self.probe_legacy_file(
            None,
            &join_normalized(candidate, "index"),
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                attach_package_id: false,
                resolved_using_ts_extension: false,
                follow_realpath: external,
                ..context
            },
        )
    }

    /// Attach the package facts used by `nodeLoadModuleByRelativeName` after
    /// a direct file probe has succeeded. Local candidates do not search
    /// ancestor manifests; an external optional-setting or type-reference
    /// candidate consults only its actual `node_modules` package root.
    fn attach_direct_node_package(
        &mut self,
        module: &mut HostResolvedModule,
    ) -> Result<(), ResolutionError> {
        if !module.is_external_library_import {
            return Ok(());
        }
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
        let Some(package_root) = node_modules_package_root(&lexical_path) else {
            return Ok(());
        };
        let Some(package) = self.load_package(&join_normalized(&package_root, "package.json"))?
        else {
            return Ok(());
        };
        module.package_id = package_id_for_legacy_path(&package, &lexical_path, true)?;
        module.package_metadata = Some(Rc::clone(&package.metadata));
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
        let (resolved_file, original_path) = self.realpath_program_path(&lexical_path)?;
        module.resolved_file = resolved_file;
        module.original_path = original_path;
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
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let external = path_contains_node_modules(candidate);
        let context = LegacyResolutionContext {
            is_external_library_import: external,
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
        self.attach_direct_node_package(&mut module)?;
        self.follow_module_realpath(&mut module)?;
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
        if specifier.contains(['\\', '\0']) {
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
                )?;
                if matches!(optional, ResolutionOutcome::Resolved(_)) {
                    return Ok(HostModuleResolution::new(optional, None));
                }
                let candidate = preserve_trailing_directory_separator(
                    normalize_absolute_path(Path::new(specifier), Some(&containing_directory))?,
                    specifier,
                );
                let outcome = self.probe_classic_file(&candidate, specifier, probe_pass)?;
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
                    let outcome = self.probe_classic_file(&candidate, specifier, probe_pass)?;
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
        &self,
        candidate: &str,
        written_specifier: &str,
        probe_pass: ExtensionProbePass,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.probe_legacy_file(
            None,
            candidate,
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                is_external_library_import: path_contains_node_modules(candidate),
                attach_package_id: false,
                resolved_using_ts_extension: is_typescript_family_specifier(written_specifier),
                follow_realpath: path_contains_node_modules(candidate),
            },
        )
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
        let (outcome, resolved_package_directory) =
            self.resolve_node10_non_relative(&containing_directory, specifier, mode)?;
        let retry_for_types = resolved_package_directory
            && match &outcome {
                ResolutionOutcome::NotFound => true,
                ResolutionOutcome::Resolved(module) => module.extension().is_javascript(),
            };
        let alternate_result = if retry_for_types {
            let request = parse_package_request(specifier)?;
            match self.resolve_bundler_preferred_non_relative(
                &containing_directory,
                specifier,
                &request,
                mode,
            )? {
                ResolutionOutcome::Resolved(module) if module.is_external_library_import() => {
                    Some(module.resolved_file().clone())
                }
                ResolutionOutcome::Resolved(_) | ResolutionOutcome::NotFound => None,
            }
        } else {
            None
        };
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
        for probe_pass in [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback] {
            let optional = self.resolve_using_optional_settings(
                containing_directory,
                specifier,
                probe_pass,
                mode,
                OptionalResolutionLoader::Node,
            )?;
            if matches!(optional, ResolutionOutcome::Resolved(_)) {
                // Upstream sets `resolvedPackageDirectory` only while walking
                // the ordinary node_modules package lookup, not when an
                // optional paths/baseUrl candidate happens to carry metadata.
                return Ok((optional, resolved_package_directory));
            }
            if request.is_none() {
                request = Some(parse_package_request(specifier)?);
            }
            let request = request.as_ref().expect("non-relative request was parsed");
            for ancestor in ancestor_directories(containing_directory) {
                if base_name(&ancestor) == "node_modules" {
                    continue;
                }
                let node_modules = join_normalized(&ancestor, "node_modules");
                if !self.host.directory_exists(Path::new(&node_modules))? {
                    continue;
                }
                let package_root = join_normalized(&node_modules, request.package_name);
                let package_directory_exists =
                    self.host.directory_exists(Path::new(&package_root))?;
                let package = if package_directory_exists {
                    self.load_package(&join_normalized(&package_root, "package.json"))?
                } else {
                    None
                };
                // Upstream records a resolved package directory only after
                // successfully reading package.json. A manifestless folder is
                // still eligible for Node10's legacy index probing, but must
                // not enable the diagnostic-only Bundler retry.
                resolved_package_directory |= package.is_some();
                let outcome = if let Some(package) = package {
                    self.resolve_legacy_package(
                        &package,
                        &request.exports_subpath,
                        probe_pass,
                        mode,
                        LegacyResolutionContext {
                            is_external_library_import: true,
                            attach_package_id: true,
                            resolved_using_ts_extension: false,
                            follow_realpath: true,
                        },
                        /* allow_node_esm_index_fallback */ true,
                    )?
                } else {
                    self.resolve_manifestless_package(
                        &package_root,
                        &request.exports_subpath,
                        probe_pass,
                        mode,
                    )?
                };
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok((outcome, resolved_package_directory));
                }

                if matches!(probe_pass, ExtensionProbePass::Preferred) {
                    let (outcome, at_types_package_observed) = self
                        .resolve_legacy_at_types_from_node_modules(&node_modules, request, mode)?;
                    resolved_package_directory |= at_types_package_observed;
                    if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                        return Ok((outcome, resolved_package_directory));
                    }
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
            let (outcome, _) =
                self.resolve_legacy_at_types_from_node_modules(&node_modules, request, mode)?;
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
    ) -> Result<(ResolutionOutcome<HostResolvedModule>, bool), ResolutionError> {
        let at_types = join_normalized(node_modules, "@types");
        if !self.host.directory_exists(Path::new(&at_types))? {
            return Ok((ResolutionOutcome::NotFound, false));
        }
        let package_root =
            join_normalized(&at_types, &mangle_scoped_package_name(request.package_name));
        let package = if self.host.directory_exists(Path::new(&package_root))? {
            self.load_package(&join_normalized(&package_root, "package.json"))?
        } else {
            None
        };
        let package_info_observed = package.is_some();
        let outcome = if let Some(package) = package {
            self.resolve_legacy_package(
                &package,
                &request.exports_subpath,
                ExtensionProbePass::Declaration,
                mode,
                LegacyResolutionContext {
                    is_external_library_import: true,
                    attach_package_id: true,
                    resolved_using_ts_extension: false,
                    follow_realpath: true,
                },
                /* allow_node_esm_index_fallback */ true,
            )
        } else {
            self.resolve_manifestless_package(
                &package_root,
                &request.exports_subpath,
                ExtensionProbePass::Declaration,
                mode,
            )
        }?;
        Ok((outcome, package_info_observed))
    }

    fn resolve_bundler_preferred_non_relative(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let optional = self.resolve_using_optional_settings(
            containing_directory,
            specifier,
            ExtensionProbePass::Preferred,
            mode,
            OptionalResolutionLoader::Node,
        )?;
        if matches!(optional, ResolutionOutcome::Resolved(_)) {
            return Ok(optional);
        }
        for ancestor in ancestor_directories(containing_directory) {
            if base_name(&ancestor) == "node_modules" {
                continue;
            }
            let node_modules = join_normalized(&ancestor, "node_modules");
            if !self.host.directory_exists(Path::new(&node_modules))? {
                continue;
            }
            let package_root = join_normalized(&node_modules, request.package_name);
            let package = if self.host.directory_exists(Path::new(&package_root))? {
                self.load_package(&join_normalized(&package_root, "package.json"))?
            } else {
                None
            };
            let outcome = if let Some(package) = package {
                let uses_exports = self.options.resolve_package_json_exports != Some(false)
                    && !matches!(package.exports.as_ref(), None | Some(Value::Null));
                if uses_exports {
                    self.resolve_package_exports_with_resolution_kind(
                        &package,
                        &request.exports_subpath,
                        /* is_external_library_import */ true,
                        ExtensionProbePass::Preferred,
                        mode,
                        100,
                    )?
                } else {
                    self.resolve_legacy_package(
                        &package,
                        &request.exports_subpath,
                        ExtensionProbePass::Preferred,
                        mode,
                        LegacyResolutionContext {
                            is_external_library_import: true,
                            attach_package_id: true,
                            resolved_using_ts_extension: false,
                            follow_realpath: true,
                        },
                        /* allow_node_esm_index_fallback */ true,
                    )?
                }
            } else {
                self.resolve_manifestless_package(
                    &package_root,
                    &request.exports_subpath,
                    ExtensionProbePass::Preferred,
                    mode,
                )?
            };
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }

            let types_package = PackageRequest {
                package_name: request.package_name,
                exports_subpath: request.exports_subpath.clone(),
            };
            let outcome =
                self.resolve_bundler_preferred_at_types(&node_modules, &types_package, mode)?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn resolve_bundler_preferred_at_types(
        &mut self,
        node_modules: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let package_root = join_normalized(
            &join_normalized(node_modules, "@types"),
            &mangle_scoped_package_name(request.package_name),
        );
        let package = if self.host.directory_exists(Path::new(&package_root))? {
            self.load_package(&join_normalized(&package_root, "package.json"))?
        } else {
            None
        };
        if let Some(package) = package {
            let uses_exports = self.options.resolve_package_json_exports != Some(false)
                && !matches!(package.exports.as_ref(), None | Some(Value::Null));
            if uses_exports {
                self.resolve_package_exports_with_resolution_kind(
                    &package,
                    &request.exports_subpath,
                    /* is_external_library_import */ true,
                    ExtensionProbePass::Preferred,
                    mode,
                    100,
                )
            } else {
                self.resolve_legacy_package(
                    &package,
                    &request.exports_subpath,
                    ExtensionProbePass::Preferred,
                    mode,
                    LegacyResolutionContext {
                        is_external_library_import: true,
                        attach_package_id: true,
                        resolved_using_ts_extension: false,
                        follow_realpath: true,
                    },
                    /* allow_node_esm_index_fallback */ true,
                )
            }
        } else {
            self.resolve_manifestless_package(
                &package_root,
                &request.exports_subpath,
                ExtensionProbePass::Preferred,
                mode,
            )
        }
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
            )?;
            if let ResolutionOutcome::Resolved(module) = outcome {
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
            ResolutionOutcome::Resolved(module) => ResolutionOutcome::Resolved(
                HostResolvedTypeReferenceDirective::from_module(module, false),
            ),
            ResolutionOutcome::NotFound => ResolutionOutcome::NotFound,
        })
    }

    fn resolve_non_relative(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        const MAX_PACKAGE_MAP_REWRITES: usize = 64;
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
        if self.active_resolutions.len() >= MAX_PACKAGE_MAP_REWRITES
            || self.active_resolutions.contains(&active)
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.active_resolutions.push(active);
        let result = self.resolve_non_relative_inner(&containing_directory, specifier, mode);
        self.active_resolutions.pop();
        result
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
        )?;
        if matches!(optional, ResolutionOutcome::Resolved(_)) {
            return Ok(optional);
        }
        if specifier.starts_with('#') {
            if let Search::Terminal(outcome) =
                self.resolve_package_imports(containing_directory, specifier, mode)?
            {
                return Ok(outcome);
            }
        }
        let request = parse_package_request(specifier)?;

        if let Search::Terminal(outcome) =
            self.try_self_reference(containing_directory, &request, mode)?
        {
            return Ok(outcome);
        }

        self.resolve_from_node_modules(containing_directory, &request, mode)
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
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        if self.options.resolve_package_json_exports == Some(false) {
            return Ok(Search::Continue);
        }
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
            || matches!(package.exports.as_ref(), None | Some(Value::Null))
        {
            return Ok(Search::Continue);
        }

        self.search_package_exports(
            &package,
            &request.exports_subpath,
            /* is_external_library_import */ false,
            ExtensionProbePass::All,
            mode,
            self.options.emit_module_resolution_kind(),
        )
    }

    /// tsc-port: loadModuleFromImports @6.0.3
    /// tsc-hash: 4f4510daf578be52814574369949af61fa39b610fef58eadc272282bfd77f6d5
    /// tsc-span: _tsc.js:41534-41586
    fn resolve_package_imports(
        &mut self,
        containing_directory: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        if self.options.resolve_package_json_imports == Some(false) {
            return Ok(Search::Continue);
        }
        let resolution_kind = self.options.emit_module_resolution_kind();
        if specifier == "#" || (specifier.starts_with("#/") && resolution_kind == 3) {
            return Ok(Search::Terminal(ResolutionOutcome::NotFound));
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
                pass: ExtensionProbePass::All,
                mode,
                resolution_kind,
                kind: PackageMapKind::Imports,
            },
        );
        self.active_package_maps.pop();
        search
    }

    fn resolve_from_node_modules(
        &mut self,
        containing_directory: &str,
        request: &PackageRequest<'_>,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        for probe_pass in [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback] {
            for ancestor in ancestor_directories(containing_directory) {
                if base_name(&ancestor) == "node_modules" {
                    continue;
                }
                let node_modules = join_normalized(&ancestor, "node_modules");
                if !self.host.directory_exists(Path::new(&node_modules))? {
                    continue;
                }
                let package_root = join_normalized(&node_modules, request.package_name);
                let package = if self.host.directory_exists(Path::new(&package_root))? {
                    let package_json = join_normalized(&package_root, "package.json");
                    self.load_package(&package_json)?
                } else {
                    None
                };
                if let Some(package) = package {
                    let uses_exports = self.options.resolve_package_json_exports != Some(false)
                        && !matches!(package.exports.as_ref(), None | Some(Value::Null));
                    if !uses_exports
                        && request.exports_subpath == "."
                        && !self.is_node_esm_mode(mode)
                    {
                        let direct = self.probe_legacy_file(
                            Some(&package),
                            &package_root,
                            probe_pass,
                            /* allow_implicit */ true,
                            LegacyResolutionContext {
                                is_external_library_import: true,
                                attach_package_id: true,
                                resolved_using_ts_extension: false,
                                follow_realpath: true,
                            },
                        )?;
                        if matches!(direct, ResolutionOutcome::Resolved(_)) {
                            return Ok(direct);
                        }
                    }
                    let mut outcome = if uses_exports {
                        self.resolve_package_exports(
                            &package,
                            &request.exports_subpath,
                            /* is_external_library_import */ true,
                            probe_pass,
                            mode,
                        )?
                    } else {
                        self.resolve_legacy_package(
                            &package,
                            &request.exports_subpath,
                            probe_pass,
                            mode,
                            LegacyResolutionContext {
                                is_external_library_import: true,
                                attach_package_id: true,
                                resolved_using_ts_extension: false,
                                follow_realpath: true,
                            },
                            /* allow_node_esm_index_fallback */ true,
                        )?
                    };
                    if let ResolutionOutcome::Resolved(module) = &mut outcome {
                        if uses_exports
                            && mode == ResolutionMode::EsNext
                            && module.extension().is_javascript()
                        {
                            if let ResolutionOutcome::Resolved(alternate) = self
                                .resolve_legacy_package(
                                    &package,
                                    &request.exports_subpath,
                                    ExtensionProbePass::Preferred,
                                    mode,
                                    LegacyResolutionContext {
                                        is_external_library_import: true,
                                        attach_package_id: true,
                                        resolved_using_ts_extension: false,
                                        follow_realpath: true,
                                    },
                                    /* allow_node_esm_index_fallback */ true,
                                )?
                            {
                                module.alternate_result = Some(alternate.resolved_file().clone());
                            }
                        }
                        return Ok(outcome);
                    }
                } else {
                    let outcome = self.resolve_manifestless_package(
                        &package_root,
                        &request.exports_subpath,
                        probe_pass,
                        mode,
                    )?;
                    if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                        return Ok(outcome);
                    }
                }

                if matches!(probe_pass, ExtensionProbePass::Preferred) {
                    let at_types = join_normalized(&node_modules, "@types");
                    if self.host.directory_exists(Path::new(&at_types))? {
                        let types_package = join_normalized(
                            &at_types,
                            &mangle_scoped_package_name(request.package_name),
                        );
                        let use_package_exports =
                            self.options.resolve_package_json_exports != Some(false);
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
                }
            }
        }
        Ok(ResolutionOutcome::NotFound)
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
        let rest = package_subpath(exports_subpath)?;
        let mut root_package = None;
        let mut root_package_loaded = false;

        // A package subpath is itself allowed to be a package boundary. The
        // candidate manifest is observed before the root manifest, even when
        // the root exports map will ultimately own the request.
        if let Some(rest) = rest {
            let candidate =
                normalize_absolute_path(Path::new(&join_normalized(package_root, rest)), None)?;
            let nested_package = self.load_package(&join_normalized(&candidate, "package.json"))?;
            if let Some(nested_package) = nested_package {
                let root_exports_govern = if use_package_exports {
                    root_package = self.load_declaration_package_manifest(package_root)?;
                    root_package_loaded = true;
                    root_package
                        .as_ref()
                        .is_some_and(|package| package.exports.is_some())
                } else {
                    false
                };
                if !root_exports_govern {
                    return self.resolve_nested_declaration_package(
                        &candidate,
                        rest,
                        &nested_package,
                        mode,
                    );
                }
            }
        }

        let package = if root_package_loaded {
            root_package
        } else {
            self.load_declaration_package_manifest(package_root)?
        };
        if let Some(package) = package {
            let uses_exports = use_package_exports
                && !matches!(package.exports.as_ref(), None | Some(Value::Null));
            if !uses_exports && exports_subpath == "." && !self.is_node_esm_mode(mode) {
                let direct = self.probe_legacy_file(
                    Some(&package),
                    package_root,
                    ExtensionProbePass::Declaration,
                    /* allow_implicit */ true,
                    LegacyResolutionContext {
                        is_external_library_import: true,
                        attach_package_id: true,
                        resolved_using_ts_extension: false,
                        follow_realpath: true,
                    },
                )?;
                if matches!(direct, ResolutionOutcome::Resolved(_)) {
                    return Ok(direct);
                }
            }
            if uses_exports {
                self.resolve_package_exports(
                    &package,
                    exports_subpath,
                    /* is_external_library_import */ true,
                    ExtensionProbePass::Declaration,
                    mode,
                )
            } else {
                self.resolve_legacy_package(
                    &package,
                    exports_subpath,
                    ExtensionProbePass::Declaration,
                    mode,
                    LegacyResolutionContext {
                        is_external_library_import: true,
                        attach_package_id: true,
                        resolved_using_ts_extension: false,
                        follow_realpath: true,
                    },
                    /* allow_node_esm_index_fallback */ true,
                )
            }
        } else {
            self.resolve_manifestless_package(
                package_root,
                exports_subpath,
                ExtensionProbePass::Declaration,
                mode,
            )
        }
    }

    fn load_declaration_package_manifest(
        &mut self,
        package_root: &str,
    ) -> Result<Option<Rc<CachedPackage>>, ResolutionError> {
        if !self.host.directory_exists(Path::new(package_root))? {
            return Ok(None);
        }
        self.load_package(&join_normalized(package_root, "package.json"))
    }

    fn resolve_nested_declaration_package(
        &self,
        candidate: &str,
        written_subpath: &str,
        package: &CachedPackage,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let direct = self.probe_legacy_file(
            None,
            candidate,
            ExtensionProbePass::Declaration,
            /* allow_implicit */ !self.is_node_esm_mode(mode),
            LegacyResolutionContext {
                is_external_library_import: true,
                attach_package_id: false,
                resolved_using_ts_extension: is_typescript_family_specifier(written_subpath),
                follow_realpath: true,
            },
        )?;
        if matches!(direct, ResolutionOutcome::Resolved(_)) {
            return Ok(direct);
        }
        self.resolve_legacy_package(
            package,
            ".",
            ExtensionProbePass::Declaration,
            mode,
            LegacyResolutionContext {
                is_external_library_import: true,
                attach_package_id: true,
                resolved_using_ts_extension: false,
                follow_realpath: true,
            },
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

    fn resolve_type_reference_from_root(
        &mut self,
        type_root: &str,
        specifier: &str,
        mode: ResolutionMode,
        custom_type_roots: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        if !self.host.directory_exists(Path::new(type_root))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        let name_for_lookup = if type_root.ends_with("/node_modules/@types") {
            mangle_scoped_package_name(specifier)
        } else {
            specifier.to_owned()
        };
        let candidate = normalize_absolute_path(
            Path::new(&join_normalized(type_root, &name_for_lookup)),
            None,
        )?;
        let external = path_contains_node_modules(&candidate);
        let context = LegacyResolutionContext {
            is_external_library_import: external,
            attach_package_id: external,
            resolved_using_ts_extension: false,
            follow_realpath: true,
        };

        // An explicitly configured type root first receives the declaration
        // file probe that default node_modules/@types roots deliberately omit.
        if custom_type_roots {
            let direct = self.probe_direct_type_reference_file(&candidate, mode)?;
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

            let package_root = join_normalized(&node_modules, request.package_name);
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
            let types_package =
                join_normalized(&at_types, &mangle_scoped_package_name(request.package_name));
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
        let target =
            normalize_absolute_path(Path::new(specifier), Some(&directory_name(containing_file)))?;
        let external = path_contains_node_modules(&target);
        let context = LegacyResolutionContext {
            is_external_library_import: external,
            attach_package_id: external,
            resolved_using_ts_extension: false,
            follow_realpath: true,
        };
        let allow_implicit = !self.is_node_esm_mode(mode);
        let direct = self.probe_direct_type_reference_file(&target, mode)?;
        if matches!(direct, ResolutionOutcome::Resolved(_)) {
            return Ok(direct);
        }
        if !allow_implicit || !self.host.directory_exists(Path::new(&target))? {
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
        if specifier.contains(['\\', '\0']) {
            return Err(ResolutionError::invalid_data(format!(
                "invalid relative module specifier {specifier:?}"
            )));
        }
        let containing_directory = directory_name(containing_file);
        let directory_spelling = has_node_directory_spelling(specifier);
        // Program paths use the loader's normalized host spelling, so the
        // separator itself is not retained here. Keep its semantic effect in
        // a separate bit: Node skips the file phase for an explicit trailing
        // separator or a final `.`/`..` component.
        let target = normalize_absolute_path(Path::new(specifier), Some(&containing_directory))?;
        let external = path_contains_node_modules(&target);
        let allow_implicit = !self.is_node_esm_mode(mode);
        let mut package = None;
        let mut package_observed = false;

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

        for &probe_pass in probe_passes {
            let optional = self.resolve_using_optional_settings(
                &containing_directory,
                specifier,
                probe_pass,
                mode,
                OptionalResolutionLoader::Node,
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
                if !package_observed {
                    package = self.find_nearest_package_scope(&directory_name(&target))?;
                    package_observed = true;
                }
                let outcome = self.probe_legacy_file(
                    package.as_deref(),
                    &target,
                    probe_pass,
                    allow_implicit,
                    LegacyResolutionContext {
                        is_external_library_import: external,
                        attach_package_id: external,
                        resolved_using_ts_extension: is_typescript_family_specifier(specifier),
                        follow_realpath: false,
                    },
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            }

            let target_exists = self.host.directory_exists(Path::new(&target))?;
            if !allow_implicit || !target_exists {
                continue;
            }
            if !package_observed {
                package = self.find_nearest_package_scope(&directory_name(&target))?;
                package_observed = true;
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
                    /* allow_node_esm_index_fallback */ true,
                )?;
                if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                    return Ok(outcome);
                }
            } else {
                let index = join_normalized(&target, "index");
                let outcome = self.probe_legacy_file(
                    package.as_deref(),
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
        package_root: &str,
        exports_subpath: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let rest = package_subpath(exports_subpath)?;
        let candidate = rest.map_or_else(
            || package_root.to_owned(),
            |rest| join_normalized(package_root, rest),
        );
        let allow_implicit = !self.is_node_esm_mode(mode);
        if rest.is_some() || allow_implicit {
            let outcome = self.probe_legacy_file(
                None,
                &candidate,
                probe_pass,
                allow_implicit,
                LegacyResolutionContext {
                    is_external_library_import: true,
                    attach_package_id: false,
                    resolved_using_ts_extension: rest.is_some_and(is_typescript_family_specifier),
                    follow_realpath: true,
                },
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }
        if allow_implicit && self.host.directory_exists(Path::new(&candidate))? {
            return self.probe_legacy_file(
                None,
                &join_normalized(&candidate, "index"),
                probe_pass,
                /* allow_implicit */ true,
                LegacyResolutionContext {
                    is_external_library_import: true,
                    attach_package_id: false,
                    resolved_using_ts_extension: false,
                    follow_realpath: true,
                },
            );
        }
        Ok(ResolutionOutcome::NotFound)
    }

    /// Resolve package `typings`, `types`, `main`, `typesVersions`, and the
    /// legacy index fallback without consulting a package.json below the
    /// package root.
    fn resolve_legacy_package(
        &self,
        package: &CachedPackage,
        exports_subpath: &str,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        context: LegacyResolutionContext,
        allow_node_esm_index_fallback: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let rest = package_subpath(exports_subpath)?;
        let logical_name = rest.map(str::to_owned).unwrap_or_else(|| {
            package_version_logical_name(package, probe_pass).unwrap_or_else(|| "index".to_owned())
        });
        match self.search_package_types_versions(
            package,
            &logical_name,
            probe_pass,
            mode,
            context,
            /* is_package_root */ rest.is_none(),
        )? {
            Search::Terminal(outcome) => return Ok(outcome),
            Search::Continue => {}
        }

        if let Some(rest) = rest {
            let candidate = normalize_package_target(package, rest)?;
            return self.probe_legacy_path(
                Some(package),
                &candidate,
                probe_pass,
                !self.is_node_esm_mode(mode),
                LegacyResolutionContext {
                    resolved_using_ts_extension: is_typescript_family_specifier(rest),
                    ..context
                },
            );
        }

        let allow_package_field_directory = !self.is_node_esm_mode(mode)
            || package.metadata.module_type() != PackageJsonType::Module;
        if let Some(field) = selected_package_entry_field(package, probe_pass) {
            let candidate = normalize_package_target(package, field)?;
            // loadNodeModuleFromDirectoryWorker first probes a declaration
            // twin for a written package-json extension, then expands to
            // TypeScript + declaration files.
            let field_probe_pass = if matches!(probe_pass, ExtensionProbePass::Declaration) {
                ExtensionProbePass::DeclarationPackageField
            } else {
                probe_pass
            };
            let outcome = self.probe_legacy_path(
                Some(package),
                &candidate,
                field_probe_pass,
                allow_package_field_directory,
                LegacyResolutionContext {
                    resolved_using_ts_extension: false,
                    ..context
                },
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(outcome);
            }
        }

        // Node ESM still assumes a package-root index.js only when the
        // manifest has no effective exports value. Subpath and relative
        // directory requests do not receive this exception.
        let node_esm = self.is_node_esm_mode(mode);
        if node_esm
            && (!allow_node_esm_index_fallback
                || !matches!(package.exports.as_ref(), None | Some(Value::Null)))
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        let index = if node_esm {
            join_normalized(&package.root, "index.js")
        } else {
            join_normalized(&package.root, "index")
        };
        self.probe_legacy_file(
            Some(package),
            &index,
            probe_pass,
            /* allow_implicit */ !node_esm,
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )
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
        context: LegacyResolutionContext,
        is_package_root: bool,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let Some(types_versions) = package.types_versions.as_ref() else {
            return Ok(Search::Continue);
        };
        let table = types_versions.as_object().ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "{} typesVersions must be an object",
                package.metadata.package_json().display().display()
            ))
        })?;
        let matching = table
            .iter()
            .find(|(range, _)| compiler_version_satisfies(range) == Some(true));
        let Some((range, mappings)) = matching else {
            return Ok(Search::Continue);
        };
        let mappings = mappings.as_object().ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "typesVersions[{range:?}] in {} must be an object",
                package.metadata.package_json().display().display()
            ))
        })?;
        let Some((pattern, capture, targets)) =
            select_types_versions_mapping(mappings, logical_name)
        else {
            return Ok(Search::Continue);
        };
        let targets = targets.as_array().ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "typesVersions mapping {pattern:?} must be an array"
            ))
        })?;
        for target in targets {
            let target = target.as_str().ok_or_else(|| {
                ResolutionError::invalid_data(format!(
                    "typesVersions mapping {pattern:?} contains a non-string target"
                ))
            })?;
            let expanded = target.replace('*', capture);
            let target = expanded.strip_prefix("./").unwrap_or(&expanded);
            if target.is_empty()
                || target.starts_with(['/', '\\'])
                || target.contains(['\\', '\0', ':'])
            {
                return Err(ResolutionError::invalid_data(format!(
                    "typesVersions target {expanded:?} is not package-relative"
                )));
            }
            let candidate = normalize_package_target(package, target)?;
            // tsc's paths loader first probes a substitution that already has
            // a recognized extension exactly, irrespective of the preferred
            // TypeScript/declaration pass. The paths loader itself returns an
            // exact hit without a package id; the outer package-root loader
            // may attach the root package id again. An exact miss falls
            // through to the ordinary package loader.
            if !matches!(probe_pass, ExtensionProbePass::Fallback) {
                if let Some(extension) = recognized_module_extension(&candidate) {
                    if self.host.file_exists(Path::new(&candidate))? {
                        return self
                            .finish_legacy_resolution(
                                Some(package),
                                &candidate,
                                extension,
                                LegacyResolutionContext {
                                    attach_package_id: context.attach_package_id && is_package_root,
                                    resolved_using_ts_extension: false,
                                    ..context
                                },
                            )
                            .map(Search::Terminal);
                    }
                }
            }
            let mapping_probe_pass =
                if matches!(probe_pass, ExtensionProbePass::Declaration) && is_package_root {
                    // The directory package loader applies package-field
                    // declaration-twin precedence to root substitutions.
                    ExtensionProbePass::DeclarationPackageField
                } else {
                    probe_pass
                };
            let outcome = self.probe_legacy_path(
                Some(package),
                &candidate,
                mapping_probe_pass,
                !self.is_node_esm_mode(mode)
                    || (is_package_root
                        && package.metadata.module_type() != PackageJsonType::Module),
                LegacyResolutionContext {
                    resolved_using_ts_extension: false,
                    ..context
                },
            )?;
            if matches!(outcome, ResolutionOutcome::Resolved(_)) {
                return Ok(Search::Terminal(outcome));
            }
        }
        Ok(Search::Terminal(ResolutionOutcome::NotFound))
    }

    fn probe_legacy_path(
        &self,
        package: Option<&CachedPackage>,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        allow_implicit: bool,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let outcome =
            self.probe_legacy_file(package, candidate, probe_pass, allow_implicit, context)?;
        if matches!(outcome, ResolutionOutcome::Resolved(_)) || !allow_implicit {
            return Ok(outcome);
        }
        if !self.host.directory_exists(Path::new(candidate))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.probe_legacy_file(
            package,
            &join_normalized(candidate, "index"),
            probe_pass,
            /* allow_implicit */ true,
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )
    }

    fn probe_legacy_file(
        &self,
        package: Option<&CachedPackage>,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        allow_implicit: bool,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        // loadNodeModuleFromDirectoryWorker retains EsmMode for a package
        // whose `type` is `module`. Its expanded package-field loader may
        // replace a written extension, but must not add one to an
        // extensionless `types` or `typesVersions` target.
        if matches!(probe_pass, ExtensionProbePass::DeclarationPackageField)
            && !allow_implicit
            && !base_name(candidate).contains('.')
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        let plan = match probe_pass {
            ExtensionProbePass::Declaration => declaration_extension_probe_plan(candidate),
            ExtensionProbePass::DeclarationPackageField => {
                declaration_package_field_probe_plan(candidate)
            }
            _ => extension_probe_plan(candidate, self.options.resolve_json_module_effective()),
        };
        let (base, probes, preferred_len) = match plan {
            Ok(plan) => plan,
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension"
                    && allow_implicit
                    && !base_name(candidate).contains('.') =>
            {
                if matches!(probe_pass, ExtensionProbePass::Declaration) {
                    (candidate, DTS_PROBES, 1)
                } else {
                    (candidate, JS_PROBES, 3)
                }
            }
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension" =>
            {
                return self
                    .probe_arbitrary_declaration_twin(package, candidate, probe_pass, context);
            }
            Err(error) => return Err(error),
        };
        let probes = match probe_pass {
            ExtensionProbePass::All => probes,
            ExtensionProbePass::Preferred => &probes[..preferred_len],
            ExtensionProbePass::Declaration => probes,
            ExtensionProbePass::DeclarationPackageField => &probes[..preferred_len],
            ExtensionProbePass::Fallback => &probes[preferred_len..],
        };
        if !self
            .host
            .directory_exists(Path::new(&directory_name(candidate)))?
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        for (extension, suffix) in probes {
            let path = format!("{base}{suffix}");
            if self.host.file_exists(Path::new(&path))? {
                return self.finish_legacy_resolution(
                    package,
                    &path,
                    extension.clone(),
                    LegacyResolutionContext {
                        resolved_using_ts_extension: context.resolved_using_ts_extension
                            && is_typescript_module_extension(extension),
                        ..context
                    },
                );
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    fn probe_arbitrary_declaration_twin(
        &self,
        package: Option<&CachedPackage>,
        candidate: &str,
        probe_pass: ExtensionProbePass,
        context: LegacyResolutionContext,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        if matches!(probe_pass, ExtensionProbePass::Fallback) {
            return Ok(ResolutionOutcome::NotFound);
        }
        let Some((path, extension)) = arbitrary_declaration_twin(candidate) else {
            return Ok(ResolutionOutcome::NotFound);
        };
        if !self
            .host
            .directory_exists(Path::new(&directory_name(candidate)))?
            || !self.host.file_exists(Path::new(&path))?
        {
            return Ok(ResolutionOutcome::NotFound);
        }
        self.finish_legacy_resolution(
            package,
            &path,
            ModuleExtension::Arbitrary(extension),
            LegacyResolutionContext {
                resolved_using_ts_extension: false,
                ..context
            },
        )
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
        if let Some(entry) = self.package_cache.get(&cache_key) {
            return Ok(match entry {
                PackageCacheEntry::Missing => None,
                PackageCacheEntry::Found(package) => Some(Rc::clone(package)),
            });
        }

        let package_json_path = Path::new(package_json);
        let package_directory = directory_name(package_json);
        if !self.host.directory_exists(Path::new(&package_directory))?
            || !self.host.file_exists(package_json_path)?
        {
            self.package_cache
                .insert(cache_key, PackageCacheEntry::Missing);
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
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let version = object
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let module_type = match object.get("type").and_then(Value::as_str) {
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
            exports: object.get("exports").cloned(),
            imports: object.get("imports").cloned(),
            types_versions: object.get("typesVersions").cloned(),
            typings: non_empty_string_field(&object, "typings"),
            types: non_empty_string_field(&object, "types"),
            main: non_empty_string_field(&object, "main"),
            metadata,
        });
        self.package_cache
            .insert(cache_key, PackageCacheEntry::Found(Rc::clone(&package)));
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
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let search = self.search_package_exports(
            package,
            subpath,
            is_external_library_import,
            probe_pass,
            mode,
            self.options.emit_module_resolution_kind(),
        )?;
        Ok(match search {
            // A present exports map suppresses every legacy package fallback.
            Search::Continue => ResolutionOutcome::NotFound,
            Search::Terminal(outcome) => outcome,
        })
    }

    fn resolve_package_exports_with_resolution_kind(
        &mut self,
        package: &CachedPackage,
        subpath: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        resolution_kind: i32,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let search = self.search_package_exports(
            package,
            subpath,
            is_external_library_import,
            probe_pass,
            mode,
            resolution_kind,
        )?;
        Ok(match search {
            Search::Continue => ResolutionOutcome::NotFound,
            Search::Terminal(outcome) => outcome,
        })
    }

    /// Preserve the upstream SearchResult distinction for self references:
    /// an ordinary target miss continues to node_modules, while an explicit
    /// null target is terminal.
    fn search_package_exports(
        &mut self,
        package: &CachedPackage,
        subpath: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
        mode: ResolutionMode,
        resolution_kind: i32,
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

        let search = match exports {
            Value::String(target) if target.is_empty() => {
                return Err(ResolutionError::unsupported(
                    "legacy-node-package-entry-from-falsy-exports",
                    format!(
                        "{} has a falsy exports value and requires legacy package fallback",
                        package.metadata.package_json().display().display()
                    ),
                ));
            }
            Value::String(_) if subpath == "." => self.resolve_selected_export(
                package,
                exports,
                "",
                false,
                ExportProbeContext {
                    is_external_library_import,
                    pass: probe_pass,
                    mode,
                    resolution_kind,
                    kind: PackageMapKind::Exports,
                },
            )?,
            Value::String(_) => Search::Continue,
            Value::Object(table) => {
                let has_dotted_key = table.keys().any(|key| key.starts_with('.'));
                let has_condition_key = table.keys().any(|key| !key.starts_with('.'));
                if has_dotted_key && has_condition_key {
                    return Err(ResolutionError::invalid_data(format!(
                        "{} exports mixes subpath and condition keys",
                        package.metadata.package_json().display().display()
                    )));
                }
                if has_condition_key {
                    if subpath == "." {
                        self.resolve_selected_export(
                            package,
                            exports,
                            "",
                            false,
                            ExportProbeContext {
                                is_external_library_import,
                                pass: probe_pass,
                                mode,
                                resolution_kind,
                                kind: PackageMapKind::Exports,
                            },
                        )?
                    } else {
                        Search::Continue
                    }
                } else {
                    self.search_exports_table(
                        package,
                        table,
                        subpath,
                        ExportProbeContext {
                            is_external_library_import,
                            pass: probe_pass,
                            mode,
                            resolution_kind,
                            kind: PackageMapKind::Exports,
                        },
                    )?
                }
            }
            Value::Array(_) if subpath == "." => self.resolve_selected_export(
                package,
                exports,
                "",
                false,
                ExportProbeContext {
                    is_external_library_import,
                    pass: probe_pass,
                    mode,
                    resolution_kind,
                    kind: PackageMapKind::Exports,
                },
            )?,
            Value::Array(_) => Search::Continue,
            Value::Null | Value::Bool(false) => {
                return Err(ResolutionError::unsupported(
                    "legacy-node-package-entry-from-falsy-exports",
                    format!(
                        "{} has a falsy exports value and requires legacy package fallback",
                        package.metadata.package_json().display().display()
                    ),
                ));
            }
            Value::Number(number) if number.as_f64() == Some(0.0) => {
                return Err(ResolutionError::unsupported(
                    "legacy-node-package-entry-from-falsy-exports",
                    format!(
                        "{} has a falsy exports value and requires legacy package fallback",
                        package.metadata.package_json().display().display()
                    ),
                ));
            }
            Value::Bool(_) | Value::Number(_) => {
                return Err(ResolutionError::invalid_data(format!(
                    "{} has an invalid exports value",
                    package.metadata.package_json().display().display()
                )));
            }
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
        if !subpath.ends_with('/') && !subpath.contains('*') {
            if let Some(target) = table.get(subpath) {
                return self.resolve_selected_export(package, target, "", false, context);
            }
        }

        let mut expanding_keys = table
            .keys()
            .filter(|key| has_one_asterisk(key) || key.ends_with('/'))
            .map(String::as_str)
            .collect::<Vec<_>>();
        expanding_keys.sort_by(|left, right| compare_pattern_keys(left, right));

        for key in expanding_keys {
            if key.ends_with('/') && !key.contains('*') && subpath.starts_with(key) {
                let target = table
                    .get(key)
                    .expect("expanding key was collected from this table");
                return self.resolve_selected_export(
                    package,
                    target,
                    &subpath[key.len()..],
                    false,
                    context,
                );
            }
            let Some(star) = key.find('*') else {
                continue;
            };
            let prefix = &key[..star];
            let suffix = &key[star + 1..];
            if !subpath.starts_with(prefix) || !subpath.ends_with(suffix) {
                continue;
            }
            if subpath.len() < prefix.len() + suffix.len() {
                return Err(ResolutionError::unsupported(
                    "overlapping-package-exports-pattern",
                    format!(
                        "specifier {subpath} overlaps the prefix and suffix of exports key {key}"
                    ),
                ));
            }
            let capture = &subpath[prefix.len()..subpath.len() - suffix.len()];
            let target = table
                .get(key)
                .expect("expanding key was collected from this table");
            return self.resolve_selected_export(package, target, capture, true, context);
        }
        Ok(Search::Continue)
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
                    let Some(target) = expand_imports_bare_target(raw_target, subpath, pattern)
                    else {
                        return Ok(Search::Continue);
                    };
                    let outcome =
                        self.resolve_non_relative(&package.root, &target, context.mode)?;
                    return Ok(match outcome {
                        ResolutionOutcome::Resolved(mut module) => {
                            // A package-imports rewrite is owned by the source
                            // package even when its target traverses
                            // node_modules. Upstream retains the nested path,
                            // package id, and original path but clears external
                            // provenance at the outer imports boundary.
                            module.is_external_library_import = false;
                            Search::Terminal(ResolutionOutcome::Resolved(module))
                        }
                        ResolutionOutcome::NotFound => Search::Continue,
                    });
                }
                let Some(target) = expand_export_target(raw_target, subpath, pattern) else {
                    return Ok(Search::Continue);
                };
                let candidate = normalize_absolute_path(
                    Path::new(&join_normalized(&package.root, &target)),
                    None,
                )?;
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
                for (condition, target) in conditions {
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
                    return Ok(Search::Terminal(ResolutionOutcome::NotFound));
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
        let plan = match context.pass {
            ExtensionProbePass::Declaration => declaration_extension_probe_plan(target),
            ExtensionProbePass::DeclarationPackageField => {
                declaration_package_field_probe_plan(target)
            }
            _ => extension_probe_plan(target, self.options.resolve_json_module_effective()),
        };
        let (base, probes, preferred_len) = match plan {
            Ok(plan) => plan,
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension"
                    && matches!(context.pass, ExtensionProbePass::Declaration)
                    && !self.is_node_esm_mode(context.mode)
                    && !base_name(target).contains('.') =>
            {
                (target, DTS_PROBES, 1)
            }
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension"
                    && context.mode == ResolutionMode::EsNext
                    && !base_name(target).contains('.') =>
            {
                return Ok(ResolutionOutcome::NotFound);
            }
            Err(error) => return Err(error),
        };

        let probes = match context.pass {
            ExtensionProbePass::All => probes,
            ExtensionProbePass::Preferred => &probes[..preferred_len],
            ExtensionProbePass::Declaration => probes,
            ExtensionProbePass::DeclarationPackageField => &probes[..preferred_len],
            ExtensionProbePass::Fallback => &probes[preferred_len..],
        };
        let target_directory = directory_name(target);
        if !self.host.directory_exists(Path::new(&target_directory))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        for (extension, suffix) in probes {
            let candidate = format!("{base}{suffix}");
            if self.host.file_exists(Path::new(&candidate))? {
                let resolved_using_ts_extension = target.ends_with(suffix)
                    && is_typescript_module_extension(extension)
                    && raw_package_target.is_some_and(|raw| !raw.ends_with(suffix));
                return self.finish_resolution(
                    package,
                    &candidate,
                    extension.clone(),
                    context.is_external_library_import,
                    attach_package_id,
                    resolved_using_ts_extension,
                );
            }
        }
        Ok(ResolutionOutcome::NotFound)
    }

    /// tsc-port: withPackageId @6.0.3
    /// tsc-hash: 714c67b6e906e185d5b4f85b128147b60ec24d8a1bd1c82b386103fc5ddf3eb0
    /// tsc-span: _tsc.js:39824-39838
    fn finish_resolution(
        &self,
        package: &CachedPackage,
        lexical_path: &str,
        extension: ModuleExtension,
        is_external_library_import: bool,
        attach_package_id: bool,
        resolved_using_ts_extension: bool,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        self.finish_legacy_resolution(
            Some(package),
            lexical_path,
            extension,
            LegacyResolutionContext {
                is_external_library_import,
                attach_package_id,
                resolved_using_ts_extension,
                /* package-map external results follow realpath */
                follow_realpath: is_external_library_import,
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
        let (resolved_file, original_path) = if context.follow_realpath {
            self.realpath_program_path(lexical_path)?
        } else {
            (self.program_path(lexical_path)?, None)
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
        }))
    }

    fn realpath_program_path(
        &self,
        lexical_path: &str,
    ) -> Result<(ProgramPath, Option<ProgramPath>), ResolutionError> {
        let lexical = self.program_path(lexical_path)?;
        let real_path = self
            .host
            .realpath(Path::new(lexical_path))?
            .ok_or_else(|| {
                ResolutionError::invalid_data(format!(
                    "host reported {} as a file but returned no realpath",
                    Path::new(lexical_path).display()
                ))
            })?;
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
}

fn preserve_trailing_directory_separator(mut normalized: String, source: &str) -> String {
    if source.ends_with('/') && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn validate_and_clone_paths(
    paths: Option<&[PathMapping]>,
) -> Result<Option<Vec<PathMapping>>, ResolutionError> {
    let Some(paths) = paths else {
        return Ok(None);
    };
    let mut patterns = BTreeSet::new();
    for mapping in paths {
        let pattern = mapping.pattern();
        validate_owned_path_text(pattern, "paths pattern", /* allow_empty */ false)?;
        if pattern.matches('*').count() > 1 {
            return Err(ResolutionError::invalid_data(format!(
                "paths pattern {pattern:?} contains more than one '*'"
            )));
        }
        if !patterns.insert(pattern.to_owned()) {
            return Err(ResolutionError::invalid_data(format!(
                "duplicate paths pattern {pattern:?} has no object-equivalent ordering semantics"
            )));
        }
        if mapping.substitutions().is_empty() {
            return Err(ResolutionError::invalid_data(format!(
                "paths pattern {pattern:?} has no substitutions"
            )));
        }
        for substitution in mapping.substitutions() {
            validate_owned_path_text(
                substitution,
                "paths substitution",
                /* allow_empty */ true,
            )?;
            if substitution.matches('*').count() > 1 {
                return Err(ResolutionError::invalid_data(format!(
                    "paths substitution {substitution:?} for pattern {pattern:?} contains more than one '*'"
                )));
            }
        }
    }
    Ok(Some(paths.to_vec()))
}

fn validate_owned_path_text(
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
    object
        .get(field)
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

fn normalize_package_target(
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
        ExtensionProbePass::All
        | ExtensionProbePass::Preferred
        | ExtensionProbePass::Declaration
        | ExtensionProbePass::DeclarationPackageField => package
            .typings
            .as_deref()
            .or(package.types.as_deref())
            .or(package.main.as_deref()),
        ExtensionProbePass::Fallback => package.main.as_deref(),
    }
}

fn package_version_logical_name(
    package: &CachedPackage,
    probe_pass: ExtensionProbePass,
) -> Option<String> {
    selected_package_entry_field(package, probe_pass)
        .map(|field| field.strip_prefix("./").unwrap_or(field).to_owned())
}

fn parse_package_request(specifier: &str) -> Result<PackageRequest<'_>, ResolutionError> {
    if specifier.is_empty()
        || is_relative_specifier(specifier)
        || specifier.starts_with(['/', '\\'])
        || specifier.contains(['\\', '\0', ':'])
    {
        return Err(ResolutionError::unsupported(
            "non-bare-module-specifier",
            format!("the H0.2b exports resolver cannot resolve {specifier:?}"),
        ));
    }

    let package_end = if specifier.starts_with('@') {
        let scope_end = specifier.find('/').ok_or_else(|| {
            ResolutionError::invalid_data(format!("invalid scoped package specifier {specifier:?}"))
        })?;
        if scope_end == 1 {
            return Err(ResolutionError::invalid_data(format!(
                "invalid scoped package specifier {specifier:?}"
            )));
        }
        let package_start = scope_end + 1;
        let package_tail = &specifier[package_start..];
        let relative_end = package_tail.find('/').unwrap_or(package_tail.len());
        if relative_end == 0 {
            return Err(ResolutionError::invalid_data(format!(
                "invalid scoped package specifier {specifier:?}"
            )));
        }
        package_start + relative_end
    } else {
        specifier.find('/').unwrap_or(specifier.len())
    };

    let package_name = &specifier[..package_end];
    let has_subpath_separator = package_end < specifier.len();
    let rest = specifier
        .get(package_end + usize::from(has_subpath_separator)..)
        .unwrap_or("");
    if package_name.is_empty()
        || (has_subpath_separator && rest.is_empty())
        || (!rest.is_empty()
            && rest
                .split('/')
                .any(|part| matches!(part, "." | "..") || part.is_empty()))
    {
        return Err(ResolutionError::invalid_data(format!(
            "invalid package specifier {specifier:?}"
        )));
    }

    Ok(PackageRequest {
        package_name,
        exports_subpath: if rest.is_empty() {
            ".".to_owned()
        } else {
            format!("./{rest}")
        },
    })
}

fn is_relative_specifier(specifier: &str) -> bool {
    is_path_relative_specifier(specifier) || is_supported_rooted_specifier(specifier)
}

fn is_path_relative_specifier(specifier: &str) -> bool {
    matches!(specifier, "." | "..") || specifier.starts_with("./") || specifier.starts_with("../")
}

fn is_supported_rooted_specifier(specifier: &str) -> bool {
    (specifier.starts_with('/') && !specifier.starts_with("//"))
        || (specifier.len() >= 3
            && specifier.as_bytes()[0].is_ascii_alphabetic()
            && specifier.as_bytes()[1] == b':'
            && specifier.as_bytes()[2] == b'/')
}

fn has_node_directory_spelling(specifier: &str) -> bool {
    specifier.ends_with('/') || matches!(specifier.rsplit('/').next(), Some("." | ".."))
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
    let remainder = &path[package_start..];
    let first_separator = remainder.find('/')?;
    if first_separator == 0 {
        return None;
    }
    let package_end = if remainder.starts_with('@') {
        let scoped_remainder = &remainder[first_separator + 1..];
        let second_separator = scoped_remainder.find('/')?;
        if second_separator == 0 {
            return None;
        }
        package_start + first_separator + 1 + second_separator
    } else {
        package_start + first_separator
    };
    Some(path[..package_end].to_owned())
}

fn package_id_for_legacy_path(
    package: &CachedPackage,
    lexical_path: &str,
    attach_package_id: bool,
) -> Result<Option<PackageId>, ResolutionError> {
    let (Some(name), Some(version)) = (package.metadata.name(), package.metadata.version()) else {
        return Ok(None);
    };
    if !attach_package_id {
        return Ok(None);
    }
    let submodule_name = lexical_path
        .strip_prefix(&package.root)
        // withPackageId slices one character after the package directory even
        // for the sibling-file shape `pkg.ts`.
        .map(|path| path.get(1..).unwrap_or(""))
        .ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "resolved path {lexical_path} is outside package {}",
                package.root
            ))
        })?;
    Ok(Some(PackageId::new(name, submodule_name, version)))
}

fn arbitrary_declaration_twin(candidate: &str) -> Option<(String, String)> {
    let file_name = base_name(candidate);
    let dot = file_name.rfind('.')?;
    let original_extension = &file_name[dot..];
    let base = candidate.get(..candidate.len() - original_extension.len())?;
    let extension = format!(".d{original_extension}.ts");
    Some((format!("{base}{extension}"), extension))
}

fn extension_probe_plan(
    target: &str,
    resolve_json_module: bool,
) -> Result<ExtensionProbePlan<'_>, ResolutionError> {
    let plan = if let Some(base) = target.strip_suffix(".d.cts") {
        (base, DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".d.mts") {
        (base, DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".d.ts") {
        (base, DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".cjs") {
        (base, CJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".mjs") {
        (base, MJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".jsx") {
        (base, JSX_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".js") {
        (base, JS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".tsx") {
        (base, TSX_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".ts") {
        (base, TS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".mts") {
        (base, MTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".cts") {
        (base, CTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".json") {
        if resolve_json_module {
            (base, JSON_PROBES, 1)
        } else {
            (base, JSON_DISABLED_PROBES, 4)
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
        (base, DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".d.mts") {
        (base, DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".d.ts") {
        (base, DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".cjs") {
        (base, DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".cts") {
        (base, DCTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".mjs") {
        (base, DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".mts") {
        (base, DMTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".jsx") {
        (base, DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".tsx") {
        (base, DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".js") {
        (base, DTS_PROBES, 1)
    } else if let Some(base) = target.strip_suffix(".ts") {
        (base, DTS_PROBES, 1)
    } else {
        return Err(ResolutionError::unsupported(
            "module-target-extension",
            format!("target has no supported written declaration extension: {target}"),
        ));
    };
    Ok(plan)
}

/// The declaration directory loader treats a package-json entry differently
/// from an ordinary declaration probe: a written extension first receives its
/// declaration twin, followed by the TypeScript + declaration expansion.
fn declaration_package_field_probe_plan(
    target: &str,
) -> Result<ExtensionProbePlan<'_>, ResolutionError> {
    let plan = if let Some(base) = target.strip_suffix(".d.cts") {
        (base, DECLARATION_PACKAGE_CJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".d.mts") {
        (base, DECLARATION_PACKAGE_MJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".d.ts") {
        (base, DECLARATION_PACKAGE_JS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".cjs") {
        (base, DECLARATION_PACKAGE_CJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".cts") {
        (base, DECLARATION_PACKAGE_CJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".mjs") {
        (base, DECLARATION_PACKAGE_MJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".mts") {
        (base, DECLARATION_PACKAGE_MJS_PROBES, 2)
    } else if let Some(base) = target.strip_suffix(".jsx") {
        (base, DECLARATION_PACKAGE_JSX_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".tsx") {
        (base, DECLARATION_PACKAGE_JSX_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".js") {
        (base, DECLARATION_PACKAGE_JS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".ts") {
        (base, DECLARATION_PACKAGE_JS_PROBES, 3)
    } else if let Some(base) = target.strip_suffix(".json") {
        (base, DJSON_PROBES, 1)
    } else if !base_name(target).contains('.') {
        (target, JS_PROBES, 3)
    } else {
        return Err(ResolutionError::unsupported(
            "module-target-extension",
            format!("package field has no supported written extension: {target}"),
        ));
    };
    Ok(plan)
}

/// tsrs-native: the recognized-extension projection consumed by the
/// typesVersions exact-substitution probe.
fn recognized_module_extension(path: &str) -> Option<ModuleExtension> {
    if path.ends_with(".d.ts") {
        Some(ModuleExtension::Dts)
    } else if path.ends_with(".d.mts") {
        Some(ModuleExtension::Dmts)
    } else if path.ends_with(".d.cts") {
        Some(ModuleExtension::Dcts)
    } else if path.ends_with(".mjs") {
        Some(ModuleExtension::Mjs)
    } else if path.ends_with(".mts") {
        Some(ModuleExtension::Mts)
    } else if path.ends_with(".cjs") {
        Some(ModuleExtension::Cjs)
    } else if path.ends_with(".cts") {
        Some(ModuleExtension::Cts)
    } else if path.ends_with(".ts") {
        Some(ModuleExtension::Ts)
    } else if path.ends_with(".js") {
        Some(ModuleExtension::Js)
    } else if path.ends_with(".tsx") {
        Some(ModuleExtension::Tsx)
    } else if path.ends_with(".jsx") {
        Some(ModuleExtension::Jsx)
    } else if path.ends_with(".json") {
        Some(ModuleExtension::Json)
    } else {
        None
    }
}

fn select_types_versions_mapping<'a, 'b>(
    table: &'a Map<String, Value>,
    request: &'b str,
) -> Option<(&'a str, &'b str, &'a Value)> {
    if let Some((key, targets)) = table.get_key_value(request) {
        return Some((key.as_str(), "", targets));
    }
    let mut patterns = table
        .keys()
        .filter(|key| has_one_asterisk(key))
        .map(String::as_str)
        .collect::<Vec<_>>();
    patterns.sort_by(|left, right| compare_pattern_keys(left, right));
    for pattern in patterns {
        let star = pattern
            .find('*')
            .expect("typesVersions pattern was filtered to one asterisk");
        let prefix = &pattern[..star];
        let suffix = &pattern[star + 1..];
        if request.starts_with(prefix)
            && request.ends_with(suffix)
            && request.len() >= prefix.len() + suffix.len()
        {
            let capture = &request[prefix.len()..request.len() - suffix.len()];
            return Some((
                pattern,
                capture,
                table
                    .get(pattern)
                    .expect("typesVersions pattern belongs to this table"),
            ));
        }
    }
    None
}

fn mangle_scoped_package_name(package_name: &str) -> String {
    match package_name.strip_prefix('@') {
        Some(scoped) => scoped.replacen('/', "__", 1),
        None => package_name.to_owned(),
    }
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

fn expand_export_target(target: &str, subpath: &str, pattern: bool) -> Option<String> {
    let target = target.strip_prefix("./")?;
    if target.contains(['\\', '\0'])
        || subpath.contains(['\\', '\0'])
        || contains_forbidden_package_segment(target)
        || contains_forbidden_package_segment(subpath)
        || (!pattern && target.contains('*'))
    {
        return None;
    }
    Some(if pattern {
        target.replace('*', subpath)
    } else {
        format!("{target}{subpath}")
    })
}

fn expand_imports_bare_target(target: &str, subpath: &str, pattern: bool) -> Option<String> {
    if target.is_empty()
        || target.starts_with("../")
        || target.starts_with(['/', '\\'])
        || target.contains(['\\', '\0', ':'])
        || subpath.contains(['\\', '\0'])
        || (!pattern && target.contains('*'))
    {
        return None;
    }
    Some(if pattern {
        target.replace('*', subpath)
    } else {
        format!("{target}{subpath}")
    })
}

fn contains_forbidden_package_segment(path: &str) -> bool {
    path.split('/')
        .any(|part| matches!(part, "." | ".." | "node_modules"))
}

fn path_is_within(path: &str, directory: &str) -> bool {
    path == directory
        || (path.starts_with(directory)
            && path.as_bytes().get(directory.len()).copied() == Some(b'/'))
        || directory == "/"
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
    let text = path.to_str().ok_or_else(|| {
        ResolutionError::canonicalization(Some(path.to_path_buf()), "path is not valid Unicode")
    })?;
    if text.is_empty() || text.contains('\0') {
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
        join_normalized(base, &slashed)
    };
    normalize_rooted_text(&absolute)
        .map_err(|detail| ResolutionError::canonicalization(Some(path.to_path_buf()), detail))
}

fn is_normalized_rooted_text(path: &str) -> bool {
    path.starts_with('/')
        || (path.len() >= 3
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'
            && path.as_bytes()[2] == b'/')
}

fn normalize_rooted_text(path: &str) -> Result<String, &'static str> {
    let (root, tail) = if let Some(tail) = path.strip_prefix('/') {
        ("/".to_owned(), tail)
    } else if path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'/'
    {
        (path[..3].to_owned(), &path[3..])
    } else {
        return Err("path has no supported absolute root");
    };

    let mut components = Vec::new();
    for component in tail.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            component => components.push(component),
        }
    }
    if components.is_empty() {
        return Ok(root);
    }
    let separator = if root.ends_with('/') { "" } else { "/" };
    Ok(format!("{root}{separator}{}", components.join("/")))
}

fn join_normalized(parent: &str, child: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{}", child.trim_start_matches('/'))
    } else {
        format!("{parent}/{}", child.trim_start_matches('/'))
    }
}

pub(crate) fn directory_name(path: &str) -> String {
    if path == "/" || is_drive_root(path) {
        return path.to_owned();
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(2) if trimmed.as_bytes().get(1) == Some(&b':') => trimmed[..=2].to_owned(),
        Some(index) => trimmed[..index].to_owned(),
        None => trimmed.to_owned(),
    }
}

fn base_name(path: &str) -> &str {
    if path == "/" || is_drive_root(path) {
        ""
    } else {
        path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
    }
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

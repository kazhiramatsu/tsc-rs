use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{Map, Value};
use tsc_host::{to_file_name_lower_case, CompilerHost};
use tsc_types::CompilerOptions;

use crate::path::ProgramPath;
use crate::prepared::{PackageJsonType, PackageMetadata, PathContext};
use crate::resolution::{
    ModuleExtension, PackageId, ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModule,
    ResolvedModuleTarget,
};

/// Filesystem-derived module facts that have not yet been bound to a program
/// source id.
///
/// The package metadata is retained because the loader needs the same decoded
/// `package.json` observation for package scopes and implied node formats. It
/// is reference-counted internally, so repeated resolutions do not copy the
/// decoded JSON text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostResolvedModule {
    resolved_file: ProgramPath,
    extension: ModuleExtension,
    original_path: Option<ProgramPath>,
    is_external_library_import: bool,
    resolved_using_ts_extension: bool,
    package_id: Option<PackageId>,
    package_metadata: Rc<PackageMetadata>,
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

    pub fn package_metadata(&self) -> &PackageMetadata {
        &self.package_metadata
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

#[derive(Clone, Debug)]
struct CachedPackage {
    root: String,
    exports: Option<Value>,
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

/// A selected export is terminal within its package map even when its target
/// is `null` or its file probes miss. `Continue` is reserved for an
/// inapplicable self-reference or an export table with no selected key. An
/// external node_modules pass may still continue to `@types` or an outer
/// ancestor, but it never tries a broader key or a legacy entry in the same
/// package.
enum Search<T> {
    Continue,
    Terminal(ResolutionOutcome<T>),
}

#[derive(Clone, Copy)]
enum ExtensionProbePass {
    All,
    Preferred,
    Fallback,
}

#[derive(Clone, Copy)]
struct ExportProbeContext {
    is_external_library_import: bool,
    pass: ExtensionProbePass,
}

/// Sequential Node16/NodeNext/Bundler resolver for the H0.2b package-exports
/// slice.
///
/// One resolver owns a per-run `package.json` cache. Host methods remain
/// fallible and are never translated into ordinary lookup misses.
pub struct ModuleResolver<'a> {
    host: &'a dyn CompilerHost,
    options: &'a CompilerOptions,
    path_context: PathContext,
    package_cache: BTreeMap<String, PackageCacheEntry>,
}

impl<'a> ModuleResolver<'a> {
    /// Construct the resolver from the host's exact path profile.
    ///
    /// tsrs-native: establishes the shared program-layer path context.
    pub fn new(
        host: &'a dyn CompilerHost,
        options: &'a CompilerOptions,
    ) -> Result<Self, ResolutionError> {
        let current_directory = host.current_directory()?;
        let normalized = normalize_absolute_path(&current_directory, None)?;
        let case_sensitive = host.use_case_sensitive_file_names();
        let current_directory = make_program_path(&normalized, case_sensitive)?;
        Ok(Self {
            host,
            options,
            path_context: PathContext::new(current_directory, case_sensitive),
            package_cache: BTreeMap::new(),
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
        Ok(Self {
            host,
            options,
            path_context,
            package_cache: BTreeMap::new(),
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
        self.validate_supported_configuration(mode)?;
        let current_directory = self.current_directory_text()?;
        let containing_file = normalize_absolute_path(containing_file, Some(current_directory))?;
        let containing_directory = directory_name(&containing_file);
        let request = parse_package_request(specifier)?;

        if let Search::Terminal(outcome) =
            self.try_self_reference(&containing_directory, &request)?
        {
            return Ok(outcome);
        }

        self.resolve_from_node_modules(&containing_directory, &request)
    }

    fn validate_supported_configuration(
        &self,
        _mode: ResolutionMode,
    ) -> Result<(), ResolutionError> {
        let resolution_kind = self.options.emit_module_resolution_kind();
        if !matches!(resolution_kind, 3 | 99 | 100) {
            return Err(ResolutionError::unsupported(
                "module-resolution-kind",
                format!(
                    "package exports are implemented only for Node16, NodeNext, and Bundler; got {resolution_kind}"
                ),
            ));
        }
        if self.options.resolve_package_json_exports == Some(false) {
            return Err(ResolutionError::unsupported(
                "resolve-package-json-exports-disabled",
                "the H0.2b resolver owns only the package-exports branch",
            ));
        }
        if self.options.no_dts_resolution == Some(true) {
            return Err(ResolutionError::unsupported(
                "no-dts-resolution",
                "implementation-only exports probing is outside the H0.2b slice",
            ));
        }
        if self.options.base_url.is_some() {
            return Err(ResolutionError::unsupported(
                "base-url-before-package-exports",
                "baseUrl candidates must be resolved before node_modules lookup",
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
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        let Some(package) = self.find_nearest_package_scope(containing_directory)? else {
            return Ok(Search::Continue);
        };
        if package.metadata.name() != Some(request.package_name) || package.exports.is_none() {
            return Ok(Search::Continue);
        }

        let outcome = self.resolve_package_exports(
            &package,
            &request.exports_subpath,
            /* is_external_library_import */ false,
            ExtensionProbePass::All,
        )?;
        Ok(Search::Terminal(outcome))
    }

    fn resolve_from_node_modules(
        &mut self,
        containing_directory: &str,
        request: &PackageRequest<'_>,
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
                if self.host.directory_exists(Path::new(&package_root))? {
                    let package_json = join_normalized(&package_root, "package.json");
                    let Some(package) = self.load_package(&package_json)? else {
                        return Err(ResolutionError::unsupported(
                            "legacy-node-package-entry",
                            format!(
                                "{} is an existing package directory without package.json",
                                Path::new(&package_root).display()
                            ),
                        ));
                    };
                    if package.exports.is_none() {
                        return Err(ResolutionError::unsupported(
                            "legacy-node-package-entry",
                            format!(
                                "{} has no exports field",
                                package.metadata.package_json().display().display()
                            ),
                        ));
                    }
                    let outcome = self.resolve_package_exports(
                        &package,
                        &request.exports_subpath,
                        /* is_external_library_import */ true,
                        probe_pass,
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
                        if self.host.directory_exists(Path::new(&types_package))? {
                            return Err(ResolutionError::unsupported(
                                "node-modules-at-types-fallback",
                                format!(
                                    "{} may satisfy {specifier:?} through the unported @types fallback",
                                    Path::new(&types_package).display(),
                                    specifier = request.package_name,
                                ),
                            ));
                        }
                    }
                }
            }
        }
        Ok(ResolutionOutcome::NotFound)
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
        let bytes = self.host.read_file(package_json_path)?.ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "host reported {} as a file but returned no contents",
                package_json_path.display()
            ))
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|error| {
            ResolutionError::invalid_data(format!(
                "{} is not UTF-8: {error}",
                Path::new(package_json).display()
            ))
        })?;
        let json_text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let value: Value = serde_json::from_str(json_text).map_err(|error| {
            ResolutionError::invalid_data(format!(
                "cannot parse {}: {error}",
                Path::new(package_json).display()
            ))
        })?;
        let object = value.as_object().ok_or_else(|| {
            ResolutionError::invalid_data(format!(
                "{} must contain a JSON object",
                Path::new(package_json).display()
            ))
        })?;

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
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
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
                ".",
                ExportProbeContext {
                    is_external_library_import,
                    pass: probe_pass,
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
                    return Err(ResolutionError::unsupported(
                        "conditional-package-exports",
                        format!(
                            "{} exports selects through a condition object",
                            package.metadata.package_json().display().display()
                        ),
                    ));
                }
                self.search_exports_table(
                    package,
                    table,
                    subpath,
                    is_external_library_import,
                    probe_pass,
                )?
            }
            Value::Array(_) => {
                return Err(ResolutionError::unsupported(
                    "package-exports-array",
                    format!(
                        "{} uses an exports fallback array",
                        package.metadata.package_json().display().display()
                    ),
                ));
            }
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

        Ok(match search {
            // A present exports map suppresses every legacy package fallback.
            Search::Continue => ResolutionOutcome::NotFound,
            Search::Terminal(outcome) => outcome,
        })
    }

    /// tsc-port: loadModuleFromExportsOrImports @6.0.3
    /// tsc-hash: a0b5d92673856e6203a41bb797a6a332ed2fd142e7777d6ad148f19dd189af4e
    /// tsc-span: _tsc.js:41600-41654
    fn search_exports_table(
        &mut self,
        package: &CachedPackage,
        table: &Map<String, Value>,
        subpath: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        if !subpath.ends_with('/') && !subpath.contains('*') {
            if let Some(target) = table.get(subpath) {
                return self.resolve_selected_export(
                    package,
                    target,
                    "",
                    false,
                    subpath,
                    ExportProbeContext {
                        is_external_library_import,
                        pass: probe_pass,
                    },
                );
            }
        }

        for key in table.keys().filter(|key| key.matches('*').count() > 1) {
            let first = key.find('*').expect("filtered to keys containing a star");
            let last = key.rfind('*').expect("filtered to keys containing a star");
            let prefix = &key[..first];
            let suffix = &key[last + 1..];
            if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                return Err(ResolutionError::unsupported(
                    "multi-star-package-exports-key",
                    format!("specifier {subpath} may select exports key {key}"),
                ));
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
                return Err(ResolutionError::unsupported(
                    "deprecated-package-exports-directory-key",
                    format!("matched exports key {key}"),
                ));
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
            return self.resolve_selected_export(
                package,
                target,
                capture,
                true,
                key,
                ExportProbeContext {
                    is_external_library_import,
                    pass: probe_pass,
                },
            );
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
        key: &str,
        context: ExportProbeContext,
    ) -> Result<Search<HostResolvedModule>, ResolutionError> {
        match target {
            Value::Null => Ok(Search::Terminal(ResolutionOutcome::NotFound)),
            Value::String(target) => {
                let Some(target) = expand_export_target(target, subpath, pattern) else {
                    return Ok(Search::Terminal(ResolutionOutcome::NotFound));
                };
                let candidate = normalize_absolute_path(
                    Path::new(&join_normalized(&package.root, &target)),
                    None,
                )?;
                if !path_is_within(&candidate, &package.root) {
                    return Ok(Search::Terminal(ResolutionOutcome::NotFound));
                }
                let resolved = self.probe_export_target(
                    package,
                    &candidate,
                    context.is_external_library_import,
                    context.pass,
                )?;
                Ok(Search::Terminal(resolved))
            }
            Value::Object(_) => Err(ResolutionError::unsupported(
                "conditional-package-exports",
                format!("exports key {key} selects through a condition object"),
            )),
            Value::Array(_) => Err(ResolutionError::unsupported(
                "package-exports-array",
                format!("exports key {key} selects through a fallback array"),
            )),
            Value::Bool(_) | Value::Number(_) => Ok(Search::Terminal(ResolutionOutcome::NotFound)),
        }
    }

    /// tsc-port: tryAddingExtensions @6.0.3
    /// tsc-hash: ce6ede18162cf6d430e57f3971a4f9018eca6a56b790d28b7d46ab3e6310ec2b
    /// tsc-span: _tsc.js:41196-41229
    fn probe_export_target(
        &self,
        package: &CachedPackage,
        target: &str,
        is_external_library_import: bool,
        probe_pass: ExtensionProbePass,
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let (base, probes, preferred_len): (&str, &[(ModuleExtension, &str)], usize) =
            if let Some(base) = target.strip_suffix(".cjs") {
                (
                    base,
                    &[
                        (ModuleExtension::Cts, ".cts"),
                        (ModuleExtension::Dcts, ".d.cts"),
                        (ModuleExtension::Cjs, ".cjs"),
                    ],
                    2,
                )
            } else if let Some(base) = target.strip_suffix(".mjs") {
                (
                    base,
                    &[
                        (ModuleExtension::Mts, ".mts"),
                        (ModuleExtension::Dmts, ".d.mts"),
                        (ModuleExtension::Mjs, ".mjs"),
                    ],
                    2,
                )
            } else if let Some(base) = target.strip_suffix(".js") {
                (
                    base,
                    &[
                        (ModuleExtension::Ts, ".ts"),
                        (ModuleExtension::Tsx, ".tsx"),
                        (ModuleExtension::Dts, ".d.ts"),
                        (ModuleExtension::Js, ".js"),
                        (ModuleExtension::Jsx, ".jsx"),
                    ],
                    3,
                )
            } else {
                return Err(ResolutionError::unsupported(
                    "package-exports-target-extension",
                    format!("only .cjs, .mjs, and .js targets are implemented: {target}"),
                ));
            };

        let probes = match probe_pass {
            ExtensionProbePass::All => probes,
            ExtensionProbePass::Preferred => &probes[..preferred_len],
            ExtensionProbePass::Fallback => &probes[preferred_len..],
        };
        let target_directory = directory_name(target);
        if !self.host.directory_exists(Path::new(&target_directory))? {
            return Ok(ResolutionOutcome::NotFound);
        }
        for (extension, suffix) in probes {
            let candidate = format!("{base}{suffix}");
            if self.host.file_exists(Path::new(&candidate))? {
                return self.finish_resolution(
                    package,
                    &candidate,
                    extension.clone(),
                    is_external_library_import,
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
    ) -> Result<ResolutionOutcome<HostResolvedModule>, ResolutionError> {
        let lexical = self.program_path(lexical_path)?;
        let (resolved_file, original_path) = if is_external_library_import {
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
                (lexical, None)
            } else {
                (real, Some(lexical))
            }
        } else {
            (lexical, None)
        };

        let package_id = match (package.metadata.name(), package.metadata.version()) {
            (Some(name), Some(version)) => {
                let submodule_name = lexical_path
                    .strip_prefix(&package.root)
                    .and_then(|path| path.strip_prefix('/'))
                    .ok_or_else(|| {
                        ResolutionError::invalid_data(format!(
                            "resolved path {lexical_path} is outside package {}",
                            package.root
                        ))
                    })?;
                Some(PackageId::new(name, submodule_name, version))
            }
            _ => None,
        };

        Ok(ResolutionOutcome::Resolved(HostResolvedModule {
            resolved_file,
            extension,
            original_path,
            is_external_library_import,
            resolved_using_ts_extension: false,
            package_id,
            package_metadata: Rc::clone(&package.metadata),
        }))
    }

    fn program_path(&self, normalized_path: &str) -> Result<ProgramPath, ResolutionError> {
        make_program_path(
            normalized_path,
            self.path_context.use_case_sensitive_file_names(),
        )
    }
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

fn parse_package_request(specifier: &str) -> Result<PackageRequest<'_>, ResolutionError> {
    if specifier.is_empty()
        || specifier.starts_with(['.', '/', '\\', '#'])
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

fn make_program_path(
    normalized_path: &str,
    case_sensitive: bool,
) -> Result<ProgramPath, ResolutionError> {
    let canonical = canonical_text(normalized_path, case_sensitive);
    ProgramPath::from_trusted_parts(normalized_path, canonical).map_err(|error| {
        ResolutionError::canonicalization(Some(PathBuf::from(normalized_path)), error.to_string())
    })
}

fn normalize_absolute_path(path: &Path, base: Option<&str>) -> Result<String, ResolutionError> {
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

fn directory_name(path: &str) -> String {
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

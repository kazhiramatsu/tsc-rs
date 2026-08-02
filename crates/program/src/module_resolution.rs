use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{Map, Value};
use tsc_host::{to_file_name_lower_case, CompilerHost};
use tsc_types::{compiler_version_satisfies, CompilerOptions};

use crate::path::ProgramPath;
use crate::prepared::{PackageJsonType, PackageMetadata, PathContext};
use crate::resolution::{
    ModuleExtension, PackageId, ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModule,
    ResolvedModuleTarget,
};

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

#[derive(Clone, Copy)]
struct ExportProbeContext {
    is_external_library_import: bool,
    pass: ExtensionProbePass,
    mode: ResolutionMode,
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

/// Sequential Node16/NodeNext/Bundler resolver for the H0.2 package-map
/// slices.
///
/// One resolver owns a per-run `package.json` cache. Host methods remain
/// fallible and are never translated into ordinary lookup misses.
pub struct ModuleResolver<'a> {
    host: &'a dyn CompilerHost,
    options: &'a CompilerOptions,
    path_context: PathContext,
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
        let current_directory = host.current_directory()?;
        let normalized = normalize_absolute_path(&current_directory, None)?;
        let case_sensitive = host.use_case_sensitive_file_names();
        let current_directory = make_program_path(&normalized, case_sensitive)?;
        Ok(Self {
            host,
            options,
            path_context: PathContext::new(current_directory, case_sensitive),
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
        Ok(Self {
            host,
            options,
            path_context,
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
        self.validate_supported_configuration(mode)?;
        let current_directory = self.current_directory_text()?;
        let containing_file = normalize_absolute_path(containing_file, Some(current_directory))?;
        let containing_directory = directory_name(&containing_file);
        if is_relative_specifier(specifier) {
            return self.resolve_relative(&containing_file, specifier, mode);
        }

        self.resolve_non_relative(&containing_directory, specifier, mode)
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
        let target = normalize_absolute_path(
            Path::new(&join_normalized(&containing_directory, specifier)),
            None,
        )?;
        let package = self.find_nearest_package_scope(&directory_name(&target))?;
        let external = path_contains_node_modules(&target);
        let directory_spelling = specifier.ends_with('/') || matches!(specifier, "." | "..");
        let allow_implicit = !self.is_node_esm_mode(mode);

        for probe_pass in [ExtensionProbePass::Preferred, ExtensionProbePass::Fallback] {
            if !directory_spelling {
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

            if !allow_implicit || !self.host.directory_exists(Path::new(&target))? {
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
            let outcome = self.probe_legacy_path(
                Some(package),
                &candidate,
                probe_pass,
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
        if node_esm && !matches!(package.exports.as_ref(), None | Some(Value::Null)) {
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
            let outcome = self.probe_legacy_path(
                Some(package),
                &candidate,
                probe_pass,
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
        let (base, probes, preferred_len) = match extension_probe_plan(candidate) {
            Ok(plan) => plan,
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension"
                    && allow_implicit
                    && !base_name(candidate).contains('.') =>
            {
                (candidate, JS_PROBES, 3)
            }
            Err(ResolutionError::Unsupported { feature, .. })
                if feature == "module-target-extension" =>
            {
                return Ok(ResolutionOutcome::NotFound);
            }
            Err(error) => return Err(error),
        };
        let probes = match probe_pass {
            ExtensionProbePass::All => probes,
            ExtensionProbePass::Preferred => &probes[..preferred_len],
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
            imports: object.get("imports").cloned(),
            types_versions: object.get("typesVersions").cloned(),
            typings: non_empty_string_field(object, "typings"),
            types: non_empty_string_field(object, "types"),
            main: non_empty_string_field(object, "main"),
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
        )?;
        Ok(match search {
            // A present exports map suppresses every legacy package fallback.
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
                    if !self.package_condition_matches(condition, context.mode) {
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
    fn package_condition_matches(&self, condition: &str, mode: ResolutionMode) -> bool {
        if condition == "default" {
            return true;
        }
        let resolution_kind = self.options.emit_module_resolution_kind();
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
        let (base, probes, preferred_len) = match extension_probe_plan(target) {
            Ok(plan) => plan,
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
        let lexical = self.program_path(lexical_path)?;
        let (resolved_file, original_path) =
            if context.is_external_library_import && context.follow_realpath {
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

        let package_id = match package
            .map(|package| (package, package.metadata.name(), package.metadata.version()))
        {
            Some((package, Some(name), Some(version))) if context.attach_package_id => {
                let submodule_name = lexical_path
                    .strip_prefix(&package.root)
                    // withPackageId slices one character after the package
                    // directory even for the sibling-file shape `pkg.ts`.
                    .map(|path| path.get(1..).unwrap_or(""))
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
            is_external_library_import: context.is_external_library_import,
            resolved_using_ts_extension: context.resolved_using_ts_extension,
            package_id,
            alternate_result: None,
            package_metadata: package.map(|package| Rc::clone(&package.metadata)),
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
        ExtensionProbePass::All | ExtensionProbePass::Preferred => package
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
    matches!(specifier, "." | "..") || specifier.starts_with("./") || specifier.starts_with("../")
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

fn extension_probe_plan(target: &str) -> Result<ExtensionProbePlan<'_>, ResolutionError> {
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
    } else {
        return Err(ResolutionError::unsupported(
            "module-target-extension",
            format!("target has no supported written extension: {target}"),
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

//! L2.3 resolution-cache contract (isolated prototype, stage B).
//!
//! Replays `fixtures/resolution_cache/manifest.v1.json` through the
//! generation cache placed above the real `ModuleResolver`, and for every
//! generation compares three observations of every request:
//!
//! 1. the cached/reused value published by the candidate generation,
//! 2. a fresh `ModuleResolver` run on the same host state, and
//! 3. the vendored TypeScript 6.0.3 result recorded by
//!    `scripts/observe-resolution-cache.mjs` (`expected.v1.json`).
//!
//! It additionally checks the declared reuse/recompute dispositions, the
//! completeness of recorded dependencies against tsc's failed-lookup and
//! affecting locations, immutability of held generations, publish-atomicity
//! under host errors and cancellation, bounded retention with eviction, the
//! Program-side rebinding of cached rows to per-generation source ids, and
//! trace determinism. A seeded 1000-generation soak measures retention.
//!
//! Traces are written below `target/l2-3/` for the report.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{json, Map, Value};
use tsc_diagnostics::JsString;
use tsc_host::{
    to_file_name_lower_case, CompilerHost, HostError, HostErrorKind, HostOperation,
    MemoryCompilerHost,
};
use tsc_program::{
    load_no_lib_program, parse_config_root_plan, CacheEntry, CacheError, CachedValue,
    CancellationToken, ChangeBatch, CompilerConfigHost, CompilerOptions, ConfigOptionValueState,
    ConfigRootPlanRequest, ConfigTypedListElement, Dependency, DependencySet, Disposition,
    GenerationHandle, HostModuleResolution, HostResolvedModule, ModuleResolution, ModuleResolver,
    ModuleSuffix, ObservationHost, PackageJsonType, PathContext, PathKey, PathMapping,
    PreparedProgramBuilder, PreparedRoot, PreparedSourceFile, ProgramLoadLimits, ProgramOptions,
    ProgramPath, RequestKind, ResolutionCache, ResolutionMode, ResolutionOutcome,
    ResolvedModuleTarget, RetentionLimits,
};

const MANIFEST: &str = include_str!("fixtures/resolution_cache/manifest.v1.json");
const EXPECTED: &str = include_str!("fixtures/resolution_cache/expected.v1.json");

fn output_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/l2-3");
    fs::create_dir_all(&dir).expect("create target/l2-3");
    dir
}

fn write_json(name: &str, value: &Value) {
    let path = output_dir().join(name);
    fs::write(
        &path,
        serde_json::to_string_pretty(value).expect("serialize") + "\n",
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

// ---------------------------------------------------------------------------
// Virtual filesystem mirror
// ---------------------------------------------------------------------------

/// The same virtual model as the native observer: files, explicit
/// directories, and directory symlinks, keyed by the host case profile.
#[derive(Clone, Debug)]
struct VirtualFs {
    case_sensitive: bool,
    current_directory: String,
    files: BTreeMap<String, (String, String)>,
    directories: BTreeMap<String, String>,
    links: BTreeMap<String, (String, String)>,
}

/// The materialized host state: what the memory host will actually contain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Materialized {
    files: BTreeMap<String, (String, String)>,
    directories: BTreeMap<String, String>,
    realpaths: BTreeMap<String, String>,
}

fn parent_of(path: &str) -> Option<String> {
    let index = path.rfind('/')?;
    if index == 0 {
        (path.len() > 1).then(|| "/".to_owned())
    } else {
        Some(path[..index].to_owned())
    }
}

impl VirtualFs {
    fn new(current_directory: &str, case_sensitive: bool) -> Self {
        Self {
            case_sensitive,
            current_directory: current_directory.to_owned(),
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
            links: BTreeMap::new(),
        }
    }

    fn key(&self, path: &str) -> String {
        if self.case_sensitive {
            path.to_owned()
        } else {
            to_file_name_lower_case(path)
        }
    }

    fn resolve(&self, path: &str) -> String {
        let mut current = path.to_owned();
        for _ in 0..32 {
            let parts: Vec<&str> = current.split('/').collect();
            let mut prefix = String::new();
            let mut substituted = None;
            for (index, part) in parts.iter().enumerate().skip(1) {
                prefix.push('/');
                prefix.push_str(part);
                if let Some((_, target)) = self.links.get(&self.key(&prefix)) {
                    let rest = &parts[index + 1..];
                    substituted = Some(if rest.is_empty() {
                        target.clone()
                    } else {
                        format!("{target}/{}", rest.join("/"))
                    });
                    break;
                }
            }
            match substituted {
                Some(next) => current = next,
                None => return current,
            }
        }
        panic!("symlink chain too deep for {path}");
    }

    fn apply(&mut self, op: &Value) {
        let kind = op["op"].as_str().expect("op kind");
        let path = || op["path"].as_str().expect("op path").to_owned();
        match kind {
            "create_file" | "update_file" => {
                let text = op["text"].as_str().expect("op text").to_owned();
                self.files.insert(self.key(&path()), (path(), text));
            }
            "delete_file" => {
                assert!(
                    self.files.remove(&self.key(&path())).is_some(),
                    "delete_file: {} is absent",
                    path()
                );
            }
            "create_directory" => {
                self.directories.insert(self.key(&path()), path());
            }
            "delete_directory" => {
                let key = self.key(&path());
                let prefix = format!("{key}/");
                self.files
                    .retain(|candidate, _| candidate != &key && !candidate.starts_with(&prefix));
                self.directories
                    .retain(|candidate, _| candidate != &key && !candidate.starts_with(&prefix));
                self.links
                    .retain(|candidate, _| candidate != &key && !candidate.starts_with(&prefix));
            }
            "set_symlink" => {
                let target = op["target"].as_str().expect("symlink target").to_owned();
                self.links.insert(self.key(&path()), (path(), target));
            }
            "remove_symlink" => {
                assert!(
                    self.links.remove(&self.key(&path())).is_some(),
                    "remove_symlink: {} is absent",
                    path()
                );
            }
            "set_options"
            | "set_program_options"
            | "unset_options"
            | "unset_program_options"
            | "invalidate_all" => {}
            other => panic!("unknown op {other}"),
        }
    }

    /// Expand directory symlinks into mirrored entries with realpath facts,
    /// and infer parent directories exactly as the memory host builder does.
    fn materialize(&self) -> Materialized {
        let mut out = Materialized::default();
        for (key, (display, text)) in &self.files {
            out.files
                .insert(key.clone(), (display.clone(), text.clone()));
        }
        for (key, display) in &self.directories {
            out.directories.insert(key.clone(), display.clone());
        }
        for (link_key, (link, target)) in &self.links {
            let target_resolved = self.resolve(target);
            let target_key = self.key(&target_resolved);
            let target_prefix = format!("{target_key}/");
            out.directories.insert(link_key.clone(), link.clone());
            out.realpaths
                .insert(link_key.clone(), target_resolved.clone());
            for (file_key, (display, text)) in &self.files {
                if let Some(rest) = file_key.strip_prefix(&target_prefix) {
                    let mirrored_display =
                        format!("{link}/{}", &display[display.len() - rest.len()..]);
                    let mirrored_key = self.key(&mirrored_display);
                    out.files
                        .insert(mirrored_key.clone(), (mirrored_display, text.clone()));
                    out.realpaths.insert(mirrored_key, display.clone());
                    // Mirror the intermediate directories with their realpaths.
                    let mut parent = parent_of(rest);
                    while let Some(relative) = parent {
                        if relative == "/" || relative.is_empty() {
                            break;
                        }
                        let mirrored_directory = format!("{link}/{relative}");
                        let real_directory = format!("{target_resolved}/{relative}");
                        let mirrored_key = self.key(&mirrored_directory);
                        out.directories
                            .insert(mirrored_key.clone(), mirrored_directory);
                        out.realpaths.insert(mirrored_key, real_directory);
                        parent = parent_of(&relative);
                    }
                }
            }
            for (directory_key, display) in &self.directories {
                if let Some(rest) = directory_key.strip_prefix(&target_prefix) {
                    let mirrored_display =
                        format!("{link}/{}", &display[display.len() - rest.len()..]);
                    let mirrored_key = self.key(&mirrored_display);
                    out.directories
                        .insert(mirrored_key.clone(), mirrored_display);
                    out.realpaths.insert(mirrored_key, display.clone());
                }
            }
        }
        // Inferred ancestors of every file and directory (the builder's rule).
        let mut inferred = Vec::new();
        for (display, _) in out.files.values() {
            let mut parent = parent_of(display);
            while let Some(directory) = parent {
                if directory.is_empty() {
                    break;
                }
                inferred.push(directory.clone());
                parent = parent_of(&directory);
            }
        }
        for display in out.directories.values() {
            let mut parent = parent_of(display);
            while let Some(directory) = parent {
                if directory.is_empty() {
                    break;
                }
                inferred.push(directory.clone());
                parent = parent_of(&directory);
            }
        }
        inferred.push(self.current_directory.clone());
        for display in inferred {
            out.directories.entry(self.key(&display)).or_insert(display);
        }
        out
    }

    fn build_host(&self, failures: &[(HostOperation, String)]) -> MemoryCompilerHost {
        let materialized = self.materialize();
        let mut builder = MemoryCompilerHost::builder_js(self.current_directory.as_str())
            .case_sensitive(self.case_sensitive);
        for (display, text) in materialized.files.values() {
            builder = builder.file_js(display.as_str(), text.as_bytes().to_vec());
        }
        for display in materialized.directories.values() {
            builder = builder.directory_js(display.as_str());
        }
        for (key, target) in &materialized.realpaths {
            let display = materialized
                .files
                .get(key)
                .map(|(display, _)| display.clone())
                .or_else(|| materialized.directories.get(key).cloned())
                .expect("realpath source exists");
            builder = builder.realpath_js(display.as_str(), target.as_str());
        }
        for (operation, path) in failures {
            builder = builder.failure(HostError::new_js(
                HostErrorKind::Other,
                *operation,
                Some(path.as_str().into()),
                "injected host failure",
            ));
        }
        builder.build().expect("build memory host")
    }
}

fn change_batch(before: &Materialized, after: &Materialized, case_sensitive: bool) -> ChangeBatch {
    let key = |text: &str| PathKey::new(text.into(), case_sensitive);
    let mut batch = ChangeBatch::default();
    for (path, (_, text)) in &after.files {
        match before.files.get(path) {
            None => {
                batch.created_files.insert(key(path));
            }
            Some((_, previous)) if previous != text => {
                batch.changed_files.insert(key(path));
            }
            Some(_) => {}
        }
    }
    for path in before.files.keys() {
        if !after.files.contains_key(path) {
            batch.deleted_files.insert(key(path));
        }
    }
    for path in after.directories.keys() {
        if !before.directories.contains_key(path) {
            batch.created_directories.insert(key(path));
        }
    }
    for path in before.directories.keys() {
        if !after.directories.contains_key(path) {
            batch.deleted_directories.insert(key(path));
        }
    }
    for (path, target) in &after.realpaths {
        if before.realpaths.get(path) != Some(target) {
            batch.realpath_changed.insert(key(path));
        }
    }
    for path in before.realpaths.keys() {
        if !after.realpaths.contains_key(path) {
            batch.realpath_changed.insert(key(path));
        }
    }
    batch
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

fn program_path(text: &str, case_sensitive: bool) -> ProgramPath {
    let canonical = if case_sensitive {
        text.to_owned()
    } else {
        to_file_name_lower_case(text)
    };
    ProgramPath::from_js_parts(text.into(), canonical.as_str().into()).expect("program path")
}

fn compiler_options(raw: &Map<String, Value>) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (name, value) in raw {
        match name.as_str() {
            "moduleResolution" => {
                options.module_resolution = Some(match value.as_str().expect("moduleResolution") {
                    "classic" => 1,
                    "node10" => 2,
                    "node16" => 3,
                    "nodenext" => 99,
                    "bundler" => 100,
                    other => panic!("moduleResolution {other}"),
                });
            }
            "module" => {
                options.module = Some(match value.as_str().expect("module") {
                    "none" => 0,
                    "commonjs" => 1,
                    "esnext" => 99,
                    "node16" => 100,
                    "nodenext" => 199,
                    "preserve" => 200,
                    other => panic!("module {other}"),
                });
            }
            "moduleSuffixes" => {
                options.module_suffixes = Some(
                    value
                        .as_array()
                        .expect("moduleSuffixes list")
                        .iter()
                        .map(|suffix| ModuleSuffix::value(suffix.as_str().expect("suffix")))
                        .collect(),
                );
            }
            "customConditions" => {
                options.custom_conditions = Some(
                    value
                        .as_array()
                        .expect("customConditions list")
                        .iter()
                        .map(|condition| JsString::from(condition.as_str().expect("condition")))
                        .collect(),
                );
            }
            "baseUrl" => options.base_url = Some(value.as_str().expect("baseUrl").into()),
            "libReplacement" => options.lib_replacement = value.as_bool(),
            "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
            "allowJs" => options.allow_js = value.as_bool().expect("allowJs"),
            "noDtsResolution" => options.no_dts_resolution = value.as_bool(),
            "allowArbitraryExtensions" => options.allow_arbitrary_extensions = value.as_bool(),
            "allowImportingTsExtensions" => {
                options.allow_importing_ts_extensions = value.as_bool();
            }
            "resolvePackageJsonExports" => {
                options.resolve_package_json_exports = value.as_bool();
            }
            "resolvePackageJsonImports" => {
                options.resolve_package_json_imports = value.as_bool();
            }
            other => panic!("unsupported manifest option {other}"),
        }
    }
    options
}

fn program_options(raw: &Map<String, Value>, case_sensitive: bool) -> ProgramOptions {
    let mut options = ProgramOptions::default();
    let mut paths = None;
    let mut paths_base = None;
    for (name, value) in raw {
        match name.as_str() {
            "paths" => {
                paths = Some(
                    value
                        .as_object()
                        .expect("paths object")
                        .iter()
                        .map(|(pattern, substitutions)| {
                            PathMapping::new(
                                pattern.as_str(),
                                substitutions
                                    .as_array()
                                    .expect("substitution list")
                                    .iter()
                                    .map(|entry| {
                                        JsString::from(entry.as_str().expect("substitution"))
                                    })
                                    .collect(),
                            )
                        })
                        .collect::<Vec<_>>(),
                );
            }
            "pathsBasePath" => paths_base = Some(value.as_str().expect("pathsBasePath").to_owned()),
            "rootDirs" => {
                options = options.with_root_dirs(
                    value
                        .as_array()
                        .expect("rootDirs")
                        .iter()
                        .map(|entry| program_path(entry.as_str().expect("rootDir"), case_sensitive))
                        .collect(),
                );
            }
            "typeRoots" => {
                options = options.with_type_roots(
                    value
                        .as_array()
                        .expect("typeRoots")
                        .iter()
                        .map(|entry| {
                            program_path(entry.as_str().expect("typeRoot"), case_sensitive)
                        })
                        .collect(),
                );
            }
            "types" => {
                options = options.with_types(
                    value
                        .as_array()
                        .expect("types")
                        .iter()
                        .map(|entry| JsString::from(entry.as_str().expect("type name")))
                        .collect(),
                );
            }
            "preserveSymlinks" => {
                options =
                    options.with_preserve_symlinks(value.as_bool().expect("preserveSymlinks"));
            }
            "configFilePath" => {
                options = options.with_config_file_path(program_path(
                    value.as_str().expect("configFilePath"),
                    case_sensitive,
                ));
            }
            other => panic!("unsupported manifest program option {other}"),
        }
    }
    if let Some(paths) = paths {
        options = match paths_base {
            Some(base) => options.with_config_paths(paths, base.as_str()),
            None => options.with_paths(paths),
        };
    }
    options
}

fn resolution_mode(text: &str) -> ResolutionMode {
    match text {
        "undefined" => ResolutionMode::Unspecified,
        "commonjs" => ResolutionMode::CommonJs,
        "esnext" => ResolutionMode::EsNext,
        other => panic!("mode {other}"),
    }
}

fn merge(base: &mut Map<String, Value>, overlay: &Value) {
    if let Some(object) = overlay.as_object() {
        for (name, value) in object {
            base.insert(name.clone(), value.clone());
        }
    }
}

fn unset(base: &mut Map<String, Value>, names: &Value) {
    for name in names.as_array().expect("names list") {
        base.remove(name.as_str().expect("option name"));
    }
}

// ---------------------------------------------------------------------------
// Observation records
// ---------------------------------------------------------------------------

fn package_id_text(module: Option<&tsc_program::PackageId>) -> Value {
    match module {
        Some(id) => {
            let sub = if id.submodule_name().is_empty() {
                String::new()
            } else {
                format!("/{}", id.submodule_name().to_string_lossy())
            };
            json!(format!(
                "{}@{}{sub}",
                id.name().to_string_lossy(),
                id.version().to_string_lossy()
            ))
        }
        None => Value::Null,
    }
}

fn host_module_record(module: &HostResolvedModule) -> Value {
    json!({
        "resolvedFileName": module.resolved_file().display().to_string_lossy(),
        "extension": module.extension().as_js().to_string_lossy(),
        "isExternalLibraryImport": module.is_external_library_import(),
        "resolvedUsingTsExtension": module.resolved_using_ts_extension(),
        "originalPath": module.original_path().map(|path| path.display().to_string_lossy().into_owned()),
        "packageId": package_id_text(module.package_id()),
    })
}

/// Project a cached/fresh value onto the native observation schema so the
/// three observations compare field by field.
fn value_record(value: &CachedValue) -> Value {
    match value {
        CachedValue::Module(resolution) => json!({
            "kind": "module",
            "resolved": match resolution.outcome() {
                ResolutionOutcome::Resolved(module) => host_module_record(module),
                ResolutionOutcome::NotFound => Value::Null,
            },
            "alternateResult": resolution.alternate_result().map(|path| path.display().to_string_lossy().into_owned()),
            "resolutionDiagnostics": resolution.diagnostics().iter().map(|diagnostic| diagnostic.code()).collect::<Vec<_>>(),
        }),
        // Type-reference resolution diagnostics are loader-owned in Rust (the
        // host result carries the outcome only); every observed native list
        // is empty, so the projection records the absence explicitly.
        CachedValue::TypeReference(outcome) => json!({
            "kind": "type_reference",
            "resolutionDiagnostics": Vec::<u32>::new(),
            "resolved": match outcome {
                ResolutionOutcome::Resolved(directive) => json!({
                    "resolvedFileName": directive.resolved_file().display().to_string_lossy(),
                    "primary": directive.primary(),
                    "isExternalLibraryImport": directive.is_external_library_import(),
                    "originalPath": directive.original_path().map(|path| path.display().to_string_lossy().into_owned()),
                    "packageId": package_id_text(directive.package_id()),
                }),
                ResolutionOutcome::NotFound => Value::Null,
            },
        }),
        CachedValue::Library(outcome) => json!({
            "kind": "library",
            "resolved": match outcome {
                ResolutionOutcome::Resolved(module) => host_module_record(module),
                ResolutionOutcome::NotFound => Value::Null,
            },
            "alternateResult": Value::Null,
            "resolutionDiagnostics": Vec::<u32>::new(),
        }),
        CachedValue::PackageScope(facts) => json!({
            "kind": "package_scope",
            "impliedNodeFormat": facts.implied_node_format.map(|mode| match mode {
                ResolutionMode::CommonJs => "commonjs",
                ResolutionMode::EsNext => "esnext",
                ResolutionMode::Unspecified => "undefined",
            }),
            "packageJson": facts.package_json.as_ref().map(|path| path.display().to_string_lossy().into_owned()),
            "moduleType": format!("{:?}", facts.module_type),
        }),
    }
}

/// Strip the native-only fields, keeping the primary parity surface.
fn native_parity_projection(record: &Value) -> Value {
    let mut projected = Map::new();
    for name in [
        "kind",
        "resolved",
        "alternateResult",
        "resolutionDiagnostics",
        "impliedNodeFormat",
    ] {
        if let Some(value) = record.get(name) {
            let value = if name == "kind" && value == "automatic_type_reference" {
                json!("type_reference")
            } else {
                value.clone()
            };
            projected.insert(name.to_owned(), value);
        }
    }
    Value::Object(projected)
}

fn rust_parity_projection(record: &Value) -> Value {
    let mut projected = Map::new();
    for name in [
        "kind",
        "resolved",
        "alternateResult",
        "resolutionDiagnostics",
        "impliedNodeFormat",
    ] {
        if let Some(value) = record.get(name) {
            projected.insert(name.to_owned(), value.clone());
        }
    }
    Value::Object(projected)
}

/// tsc's failed-lookup, affecting, and resolved locations must each be
/// covered by a recorded dependency: the exact path, or a recorded absent
/// ancestor directory (creating the location would create the ancestor).
fn uncovered_native_locations(
    native: &Value,
    dependencies: &DependencySet,
    case_sensitive: bool,
) -> Vec<String> {
    let mut locations: Vec<String> = Vec::new();
    for field in ["failedLookupLocations", "affectingLocations"] {
        if let Some(list) = native.get(field).and_then(Value::as_array) {
            locations.extend(list.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    if let Some(resolved) = native.get("resolved").and_then(Value::as_object) {
        for field in ["resolvedFileName", "originalPath"] {
            if let Some(path) = resolved.get(field).and_then(Value::as_str) {
                locations.push(path.to_owned());
            }
        }
    }
    // Covered paths: every dependency's own path plus the physical target of
    // a recorded realpath fact (the matcher invalidates on target deletion).
    let mut paths = dependencies.paths();
    for dependency in dependencies.iter() {
        if let Dependency::Realpath {
            target: Some(target),
            ..
        } = dependency
        {
            paths.insert(target.clone());
        }
    }
    let absent_directories: BTreeSet<PathKey> = dependencies
        .iter()
        .filter_map(|dependency| match dependency {
            Dependency::DirectoryExists {
                path,
                exists: false,
            } => Some(path.clone()),
            _ => None,
        })
        .collect();
    let mut uncovered = Vec::new();
    for location in locations {
        let key = PathKey::new(location.as_str().into(), case_sensitive);
        if paths.contains(&key) {
            continue;
        }
        let mut ancestor = parent_of(&location);
        let mut covered = false;
        while let Some(directory) = ancestor {
            if absent_directories.contains(&PathKey::new(directory.as_str().into(), case_sensitive))
            {
                covered = true;
                break;
            }
            ancestor = parent_of(&directory);
        }
        if !covered {
            uncovered.push(location);
        }
    }
    uncovered.sort();
    uncovered.dedup();
    uncovered
}

// ---------------------------------------------------------------------------
// Fresh resolution (the parity oracle on the same host state)
// ---------------------------------------------------------------------------

fn fresh_value(
    request: &Value,
    host: &dyn CompilerHost,
    options: &CompilerOptions,
    program_options: &ProgramOptions,
) -> Option<CachedValue> {
    let kind = request["kind"].as_str().expect("request kind");
    let containing = request.get("containing").and_then(Value::as_str);
    let specifier = request.get("specifier").and_then(Value::as_str);
    let mode = request
        .get("mode")
        .and_then(Value::as_str)
        .map_or(ResolutionMode::Unspecified, resolution_mode);
    match kind {
        "module" => {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options)
                    .expect("fresh resolver");
            Some(CachedValue::Module(
                resolver
                    .resolve_with_facts(containing.unwrap(), specifier.unwrap(), mode)
                    .expect("fresh module resolution"),
            ))
        }
        "type_reference" | "automatic_type_reference" => {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options)
                    .expect("fresh resolver");
            Some(CachedValue::TypeReference(
                resolver
                    .resolve_type_reference(
                        containing.unwrap(),
                        specifier.unwrap(),
                        mode,
                        program_options.type_roots(),
                    )
                    .expect("fresh type reference resolution"),
            ))
        }
        "library" => {
            let library_options = CompilerOptions {
                module_resolution: Some(2),
                ..CompilerOptions::default()
            };
            let mut resolver = ModuleResolver::new(host, &library_options).expect("fresh resolver");
            Some(CachedValue::Library(
                resolver
                    .resolve(
                        containing.unwrap(),
                        specifier.unwrap(),
                        ResolutionMode::Unspecified,
                    )
                    .expect("fresh library resolution"),
            ))
        }
        "package_scope" => {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options)
                    .expect("fresh resolver");
            let scope = resolver
                .package_scope_for_file(containing.unwrap())
                .expect("fresh package scope");
            // The implied format is loader-private; the fresh oracle compares
            // the package boundary and its module type.
            Some(CachedValue::PackageScope(tsc_program::PackageScopeFacts {
                package_json: scope.as_ref().map(|scope| scope.package_json().clone()),
                module_type: scope
                    .as_ref()
                    .map_or(PackageJsonType::Unspecified, |scope| scope.module_type()),
                implied_node_format: None,
            }))
        }
        _ => None,
    }
}

fn values_agree(cached: &CachedValue, fresh: &CachedValue) -> bool {
    match (cached, fresh) {
        (CachedValue::PackageScope(cached), CachedValue::PackageScope(fresh)) => {
            cached.package_json == fresh.package_json && cached.module_type == fresh.module_type
        }
        (cached, fresh) => cached == fresh,
    }
}

// ---------------------------------------------------------------------------
// Family replay
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct ParityCounters {
    fresh_equal: usize,
    native_equal: usize,
    native_known_open: usize,
    uncovered_locations: usize,
}

struct HeldReader {
    generation_id: String,
    handle: GenerationHandle,
    host: MemoryCompilerHost,
    options: CompilerOptions,
    program_options: ProgramOptions,
}

struct FamilyReplay {
    trace: Value,
    counters: ParityCounters,
    rows_reused: usize,
    rows_recomputed_changed: usize,
    rows_recomputed_unchanged: usize,
    rows_fresh: usize,
}

fn expected_generation<'e>(
    expected: &'e Value,
    family_id: &str,
    generation_id: &str,
) -> Option<&'e Value> {
    expected
        .get("families")?
        .get(family_id)?
        .get("generations")?
        .get(generation_id)
}

fn merge_automatic_names(
    configured: Option<&[JsString]>,
    discovered: &[JsString],
) -> Vec<JsString> {
    // The loader's merge: each configured name in order, expanding "*" to
    // the discovered wildcard matches, deduplicated on first occurrence.
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for name in configured.unwrap_or(&[]) {
        if name == "*" {
            for discovered in discovered {
                if seen.insert(discovered.clone()) {
                    names.push(discovered.clone());
                }
            }
        } else if seen.insert(name.clone()) {
            names.push(name.clone());
        }
    }
    names
}

/// Convert the crate's lossless JSON value to `serde_json::Value` for the
/// native comparison (every config fixture here is scalar text).
fn crate_json(value: &tsc_program::JsonValue) -> Value {
    match value {
        tsc_program::JsonValue::Null => Value::Null,
        tsc_program::JsonValue::Bool(value) => Value::Bool(*value),
        tsc_program::JsonValue::Number(number) => Value::Number(number.clone()),
        tsc_program::JsonValue::String(text) => Value::String(text.to_string_lossy().into_owned()),
        tsc_program::JsonValue::Array(values) => {
            Value::Array(values.iter().map(crate_json).collect())
        }
        tsc_program::JsonValue::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.to_string_lossy().into_owned(), crate_json(value)))
                .collect(),
        ),
    }
}

fn config_projection(plan: &tsc_program::ConfigRootPlan) -> Value {
    fn option_value(state: ConfigOptionValueState<'_>) -> Value {
        match state {
            ConfigOptionValueState::Absent => Value::Null,
            ConfigOptionValueState::Undefined => json!("undefined"),
            ConfigOptionValueState::Value(value) => crate_json(value),
            ConfigOptionValueState::List(elements) => Value::Array(
                elements
                    .iter()
                    .map(|element| match element {
                        ConfigTypedListElement::Value(value) => crate_json(value),
                        ConfigTypedListElement::Undefined => json!("undefined"),
                    })
                    .collect(),
            ),
            ConfigOptionValueState::Object(value) => crate_json(&value.json_projection()),
            ConfigOptionValueState::PositiveInfinity => json!("+Infinity"),
            ConfigOptionValueState::NegativeInfinity => json!("-Infinity"),
        }
    }
    let mut options = Map::new();
    for name in [
        "moduleResolution",
        "module",
        "baseUrl",
        "paths",
        "rootDirs",
        "moduleSuffixes",
        "customConditions",
        "types",
        "typeRoots",
    ] {
        let value = option_value(plan.options().typed_value_state(name));
        if !value.is_null() {
            options.insert(name.to_owned(), value);
        }
    }
    if let Some(base) = plan.options().stored_paths_base_path() {
        options.insert("pathsBasePath".to_owned(), json!(base.to_string_lossy()));
    }
    let mut extended_source_files = plan
        .extended_source_files()
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    extended_source_files.sort();
    let file_names = plan
        .file_names()
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let errors = plan
        .errors()
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>();
    json!({
        "fileNames": file_names,
        "errors": errors,
        "options": options,
        "extendedSourceFiles": extended_source_files,
    })
}

fn replay_family(family: &Value, defaults: &Value, expected: &Value) -> FamilyReplay {
    let family_id = family["id"].as_str().expect("family id");
    let host_spec = {
        let mut host = defaults["host"].as_object().cloned().unwrap_or_default();
        merge(&mut host, family.get("host").unwrap_or(&Value::Null));
        host
    };
    let case_sensitive = host_spec["case_sensitive"].as_bool().unwrap_or(true);
    let current_directory = host_spec["current_directory"]
        .as_str()
        .unwrap_or("/p")
        .to_owned();
    let mut raw_options = defaults["options"].as_object().cloned().unwrap_or_default();
    merge(
        &mut raw_options,
        family.get("options").unwrap_or(&Value::Null),
    );
    let mut raw_program_options = Map::new();
    merge(
        &mut raw_program_options,
        family.get("program_options").unwrap_or(&Value::Null),
    );

    let mut fs_state = VirtualFs::new(&current_directory, case_sensitive);
    if let Some(files) = family["initial"].get("files").and_then(Value::as_object) {
        for (path, text) in files {
            fs_state.apply(&json!({"op": "create_file", "path": path, "text": text}));
        }
    }
    if let Some(directories) = family["initial"]
        .get("directories")
        .and_then(Value::as_array)
    {
        for directory in directories {
            fs_state.apply(&json!({"op": "create_directory", "path": directory}));
        }
    }
    if let Some(links) = family["initial"].get("symlinks").and_then(Value::as_object) {
        for (link, target) in links {
            fs_state.apply(&json!({"op": "set_symlink", "path": link, "target": target}));
        }
    }

    let requests = family["requests"].as_array().expect("requests").clone();
    let mut generations = vec![json!({"id": "g0", "ops": []})];
    generations.extend(
        family["generations"]
            .as_array()
            .expect("generations")
            .iter()
            .cloned(),
    );

    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let token = CancellationToken::default();
    let mut published_state = fs_state.materialize();
    let mut held: Vec<HeldReader> = Vec::new();
    let mut previous_names: Option<(Vec<JsString>, DependencySet)> = None;
    let mut previous_config: Option<(Value, DependencySet)> = None;
    let mut counters = ParityCounters::default();
    let mut rows_reused = 0;
    let mut rows_recomputed_changed = 0;
    let mut rows_recomputed_unchanged = 0;
    let mut rows_fresh = 0;
    let mut generation_traces = Vec::new();

    for generation in &generations {
        let generation_id = generation["id"].as_str().expect("generation id");
        let mut failures: Vec<(HostOperation, String)> = Vec::new();
        let mut cancel_after: Option<usize> = None;
        let mut cancel_before_publish = false;
        let mut hold_reader = false;
        let mut release_readers: Vec<String> = Vec::new();
        if let Some(controls) = generation.get("controls").and_then(Value::as_array) {
            for control in controls {
                match control["control"].as_str().expect("control") {
                    "hold_reader" => hold_reader = true,
                    "release_reader" => {
                        release_readers.push(
                            control["generation"]
                                .as_str()
                                .expect("generation")
                                .to_owned(),
                        );
                    }
                    "host_failure" => {
                        let operation = match control["operation"].as_str().expect("operation") {
                            "file_exists" => HostOperation::FileExists,
                            "read_file" => HostOperation::ReadFile,
                            "directory_exists" => HostOperation::DirectoryExists,
                            "realpath" => HostOperation::Realpath,
                            other => panic!("host failure operation {other}"),
                        };
                        failures.push((
                            operation,
                            control["path"].as_str().expect("path").to_owned(),
                        ));
                    }
                    "cancel_after_requests" => {
                        cancel_after = Some(control["count"].as_u64().expect("count") as usize);
                    }
                    "cancel_before_publish" => cancel_before_publish = true,
                    "evict_all" => cache.evict_all().expect("evict_all within the live bound"),
                    "limits" => {
                        let mut limits = cache.limits();
                        if let Some(value) = control.get("max_entries").and_then(Value::as_u64) {
                            limits.max_entries = value as usize;
                        }
                        if let Some(value) = control.get("max_bytes").and_then(Value::as_u64) {
                            limits.max_bytes = value as usize;
                        }
                        if let Some(value) =
                            control.get("max_live_generations").and_then(Value::as_u64)
                        {
                            limits.max_live_generations = value as usize;
                        }
                        if let Some(value) =
                            control.get("max_eviction_history").and_then(Value::as_u64)
                        {
                            limits.max_eviction_history = value as usize;
                        }
                        cache.set_limits(limits);
                    }
                    other => panic!("unknown control {other}"),
                }
            }
        }
        for op in generation["ops"].as_array().expect("ops") {
            match op["op"].as_str().expect("op") {
                "set_options" => merge(&mut raw_options, &op["options"]),
                "set_program_options" => merge(&mut raw_program_options, &op["program_options"]),
                "unset_options" => unset(&mut raw_options, &op["names"]),
                "unset_program_options" => unset(&mut raw_program_options, &op["names"]),
                _ => fs_state.apply(op),
            }
        }
        let options = compiler_options(&raw_options);
        let prog_options = program_options(&raw_program_options, case_sensitive);
        let materialized = fs_state.materialize();
        let batch = change_batch(&published_state, &materialized, case_sensitive);
        let host = fs_state.build_host(&failures);
        let plain_host = fs_state.build_host(&[]);
        let published_before = cache.current();
        let expected_generation = expected_generation(expected, family_id, generation_id);
        let token = {
            // A fresh token per generation; cancellation is explicit below.
            let _ = &token;
            CancellationToken::default()
        };

        let mut candidate = cache.begin(batch.clone(), &host, &options, &prog_options);
        let mut request_rows: Vec<Value> = Vec::new();
        let mut entries: Vec<Option<Rc<CacheEntry>>> = Vec::new();
        let mut aborted: Option<String> = None;
        let mut names_outcome: Option<Value> = None;
        let mut config_outcome: Option<Value> = None;

        'requests: for (index, request) in requests.iter().enumerate() {
            if let Some(count) = cancel_after {
                if index == count {
                    token.cancel();
                }
            }
            let kind = request["kind"].as_str().expect("kind");
            let containing = request
                .get("containing")
                .and_then(Value::as_str)
                .unwrap_or("");
            let specifier = request
                .get("specifier")
                .and_then(Value::as_str)
                .unwrap_or("");
            let mode = request
                .get("mode")
                .and_then(Value::as_str)
                .map_or(ResolutionMode::Unspecified, resolution_mode);
            let outcome: Result<Rc<CacheEntry>, CacheError> = match kind {
                "module" => {
                    candidate.resolve_module(containing.into(), specifier.into(), mode, &token)
                }
                "type_reference" => candidate.resolve_type_reference(
                    containing.into(),
                    specifier.into(),
                    mode,
                    false,
                    &token,
                ),
                "automatic_type_reference" => candidate.resolve_type_reference(
                    containing.into(),
                    specifier.into(),
                    mode,
                    true,
                    &token,
                ),
                "library" => candidate.resolve_library(containing.into(), specifier.into(), &token),
                "package_scope" => candidate.package_scope(containing.into(), &token),
                "automatic_type_names" => {
                    let needs_discovery = match &previous_names {
                        Some((_, dependencies)) => {
                            batch.violated_dependency(dependencies).is_some()
                        }
                        None => true,
                    };
                    let (names, dependencies, disposition) = if needs_discovery {
                        match candidate.discover_automatic_type_directive_names(&token) {
                            Ok((names, dependencies)) => {
                                let changed = previous_names
                                    .as_ref()
                                    .is_none_or(|(previous, _)| previous != &names);
                                (
                                    names,
                                    dependencies,
                                    if previous_names.is_none() {
                                        "fresh"
                                    } else if changed {
                                        "recomputed_changed"
                                    } else {
                                        "recomputed_unchanged"
                                    },
                                )
                            }
                            Err(error) => {
                                aborted = Some(format!("{error}"));
                                break 'requests;
                            }
                        }
                    } else {
                        let (names, dependencies) = previous_names.clone().expect("previous names");
                        (names, dependencies, "reused")
                    };
                    let merged = merge_automatic_names(prog_options.types(), &names);
                    let merged_text: Vec<String> = merged
                        .iter()
                        .map(|name| name.to_string_lossy().into_owned())
                        .collect();
                    let native_names = expected_generation
                        .and_then(|generation| generation["requests"].get(index))
                        .and_then(|record| record.get("names"))
                        .and_then(Value::as_array)
                        .map(|names| {
                            names
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        });
                    let native_equal = native_names.as_ref() == Some(&merged_text);
                    if native_equal {
                        counters.native_equal += 1;
                    }
                    assert!(
                        native_equal,
                        "{family_id}/{generation_id} automatic names differ: rust={merged_text:?} native={native_names:?}"
                    );
                    names_outcome = Some(json!({
                        "request": index,
                        "kind": "automatic_type_names",
                        "disposition": disposition,
                        "names": merged_text,
                        "dependency_count": dependencies.len(),
                        "dependencies": dependencies.to_json(),
                    }));
                    previous_names = Some((names, dependencies));
                    entries.push(None);
                    continue;
                }
                "config" => {
                    let config_path = family["config"].as_str().expect("config path");
                    let needs_parse = match &previous_config {
                        Some((_, dependencies)) => {
                            batch.violated_dependency(dependencies).is_some()
                        }
                        None => true,
                    };
                    let observation = ObservationHost::new(&host);
                    let (projection, dependencies, disposition) = if needs_parse {
                        let (plan, dependencies) = observation.record(|observed| {
                            let config_host = CompilerConfigHost::new(observed);
                            let text = match observed.read_file_js(config_path.into()) {
                                Ok(Some(bytes)) => String::from_utf8(bytes).expect("utf8 config"),
                                Ok(None) => String::new(),
                                Err(error) => panic!("read config: {error}"),
                            };
                            parse_config_root_plan(
                                &config_host,
                                ConfigRootPlanRequest {
                                    file_name: config_path.into(),
                                    text,
                                    base_path: parent_of(config_path)
                                        .expect("config directory")
                                        .as_str()
                                        .into(),
                                },
                            )
                            .expect("parse config plan")
                        });
                        let projection = config_projection(&plan);
                        let disposition = match &previous_config {
                            None => "fresh",
                            Some((previous, _))
                                if previous["options"] != projection["options"]
                                    || previous["errors"] != projection["errors"] =>
                            {
                                "options_changed"
                            }
                            Some((previous, _))
                                if previous["fileNames"] != projection["fileNames"] =>
                            {
                                "roots_changed"
                            }
                            Some(_) => "recomputed_unchanged",
                        };
                        (projection, dependencies, disposition)
                    } else {
                        let (projection, dependencies) =
                            previous_config.clone().expect("previous config");
                        (projection, dependencies, "reused")
                    };
                    let native =
                        expected_generation.and_then(|generation| generation.get("config"));
                    let native_projection = native.map(|native| {
                        json!({
                            "fileNames": native["fileNames"],
                            "errors": native["errors"],
                            "options": native["options"],
                            "extendedSourceFiles": native["extendedSourceFiles"],
                        })
                    });
                    let native_equal = native_projection.as_ref() == Some(&projection);
                    if native_equal {
                        counters.native_equal += 1;
                    }
                    assert!(
                        native_equal,
                        "{family_id}/{generation_id} config differs:\nrust={}\nnative={}",
                        serde_json::to_string_pretty(&projection).unwrap(),
                        serde_json::to_string_pretty(&native_projection).unwrap()
                    );
                    config_outcome = Some(json!({
                        "request": index,
                        "kind": "config",
                        "disposition": disposition,
                        "projection": projection,
                        "dependency_count": dependencies.len(),
                        "dependencies": dependencies.to_json(),
                    }));
                    previous_config = Some((projection, dependencies));
                    entries.push(None);
                    continue;
                }
                other => panic!("unknown request kind {other}"),
            };
            match outcome {
                Ok(entry) => entries.push(Some(entry)),
                Err(error) => {
                    aborted = Some(match error {
                        CacheError::Cancelled { .. } => "cancelled".to_owned(),
                        CacheError::Resolution(_) => "host_error".to_owned(),
                        other => format!("{other}"),
                    });
                    break 'requests;
                }
            }
        }

        let expect = generation.get("expect").cloned().unwrap_or(Value::Null);
        if aborted.is_none() && cancel_before_publish {
            aborted = Some("dropped".to_owned());
        }

        if let Some(reason) = aborted.clone() {
            // The candidate is dropped without publishing: the published
            // view must be untouched, entry for entry.
            drop(candidate);
            let published_after = cache.current();
            assert_eq!(
                published_before.id(),
                published_after.id(),
                "{family_id}/{generation_id}: publish happened after abort"
            );
            assert!(
                published_before
                    .view()
                    .entries()
                    .zip(published_after.view().entries())
                    .all(|(before, after)| Rc::ptr_eq(before, after))
                    && published_before.view().len() == published_after.view().len(),
                "{family_id}/{generation_id}: published entries changed after abort"
            );
            if let Some(expected_abort) = expect.get("aborted").and_then(Value::as_str) {
                assert_eq!(
                    reason, expected_abort,
                    "{family_id}/{generation_id} abort reason"
                );
            }
            generation_traces.push(json!({
                "generation": generation_id,
                "batch": batch.to_json(),
                "aborted": reason,
                "published_generation": published_after.id(),
            }));
            // The next generation's batch is computed from the published
            // state, which did not advance.
            continue;
        }

        // Fresh and native parity for every cached request.
        for (index, request) in requests.iter().enumerate() {
            let Some(entry) = entries.get(index).and_then(|entry| entry.as_ref()) else {
                continue;
            };
            let fresh =
                fresh_value(request, &plain_host, &options, &prog_options).expect("fresh value");
            let fresh_equal = values_agree(entry.value(), &fresh);
            assert!(
                fresh_equal,
                "{family_id}/{generation_id} request {index}: cached value differs from fresh resolver\ncached={}\nfresh={}",
                value_record(entry.value()),
                value_record(&fresh)
            );
            counters.fresh_equal += 1;
            let record = value_record(entry.value());
            let native =
                expected_generation.and_then(|generation| generation["requests"].get(index));
            let mut native_status = "unobserved";
            let mut uncovered = Vec::new();
            if let Some(native) = native {
                let rust_projection = rust_parity_projection(&record);
                let native_projection = native_parity_projection(native);
                let equal = rust_projection == native_projection;
                if equal {
                    counters.native_equal += 1;
                    native_status = "exact";
                } else {
                    counters.native_known_open += 1;
                    native_status = "differs";
                }
                assert!(
                    equal,
                    "{family_id}/{generation_id} request {index}: native parity differs\nrust={}\nnative={}",
                    serde_json::to_string_pretty(&rust_projection).unwrap(),
                    serde_json::to_string_pretty(&native_projection).unwrap()
                );
                if matches!(
                    entry.key().kind(),
                    RequestKind::Module
                        | RequestKind::TypeReference
                        | RequestKind::AutomaticTypeReference
                        | RequestKind::Library
                ) {
                    uncovered =
                        uncovered_native_locations(native, entry.dependencies(), case_sensitive);
                    counters.uncovered_locations += uncovered.len();
                }
            }
            let row = candidate
                .trace()
                .iter()
                .find(|row| row.key == *entry.key())
                .expect("trace row for request");
            let disposition = row.disposition.name();
            match &row.disposition {
                Disposition::Reused => rows_reused += 1,
                Disposition::Recomputed { changed: true, .. } => rows_recomputed_changed += 1,
                Disposition::Recomputed { changed: false, .. } => rows_recomputed_unchanged += 1,
                Disposition::Fresh { .. } => rows_fresh += 1,
            }
            for (name, expected_disposition) in [
                ("reused", "reused"),
                ("recomputed", "recomputed"),
                ("fresh", "fresh"),
            ] {
                if let Some(indices) = expect.get(name).and_then(Value::as_array) {
                    if indices
                        .iter()
                        .any(|value| value.as_u64() == Some(index as u64))
                    {
                        assert_eq!(
                            disposition, expected_disposition,
                            "{family_id}/{generation_id} request {index}: expected {expected_disposition}, trace says {disposition} ({})",
                            row.to_json()
                        );
                    }
                }
            }
            request_rows.push(json!({
                "request": index,
                "key": entry.key().display(),
                "disposition": row.to_json(),
                "value": record,
                "fresh_equal": fresh_equal,
                "native": native_status,
                "native_locations_uncovered": uncovered,
                "dependencies": entry.dependencies().to_json(),
            }));
        }
        if let Some(names) = names_outcome.take() {
            if let Some(expected_names) = expect.get("names").and_then(Value::as_str) {
                assert_eq!(
                    names["disposition"], expected_names,
                    "{family_id}/{generation_id} names disposition"
                );
            }
            request_rows.push(names);
        }
        if let Some(config) = config_outcome.take() {
            if let Some(expected_config) = expect.get("config").and_then(Value::as_str) {
                let actual = config["disposition"].as_str().unwrap();
                let matches = actual == expected_config
                    || (expected_config == "reused" && actual == "recomputed_unchanged")
                    || (generation_id == "g0" && actual == "fresh");
                assert!(matches, "{family_id}/{generation_id} config disposition: expected {expected_config}, got {actual}");
            }
            request_rows.push(config);
        }

        // Program-side rebinding and source-order parity for root families.
        let mut program_record = Value::Null;
        if let Some(roots) = family.get("roots").and_then(Value::as_array) {
            program_record = program_rebind_record(
                family_id,
                generation_id,
                roots,
                &plain_host,
                &options,
                &prog_options,
                case_sensitive,
                &entries,
                expected_generation.and_then(|generation| generation.get("program")),
            );
        }

        let candidate_json = candidate.to_json();
        let (handle, stats) = cache.publish(candidate).expect("publish candidate");
        published_state = materialized;

        // Old readers keep their values: every entry of a held view still
        // equals a fresh resolution on the reader's own host state.
        for reader in &held {
            for entry in reader.handle.view().entries() {
                let request = request_for_key(&requests, entry.key());
                let Some(request) = request else { continue };
                let fresh = fresh_value(
                    &request,
                    &reader.host,
                    &reader.options,
                    &reader.program_options,
                )
                .expect("fresh for held reader");
                assert!(
                    values_agree(entry.value(), &fresh),
                    "{family_id}/{generation_id}: held reader {} lost parity for {}",
                    reader.generation_id,
                    entry.key().display()
                );
            }
        }
        if hold_reader {
            held.push(HeldReader {
                generation_id: generation_id.to_owned(),
                handle: handle.clone(),
                host: plain_host.clone(),
                options: options.clone(),
                program_options: prog_options.clone(),
            });
        }
        for release in &release_readers {
            let before = held.len();
            held.retain(|reader| &reader.generation_id != release);
            assert_eq!(
                before,
                held.len() + 1,
                "{family_id}/{generation_id}: release_reader {release} not held"
            );
        }
        let live = cache.live_generation_count();
        generation_traces.push(json!({
            "generation": generation_id,
            "published_generation": handle.id(),
            "batch": batch.to_json(),
            "candidate": candidate_json,
            "stats": stats.to_json(),
            "live_generations": live,
            "held_readers": held.iter().map(|reader| reader.generation_id.clone()).collect::<Vec<_>>(),
            "requests": request_rows,
            "program": program_record,
        }));
    }

    FamilyReplay {
        trace: json!({
            "family": family_id,
            "description": family["description"],
            "generations": generation_traces,
        }),
        counters,
        rows_reused,
        rows_recomputed_changed,
        rows_recomputed_unchanged,
        rows_fresh,
    }
}

fn request_for_key(requests: &[Value], key: &tsc_program::RequestKey) -> Option<Value> {
    requests
        .iter()
        .find(|request| {
            let kind = request["kind"].as_str().unwrap_or("");
            let matches_kind = match key.kind() {
                RequestKind::Module => kind == "module",
                RequestKind::TypeReference => kind == "type_reference",
                RequestKind::AutomaticTypeReference => kind == "automatic_type_reference",
                RequestKind::Library => kind == "library",
                RequestKind::PackageScope => kind == "package_scope",
            };
            if !matches_kind {
                return false;
            }
            let specifier = request
                .get("specifier")
                .and_then(Value::as_str)
                .unwrap_or("");
            let containing = request
                .get("containing")
                .and_then(Value::as_str)
                .unwrap_or("");
            let containing_key = parent_of(containing).unwrap_or_default();
            match key.kind() {
                RequestKind::PackageScope => {
                    containing.rsplit('/').next() == Some(&key.specifier().to_string_lossy())
                        && key
                            .containing_directory()
                            .to_string()
                            .eq_ignore_ascii_case(&containing_key)
                }
                _ => {
                    key.specifier() == specifier
                        && key
                            .containing_directory()
                            .to_string()
                            .eq_ignore_ascii_case(&containing_key)
                        && request
                            .get("mode")
                            .and_then(Value::as_str)
                            .map_or(ResolutionMode::Unspecified, resolution_mode)
                            == key.mode()
                }
            }
        })
        .cloned()
}

/// Bind the cached host-level rows to a per-generation `PreparedProgram`
/// built from the fresh loader's membership, then compare the rows and
/// record the per-generation source ids.
#[allow(clippy::too_many_arguments)]
fn program_rebind_record(
    family_id: &str,
    generation_id: &str,
    roots: &[Value],
    host: &MemoryCompilerHost,
    options: &CompilerOptions,
    program_options: &ProgramOptions,
    case_sensitive: bool,
    entries: &[Option<Rc<CacheEntry>>],
    native: Option<&Value>,
) -> Value {
    let root_paths: Vec<PathBuf> = roots
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root")))
        .collect();
    let load_options = CompilerOptions {
        no_emit: Some(true),
        ..options.clone()
    };
    let load_program_options = program_options.clone().with_no_lib(true);
    let fresh = load_no_lib_program(
        host,
        &root_paths,
        load_options.clone(),
        load_program_options.clone(),
        ProgramLoadLimits::new(64, 256, 16, 1 << 20, 8 << 20),
    )
    .unwrap_or_else(|error| panic!("{family_id}/{generation_id}: fresh load failed: {error}"));
    let fresh_order: Vec<String> = fresh
        .source_files()
        .iter()
        .map(|source| source.path().display().to_string_lossy().into_owned())
        .collect();
    if let Some(native) = native {
        let native_order: Vec<String> = native["sourceFiles"]
            .as_array()
            .expect("native sourceFiles")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        assert_eq!(
            fresh_order, native_order,
            "{family_id}/{generation_id}: program source order differs from native"
        );
        let native_options: Vec<u64> = native["optionsDiagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        let rust_options: Vec<u64> = fresh
            .diagnostics()
            .options()
            .iter()
            .chain(fresh.diagnostics().program())
            .map(|diagnostic| u64::from(diagnostic.code()))
            .collect();
        assert_eq!(
            rust_options, native_options,
            "{family_id}/{generation_id}: program diagnostics differ from native"
        );
    }

    // Rebuild a prepared program from the fresh membership and the cached rows.
    let path_context = PathContext::new(program_path("/p", case_sensitive), case_sensitive);
    let mut builder = PreparedProgramBuilder::new(path_context, load_options);
    builder.set_program_options(load_program_options);
    let mut ids: BTreeMap<String, tsc_program::SourceFileId> = BTreeMap::new();
    for source in fresh.source_files() {
        let text = source.text().to_owned();
        let id = builder
            .add_source_file(PreparedSourceFile::new(source.path().clone(), text))
            .expect("add source");
        ids.insert(
            source
                .path()
                .canonical()
                .as_js()
                .to_string_lossy()
                .into_owned(),
            id,
        );
    }
    for root in fresh.roots() {
        let id = fresh
            .source_id(root.path().canonical())
            .expect("root is a member");
        builder
            .add_root(PreparedRoot::loaded(root.path().clone(), id))
            .expect("add root");
    }
    let mut bound_rows = Vec::new();
    for (key, fresh_row) in fresh.resolutions().modules() {
        let cached = entries.iter().flatten().find(|entry| {
            entry.key().kind() == RequestKind::Module
                && entry.key().specifier() == key.specifier()
                && entry.key().mode() == key.mode()
                && entry.key().containing_directory().to_string()
                    == parent_of(&key.source().as_js().to_string_lossy()).unwrap_or_default()
        });
        let Some(cached) = cached else {
            continue;
        };
        let CachedValue::Module(resolution) = cached.value() else {
            unreachable!()
        };
        let (outcome, diagnostics) = resolution.clone().into_parts();
        let alternate = resolution.alternate_result().cloned();
        let bound = match outcome {
            ResolutionOutcome::Resolved(module) => {
                let canonical = module
                    .resolved_file()
                    .canonical()
                    .as_js()
                    .to_string_lossy()
                    .into_owned();
                let target = match ids.get(&canonical) {
                    Some(id) => ResolvedModuleTarget::Source {
                        source: *id,
                        resolved_file: module.resolved_file().clone(),
                    },
                    None => ResolvedModuleTarget::unloaded(
                        module.resolved_file().clone(),
                        tsc_program::UnloadedModuleReason::ResolutionOnly,
                    ),
                };
                let mut row = ModuleResolution::resolved(
                    module.into_resolved_module(target).expect("bind module"),
                )
                .with_diagnostics(diagnostics);
                if let Some(alternate) = alternate {
                    row = row.with_alternate_result(alternate);
                }
                row
            }
            ResolutionOutcome::NotFound => {
                let mut row = ModuleResolution::not_found().with_diagnostics(diagnostics);
                if let Some(alternate) = alternate {
                    row = row.with_alternate_result(alternate);
                }
                row
            }
        };
        assert_eq!(
            bound
                .outcome()
                .as_ref()
                .map(|module| module.target().resolved_file().clone()),
            fresh_row
                .outcome()
                .as_ref()
                .map(|module| module.target().resolved_file().clone()),
            "{family_id}/{generation_id}: rebound row target differs for {}",
            key.specifier().to_string_lossy()
        );
        assert_eq!(
            bound
                .outcome()
                .as_ref()
                .and_then(|module| module.target().source()),
            fresh_row
                .outcome()
                .as_ref()
                .and_then(|module| module.target().source()),
            "{family_id}/{generation_id}: rebound source id differs for {}",
            key.specifier().to_string_lossy()
        );
        bound_rows.push(json!({
            "key": format!("{}:{}:{:?}", key.source().as_js().to_string_lossy(), key.specifier().to_string_lossy(), key.mode()),
            "bound_source_id": bound.outcome().as_ref().and_then(|module| module.target().source()).map(|id| id.raw()),
            "cache_generation": cached.created_generation(),
        }));
        builder
            .add_module_resolution(key.clone(), Ok(bound))
            .expect("add bound module resolution");
    }
    let rebuilt = builder.build().expect("build rebound program");
    assert_eq!(
        rebuilt.source_files().len(),
        fresh.source_files().len(),
        "{family_id}/{generation_id}: rebound program membership differs"
    );
    json!({
        "source_order": fresh_order,
        "source_ids": ids.iter().map(|(path, id)| json!({"path": path, "id": id.raw()})).collect::<Vec<_>>(),
        "bound_rows": bound_rows,
    })
}

trait ResolutionOutcomeExt<T> {
    fn as_ref(&self) -> Option<&T>;
}

impl<T> ResolutionOutcomeExt<T> for ResolutionOutcome<T> {
    fn as_ref(&self) -> Option<&T> {
        match self {
            ResolutionOutcome::Resolved(value) => Some(value),
            ResolutionOutcome::NotFound => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn resolution_cache_change_trace_matches_fresh_and_native() {
    let manifest: Value = serde_json::from_str(MANIFEST).expect("manifest json");
    let expected: Value = serde_json::from_str(EXPECTED).expect("expected json");
    assert_eq!(manifest["schema"], "tsc-rs/resolution-cache-manifest/v1");
    assert_eq!(expected["schema"], "tsc-rs/resolution-cache-expected/v1");
    assert_eq!(expected["typescript"], "6.0.3");
    let manifest_digest = {
        use std::fmt::Write as _;
        let digest = tsc_program::content_digest(MANIFEST.as_bytes());
        let mut text = String::new();
        write!(text, "{digest:016x}").unwrap();
        text
    };
    let families = manifest["families"].as_array().expect("families");
    let mut summary_rows = Vec::new();
    let mut totals = ParityCounters::default();
    let mut total_rows = (0usize, 0usize, 0usize, 0usize);
    let mut traces_dir = output_dir();
    traces_dir.push("traces");
    fs::create_dir_all(&traces_dir).expect("traces dir");
    for family in families {
        let first = replay_family(family, &manifest["defaults"], &expected);
        let second = replay_family(family, &manifest["defaults"], &expected);
        assert_eq!(
            serde_json::to_string(&first.trace).unwrap(),
            serde_json::to_string(&second.trace).unwrap(),
            "{}: trace is not deterministic across two replays",
            family["id"]
        );
        let file_name = family["id"]
            .as_str()
            .unwrap()
            .trim_start_matches("resolution-cache/")
            .replace('/', "__");
        let path = traces_dir.join(format!("{file_name}.json"));
        fs::write(
            &path,
            serde_json::to_string_pretty(&first.trace).unwrap() + "\n",
        )
        .expect("write trace");
        totals.fresh_equal += first.counters.fresh_equal;
        totals.native_equal += first.counters.native_equal;
        totals.native_known_open += first.counters.native_known_open;
        totals.uncovered_locations += first.counters.uncovered_locations;
        total_rows.0 += first.rows_reused;
        total_rows.1 += first.rows_recomputed_changed;
        total_rows.2 += first.rows_recomputed_unchanged;
        total_rows.3 += first.rows_fresh;
        summary_rows.push(json!({
            "family": family["id"],
            "fresh_equal": first.counters.fresh_equal,
            "native_equal": first.counters.native_equal,
            "native_differs": first.counters.native_known_open,
            "native_locations_uncovered": first.counters.uncovered_locations,
            "reused": first.rows_reused,
            "recomputed_changed": first.rows_recomputed_changed,
            "recomputed_unchanged": first.rows_recomputed_unchanged,
            "fresh": first.rows_fresh,
            "trace": path.file_name().unwrap().to_str().unwrap(),
        }));
    }
    let summary = json!({
        "schema": "tsc-rs/resolution-cache-summary/v1",
        "manifest_fnv1a64": manifest_digest,
        "expected_manifest_sha256": expected["manifest_sha256"],
        "families": families.len(),
        "totals": {
            "fresh_equal": totals.fresh_equal,
            "native_equal": totals.native_equal,
            "native_differs": totals.native_known_open,
            "native_locations_uncovered": totals.uncovered_locations,
            "reused": total_rows.0,
            "recomputed_changed": total_rows.1,
            "recomputed_unchanged": total_rows.2,
            "fresh": total_rows.3,
        },
        "rows": summary_rows,
    });
    write_json("summary.json", &summary);
    assert_eq!(
        totals.native_known_open, 0,
        "native parity differences recorded"
    );
    assert_eq!(
        totals.uncovered_locations, 0,
        "native lookup locations not covered by recorded dependencies"
    );
    assert!(
        total_rows.0 > 0 && total_rows.1 > 0 && total_rows.3 > 0,
        "trace must contain reused, recomputed and fresh rows"
    );
}

// ---------------------------------------------------------------------------
// Seeded soak
// ---------------------------------------------------------------------------

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

const SOAK_GENERATIONS: usize = 1000;
const SOAK_SEED: u64 = 0x5eed_1234_abcd_0001;

#[derive(Clone, Copy)]
enum SoakRequest {
    Module(&'static str, &'static str),
    TypeReference(&'static str, &'static str, bool),
    Library(&'static str, &'static str),
    PackageScope(&'static str),
}

fn soak_requests() -> Vec<SoakRequest> {
    vec![
        SoakRequest::Module("/p/main.ts", "./a"),
        SoakRequest::Module("/p/main.ts", "./b"),
        SoakRequest::Module("/p/main.ts", "./c"),
        SoakRequest::Module("/p/main.ts", "pkg"),
        SoakRequest::Module("/p/main.ts", "pkg/lib/x"),
        SoakRequest::Module("/p/sub/other.ts", "pkg"),
        SoakRequest::Module("/p/sub/other.ts", "../a"),
        SoakRequest::TypeReference("/p/main.ts", "foo", false),
        SoakRequest::TypeReference("/p/__inferred type names__.ts", "foo", true),
        SoakRequest::Library(
            "/p/__lib_node_modules_lookup_lib.dom.d.ts__.ts",
            "@typescript/lib-dom",
        ),
        SoakRequest::PackageScope("/p/main.ts"),
        SoakRequest::PackageScope("/p/sub/other.ts"),
    ]
}

fn soak_universe() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("/p/a.ts", vec!["export {};\n", "export const a = 1;\n"]),
        ("/p/a.d.ts", vec!["export {};\n"]),
        ("/p/b.ts", vec!["export {};\n"]),
        ("/p/b.tsx", vec!["export {};\n"]),
        ("/p/c/index.ts", vec!["export {};\n"]),
        ("/p/c/package.json", vec!["{\"types\":\"./types.d.ts\"}", "{\"main\":\"./index.ts\"}"]),
        ("/p/c/types.d.ts", vec!["export {};\n"]),
        ("/p/package.json", vec!["{\"type\":\"module\"}", "{\"type\":\"commonjs\"}", "{}"]),
        ("/p/sub/package.json", vec!["{\"type\":\"module\"}", "{}"]),
        ("/p/node_modules/pkg/package.json", vec![
            "{\"name\":\"pkg\",\"version\":\"1.0.0\",\"types\":\"index.d.ts\"}",
            "{\"name\":\"pkg\",\"version\":\"2.0.0\",\"exports\":{\".\":{\"types\":\"./index.d.ts\"},\"./lib/*\":{\"types\":\"./lib/*.d.ts\"}}}",
            "{",
        ]),
        ("/p/node_modules/pkg/index.d.ts", vec!["export {};\n"]),
        ("/p/node_modules/pkg/lib/x.d.ts", vec!["export {};\n"]),
        ("/p/node_modules/@types/foo/index.d.ts", vec!["declare var foo: string;\n"]),
        ("/p/node_modules/@types/foo/package.json", vec!["{\"name\":\"@types/foo\",\"version\":\"1.0.0\"}", "{\"typings\":null}"]),
        ("/p/node_modules/@typescript/lib-dom/package.json", vec!["{\"name\":\"@typescript/lib-dom\",\"version\":\"1.0.0\",\"types\":\"index.d.ts\"}"]),
        ("/p/node_modules/@typescript/lib-dom/index.d.ts", vec!["declare var dom: string;\n"]),
    ]
}

fn soak_resolve(
    candidate: &mut tsc_program::Candidate<'_>,
    request: SoakRequest,
    token: &CancellationToken,
) -> Result<Rc<CacheEntry>, CacheError> {
    match request {
        SoakRequest::Module(containing, specifier) => candidate.resolve_module(
            containing.into(),
            specifier.into(),
            ResolutionMode::CommonJs,
            token,
        ),
        SoakRequest::TypeReference(containing, specifier, automatic) => candidate
            .resolve_type_reference(
                containing.into(),
                specifier.into(),
                ResolutionMode::Unspecified,
                automatic,
                token,
            ),
        SoakRequest::Library(containing, specifier) => {
            candidate.resolve_library(containing.into(), specifier.into(), token)
        }
        SoakRequest::PackageScope(file) => candidate.package_scope(file.into(), token),
    }
}

fn soak_request_json(request: SoakRequest) -> Value {
    match request {
        SoakRequest::Module(containing, specifier) => {
            json!({"kind": "module", "containing": containing, "specifier": specifier, "mode": "commonjs"})
        }
        SoakRequest::TypeReference(containing, specifier, automatic) => {
            json!({"kind": if automatic { "automatic_type_reference" } else { "type_reference" }, "containing": containing, "specifier": specifier, "mode": "undefined"})
        }
        SoakRequest::Library(containing, specifier) => {
            json!({"kind": "library", "containing": containing, "specifier": specifier})
        }
        SoakRequest::PackageScope(file) => json!({"kind": "package_scope", "containing": file}),
    }
}

fn run_soak(seed: u64) -> Value {
    let requests = soak_requests();
    let universe = soak_universe();
    let options = CompilerOptions {
        module_resolution: Some(99),
        module: Some(199),
        lib_replacement: Some(true),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default();
    let mut rng = XorShift(seed);
    let mut fs_state = VirtualFs::new("/p", true);
    fs_state.apply(&json!({"op": "create_file", "path": "/p/main.ts", "text": "export {};\n"}));
    fs_state
        .apply(&json!({"op": "create_file", "path": "/p/sub/other.ts", "text": "export {};\n"}));
    let limits = RetentionLimits {
        max_entries: 8,
        max_bytes: 64 << 10,
        max_live_generations: 6,
        max_eviction_history: 16,
    };
    let mut cache = ResolutionCache::new(limits);
    let mut published_state = fs_state.materialize();
    let mut held: Vec<(u64, GenerationHandle, MemoryCompilerHost)> = Vec::new();
    let mut metrics = json!({
        "generations": 0, "reused": 0, "recomputed_changed": 0, "recomputed_unchanged": 0, "fresh": 0,
        "fresh_after_eviction": 0, "evicted": 0, "max_entries": 0, "max_bytes": 0, "max_live_generations": 0,
        "parity_checks": 0, "held_reader_checks": 0, "host_calls": 0, "memo_hits": 0, "cancelled": 0, "publish_refused_live_limit": 0,
        "ops": {"create": 0, "update": 0, "delete": 0, "noop": 0, "hold": 0, "release": 0, "cancel": 0}
    });
    let bump = |metrics: &mut Value, name: &str, by: u64| {
        let slot = &mut metrics[name];
        *slot = json!(slot.as_u64().unwrap() + by);
    };
    let bump_op = |metrics: &mut Value, name: &str| {
        let slot = &mut metrics["ops"][name];
        *slot = json!(slot.as_u64().unwrap() + 1);
    };
    let mut evicted_pending: BTreeSet<String> = BTreeSet::new();
    for generation in 0..SOAK_GENERATIONS {
        let op_count = 1 + rng.below(3);
        let mut cancel = false;
        for _ in 0..op_count {
            match rng.below(10) {
                0..=3 => {
                    let (path, variants) = &universe[rng.below(universe.len())];
                    let text = variants[rng.below(variants.len())];
                    let key = fs_state.key(path);
                    if fs_state.files.contains_key(&key) {
                        bump_op(&mut metrics, "update");
                    } else {
                        bump_op(&mut metrics, "create");
                    }
                    fs_state.apply(&json!({"op": "create_file", "path": path, "text": text}));
                }
                4..=5 => {
                    let (path, _) = &universe[rng.below(universe.len())];
                    let key = fs_state.key(path);
                    if fs_state.files.contains_key(&key) {
                        fs_state.apply(&json!({"op": "delete_file", "path": path}));
                        bump_op(&mut metrics, "delete");
                    } else {
                        bump_op(&mut metrics, "noop");
                    }
                }
                6 => {
                    bump_op(&mut metrics, "hold");
                }
                7 => {
                    if !held.is_empty() {
                        held.remove(0);
                        bump_op(&mut metrics, "release");
                    } else {
                        bump_op(&mut metrics, "noop");
                    }
                }
                8 => {
                    cancel = true;
                    bump_op(&mut metrics, "cancel");
                }
                _ => bump_op(&mut metrics, "noop"),
            }
        }
        let materialized = fs_state.materialize();
        let batch = change_batch(&published_state, &materialized, true);
        let host = fs_state.build_host(&[]);
        let token = CancellationToken::default();
        let mut candidate = cache.begin(batch, &host, &options, &program_options);
        let cancel_at = if cancel {
            rng.below(requests.len())
        } else {
            usize::MAX
        };
        let mut aborted = false;
        let mut entries = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            if index == cancel_at {
                token.cancel();
            }
            match soak_resolve(&mut candidate, *request, &token) {
                Ok(entry) => entries.push(entry),
                Err(CacheError::Cancelled { .. }) => {
                    aborted = true;
                    break;
                }
                Err(error) => panic!("soak generation {generation}: {error}"),
            }
        }
        if aborted {
            let before = cache.current();
            drop(candidate);
            assert_eq!(before.id(), cache.current().id());
            bump(&mut metrics, "cancelled", 1);
            continue;
        }
        // Parity of every entry against a fresh resolver on the same host.
        for (request, entry) in requests.iter().zip(&entries) {
            let fresh = fresh_value(
                &soak_request_json(*request),
                &host,
                &options,
                &program_options,
            )
            .expect("fresh");
            assert!(
                values_agree(entry.value(), &fresh),
                "soak generation {generation}: parity lost for {}",
                entry.key().display()
            );
            bump(&mut metrics, "parity_checks", 1);
            if let Disposition::Fresh {
                evicted_before: true,
            } = entry_disposition(&candidate, entry)
            {
                bump(&mut metrics, "fresh_after_eviction", 1);
                evicted_pending.remove(&entry.key().display());
            }
        }
        for row in candidate.trace() {
            match &row.disposition {
                Disposition::Reused => bump(&mut metrics, "reused", 1),
                Disposition::Recomputed { changed: true, .. } => {
                    bump(&mut metrics, "recomputed_changed", 1)
                }
                Disposition::Recomputed { changed: false, .. } => {
                    bump(&mut metrics, "recomputed_unchanged", 1)
                }
                Disposition::Fresh { .. } => bump(&mut metrics, "fresh", 1),
            }
        }
        let host_stats = candidate.host_stats();
        bump(&mut metrics, "host_calls", host_stats.host_calls as u64);
        bump(&mut metrics, "memo_hits", host_stats.memo_hits as u64);
        let hold = metrics["ops"]["hold"].as_u64().unwrap() > 0 && rng.below(3) == 0;
        let (handle, stats) = match cache.publish(candidate) {
            Ok(published) => published,
            Err(CacheError::LiveGenerationLimit { .. }) => {
                // Bounded owners: release the oldest reader and retry from
                // the published state on the next generation.
                bump(&mut metrics, "publish_refused_live_limit", 1);
                held.remove(0);
                continue;
            }
            Err(error) => panic!("soak publish {generation}: {error}"),
        };
        published_state = materialized;
        bump(&mut metrics, "generations", 1);
        bump(&mut metrics, "evicted", stats.evicted as u64);
        if stats.evicted > 0 {
            for entry in requests.iter().zip(&entries).map(|(_, entry)| entry) {
                if handle.view().get(entry.key()).is_none() {
                    evicted_pending.insert(entry.key().display());
                }
            }
        }
        let live = cache.live_generation_count() as u64;
        for (name, value) in [
            ("max_entries", stats.entries as u64),
            ("max_bytes", stats.bytes as u64),
            ("max_live_generations", live),
        ] {
            if metrics[name].as_u64().unwrap() < value {
                metrics[name] = json!(value);
            }
        }
        // Held readers keep parity on their own host state.
        for (held_generation, held_handle, held_host) in &held {
            for entry in held_handle.view().entries() {
                let request = requests
                    .iter()
                    .copied()
                    .find(|request| soak_request_matches(*request, entry.key()));
                let Some(request) = request else { continue };
                let fresh = fresh_value(
                    &soak_request_json(request),
                    held_host,
                    &options,
                    &program_options,
                )
                .expect("fresh");
                assert!(values_agree(entry.value(), &fresh), "soak generation {generation}: held generation {held_generation} lost parity for {}", entry.key().display());
                bump(&mut metrics, "held_reader_checks", 1);
            }
        }
        if hold && held.len() < 6 {
            held.push((handle.id(), handle.clone(), host.clone()));
        }
    }
    metrics["final_entries"] = json!(cache.current().view().len());
    metrics["final_bytes"] = json!(cache.current().view().estimated_bytes());
    metrics["resident"] = cache.resident_stats().to_json();
    metrics["limits"] = json!({"max_entries": limits.max_entries, "max_bytes": limits.max_bytes, "max_live_generations": limits.max_live_generations, "max_eviction_history": limits.max_eviction_history});
    metrics["seed"] = json!(format!("{seed:016x}"));
    metrics
}

fn entry_disposition(
    candidate: &tsc_program::Candidate<'_>,
    entry: &Rc<CacheEntry>,
) -> Disposition {
    candidate
        .trace()
        .iter()
        .find(|row| row.key == *entry.key())
        .map(|row| row.disposition.clone())
        .unwrap_or(Disposition::Reused)
}

fn soak_request_matches(request: SoakRequest, key: &tsc_program::RequestKey) -> bool {
    let (kind, containing, specifier) = match request {
        SoakRequest::Module(containing, specifier) => (RequestKind::Module, containing, specifier),
        SoakRequest::TypeReference(containing, specifier, false) => {
            (RequestKind::TypeReference, containing, specifier)
        }
        SoakRequest::TypeReference(containing, specifier, true) => {
            (RequestKind::AutomaticTypeReference, containing, specifier)
        }
        SoakRequest::Library(containing, specifier) => {
            (RequestKind::Library, containing, specifier)
        }
        SoakRequest::PackageScope(file) => (
            RequestKind::PackageScope,
            file,
            file.rsplit('/').next().unwrap(),
        ),
    };
    key.kind() == kind
        && key.specifier() == specifier
        && key.containing_directory().to_string() == parent_of(containing).unwrap_or_default()
}

#[test]
fn resolution_cache_seeded_soak_is_bounded_and_deterministic() {
    let first = run_soak(SOAK_SEED);
    let second = run_soak(SOAK_SEED);
    assert_eq!(first, second, "soak metrics are not deterministic");
    write_json("soak.json", &first);
    let limits = first["limits"].clone();
    assert!(first["max_entries"].as_u64().unwrap() <= limits["max_entries"].as_u64().unwrap());
    assert!(first["max_bytes"].as_u64().unwrap() <= limits["max_bytes"].as_u64().unwrap());
    assert!(
        first["max_live_generations"].as_u64().unwrap()
            <= limits["max_live_generations"].as_u64().unwrap()
    );
    let published = first["generations"].as_u64().unwrap();
    let cancelled = first["cancelled"].as_u64().unwrap();
    let refused = first["publish_refused_live_limit"].as_u64().unwrap();
    assert_eq!(
        published + cancelled + refused,
        SOAK_GENERATIONS as u64,
        "every generation publishes, cancels, or is refused: {first}"
    );
    assert!(published >= 600, "most generations must publish: {first}");
    assert!(
        refused > 0,
        "the live-generation bound must be exercised: {first}"
    );
    assert!(
        first["evicted"].as_u64().unwrap() > 0,
        "eviction must be exercised"
    );
    assert!(
        first["fresh_after_eviction"].as_u64().unwrap() > 0,
        "post-eviction refetch must be exercised"
    );
    assert!(
        first["held_reader_checks"].as_u64().unwrap() > 0,
        "held readers must be exercised"
    );
    assert!(
        first["resident"]["eviction_history_len"].as_u64().unwrap()
            <= limits["max_eviction_history"].as_u64().unwrap(),
        "eviction history must stay within its bound: {first}"
    );
}

// ---------------------------------------------------------------------------
// Review R1 regressions (review-01 R1–R4) and resident-state churn
// ---------------------------------------------------------------------------

fn review_host() -> MemoryCompilerHost {
    MemoryCompilerHost::builder_js("/p")
        .file_js("/p/main.ts", b"import 'pkg';".to_vec())
        .file_js(
            "/p/node_modules/pkg/package.json",
            br#"{"name":"pkg","exports":{"a,b":"./joined.d.ts","a":"./split.d.ts"}}"#.to_vec(),
        )
        .file_js("/p/node_modules/pkg/joined.d.ts", b"export {};".to_vec())
        .file_js("/p/node_modules/pkg/split.d.ts", b"export {};".to_vec())
        .file_js("/p/util.ts", b"export {};".to_vec())
        .build()
        .expect("review host")
}

fn review_options(conditions: &[&str]) -> CompilerOptions {
    CompilerOptions {
        module_resolution: Some(99),
        module: Some(199),
        custom_conditions: Some(
            conditions
                .iter()
                .map(|entry| JsString::from(*entry))
                .collect(),
        ),
        ..CompilerOptions::default()
    }
}

#[test]
fn review_r1_distinct_condition_arrays_must_not_reuse_the_same_resolution() {
    let host = review_host();
    let program = ProgramOptions::default();
    let token = CancellationToken::default();
    let joined = review_options(&["a,b"]);
    let split = review_options(&["a", "b"]);
    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let mut first = cache.begin(ChangeBatch::default(), &host, &joined, &program);
    let original = first
        .resolve_module(
            "/p/main.ts".into(),
            "pkg".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    cache.publish(first).unwrap();
    let mut next = cache.begin(ChangeBatch::default(), &host, &split, &program);
    let reused = next
        .resolve_module(
            "/p/main.ts".into(),
            "pkg".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    let mut fresh_cache = ResolutionCache::new(RetentionLimits::default());
    let mut fresh = fresh_cache.begin(ChangeBatch::default(), &host, &split, &program);
    let expected = fresh
        .resolve_module(
            "/p/main.ts".into(),
            "pkg".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    assert_ne!(
        original.value(),
        expected.value(),
        "fixture must select different exports"
    );
    assert_eq!(reused.value(), expected.value());
    assert_eq!(
        value_record(expected.value())["resolved"]["resolvedFileName"],
        json!("/p/node_modules/pkg/split.d.ts")
    );
    assert_eq!(
        value_record(original.value())["resolved"]["resolvedFileName"],
        json!("/p/node_modules/pkg/joined.d.ts")
    );
}

#[test]
fn review_r2_dropping_a_candidate_must_not_mutate_a_held_published_entry() {
    let host = review_host();
    let options = review_options(&[]);
    let program = ProgramOptions::default();
    let token = CancellationToken::default();
    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let mut first = cache.begin(ChangeBatch::default(), &host, &options, &program);
    let original = first
        .resolve_module(
            "/p/main.ts".into(),
            "./util".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    let (held, _) = cache.publish(first).unwrap();
    let before = held.view().usage_stamps();
    let mut next = cache.begin(ChangeBatch::default(), &host, &options, &program);
    next.resolve_module(
        "/p/main.ts".into(),
        "./util".into(),
        ResolutionMode::CommonJs,
        &token,
    )
    .unwrap();
    assert_eq!(next.working_last_used(original.key()), Some(2));
    drop(next);
    assert_eq!(cache.published_id(), held.id());
    assert_eq!(held.view().usage_stamps(), before);
    assert_eq!(held.view().last_used(original.key()), Some(1));
}

#[test]
fn review_r3_a_candidate_from_another_cache_must_be_rejected() {
    let host = review_host();
    let options = review_options(&[]);
    let program = ProgramOptions::default();
    let token = CancellationToken::default();
    let mut source = ResolutionCache::new(RetentionLimits::default());
    let mut destination = ResolutionCache::new(RetentionLimits::default());
    let mut candidate = source.begin(ChangeBatch::default(), &host, &options, &program);
    candidate
        .resolve_module(
            "/p/main.ts".into(),
            "./util".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    let result = destination.publish(candidate);
    assert!(matches!(result, Err(CacheError::ForeignCandidate)));
    assert_eq!(destination.published_id(), 0);
    assert!(destination.current().view().is_empty());
}

/// Review R4: a workload whose keys and option identities never repeat must
/// keep the cache's resident state bounded, with and without held readers
/// and across repeated `evict_all` calls.
#[test]
fn review_r4_key_and_identity_churn_keeps_resident_state_bounded() {
    let host = review_host();
    let program = ProgramOptions::default();
    let limits = RetentionLimits {
        max_entries: 8,
        max_bytes: 64 << 10,
        max_live_generations: 6,
        max_eviction_history: 32,
    };
    let mut cache = ResolutionCache::new(limits);
    let token = CancellationToken::default();
    let mut held: Vec<GenerationHandle> = Vec::new();
    let mut samples = Vec::new();
    let mut max_history = 0u64;
    let mut max_history_bytes = 0u64;
    let mut max_live = 0u64;
    let mut evict_all_calls = 0u64;
    let mut evict_all_refused = 0u64;
    for generation in 0..2000u32 {
        // A new option identity and a new specifier every generation.
        let options = review_options(&[&format!("cond-{generation}")]);
        let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
        for request in 0..3 {
            candidate
                .resolve_module(
                    "/p/main.ts".into(),
                    format!("./gen-{generation}-{request}").as_str().into(),
                    ResolutionMode::CommonJs,
                    &token,
                )
                .unwrap();
        }
        let (handle, _) = cache.publish(candidate).unwrap();
        if generation % 7 == 0 && held.len() < 4 {
            held.push(handle);
        }
        if generation % 11 == 0 && !held.is_empty() {
            held.remove(0);
        }
        if generation % 97 == 0 {
            evict_all_calls += 1;
            match cache.evict_all() {
                Ok(()) => {}
                Err(CacheError::LiveGenerationLimit { .. }) => evict_all_refused += 1,
                Err(error) => panic!("evict_all: {error}"),
            }
            // Repeated evict_all with the same held readers.
            if cache.evict_all().is_err() {
                evict_all_refused += 1;
            }
            evict_all_calls += 1;
        }
        let resident = cache.resident_stats();
        assert!(resident.published_entries <= limits.max_entries);
        assert!(resident.eviction_history_len <= limits.max_eviction_history);
        assert!(resident.live_generation_records <= limits.max_live_generations);
        max_history = max_history.max(resident.eviction_history_len as u64);
        max_history_bytes = max_history_bytes.max(resident.eviction_history_bytes as u64);
        max_live = max_live.max(resident.live_generation_records as u64);
        if generation % 400 == 0 || generation == 1999 {
            samples.push(json!({"generation": generation, "resident": resident.to_json(), "held_readers": held.len()}));
        }
    }
    let final_resident = cache.resident_stats();
    let report = json!({
        "generations": 2000,
        "requests_per_generation": 3,
        "limits": {"max_entries": limits.max_entries, "max_bytes": limits.max_bytes, "max_live_generations": limits.max_live_generations, "max_eviction_history": limits.max_eviction_history},
        "max_eviction_history_len": max_history,
        "max_eviction_history_bytes": max_history_bytes,
        "max_live_generation_records": max_live,
        "evict_all_calls": evict_all_calls,
        "evict_all_refused_by_live_bound": evict_all_refused,
        "final": final_resident.to_json(),
        "samples": samples,
    });
    write_json("churn.json", &report);
    assert!(
        final_resident.eviction_history_dropped > 0,
        "history must have been trimmed: {report}"
    );
    assert!(
        evict_all_refused > 0 || held.is_empty(),
        "the live bound was exercised or no readers remained: {report}"
    );
}

#[test]
fn observation_host_records_negative_and_positive_facts() {
    let host = MemoryCompilerHost::builder_js("/p")
        .file_js("/p/util.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    let observation = ObservationHost::new(&host);
    let (_, dependencies) = observation.record(|observed| {
        assert!(observed.file_exists_js("/p/util.ts".into()).unwrap());
        assert!(!observed.file_exists_js("/p/missing.ts".into()).unwrap());
        assert!(!observed.directory_exists_js("/p/lib".into()).unwrap());
        assert!(observed
            .read_file_js("/p/util.ts".into())
            .unwrap()
            .is_some());
        // Second read is a memo hit and still records the same fact once.
        assert!(observed
            .read_file_js("/p/util.ts".into())
            .unwrap()
            .is_some());
    });
    assert_eq!(dependencies.len(), 4);
    let stats = observation.stats();
    assert_eq!(stats.host_calls, 4);
    assert_eq!(stats.memo_hits, 1);
    let mut batch = ChangeBatch::default();
    batch
        .created_files
        .insert(PathKey::new("/p/missing.ts".into(), true));
    assert!(batch.violated_dependency(&dependencies).is_some());
    let mut batch = ChangeBatch::default();
    batch
        .changed_files
        .insert(PathKey::new("/p/unrelated.ts".into(), true));
    assert!(batch.violated_dependency(&dependencies).is_none());
    let _ = HostModuleResolution::diagnostics;
    let _: Option<RefCell<()>> = None;
}

#[test]
fn shrinking_history_limit_applies_without_new_victims() {
    let host = review_host();
    let options = review_options(&[]);
    let program = ProgramOptions::default();
    let token = CancellationToken::default();
    let mut limits = RetentionLimits {
        max_entries: 0,
        ..RetentionLimits::default()
    };
    let mut cache = ResolutionCache::new(limits);
    let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
    candidate
        .resolve_module(
            "/p/main.ts".into(),
            "./util".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    cache.publish(candidate).unwrap();
    assert_eq!(cache.resident_stats().eviction_history_len, 1);
    limits.max_eviction_history = 0;
    cache.set_limits(limits);
    let candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
    cache.publish(candidate).unwrap();
    assert_eq!(cache.resident_stats().eviction_history_len, 0);
}
#[test]
fn published_byte_limit_includes_retained_option_identity() {
    let host = review_host();
    let options = review_options(&[&"a".repeat(65536)]);
    let program = ProgramOptions::default();
    let token = CancellationToken::default();
    let mut cache = ResolutionCache::new(RetentionLimits {
        max_bytes: 4096,
        ..RetentionLimits::default()
    });
    let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
    candidate
        .resolve_module(
            "/p/main.ts".into(),
            "./util".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    let (_, stats) = cache.publish(candidate).unwrap();
    eprintln!(
        "published entries={} reported_bytes={}, single option text=65536 bytes",
        stats.entries, stats.bytes
    );
    assert_eq!(
        stats.entries, 0,
        "oversize option identity is retained by published key"
    );
    let resident = cache.resident_stats();
    assert_eq!(
        resident.eviction_history_len, 0,
        "history must also obey the byte bound"
    );
    assert_eq!(
        resident.interned_identity_bytes, 0,
        "intern slot must not keep the oversized identity alive"
    );
}

#[test]
fn case_insensitive_type_root_spelling_change_matches_fresh() {
    let host = MemoryCompilerHost::builder_js("/p")
        .case_sensitive(false)
        .file_js("/p/types/pkg/index.d.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    let options = CompilerOptions {
        module_resolution: Some(99),
        module: Some(199),
        ..CompilerOptions::default()
    };
    let lower = ProgramOptions::default().with_type_roots(vec![ProgramPath::from_js_parts(
        "/p/types".into(),
        "/p/types".into(),
    )
    .unwrap()]);
    let upper = ProgramOptions::default().with_type_roots(vec![ProgramPath::from_js_parts(
        "/p/TYPES".into(),
        "/p/types".into(),
    )
    .unwrap()]);
    let token = CancellationToken::default();
    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let mut first = cache.begin(ChangeBatch::default(), &host, &options, &lower);
    let before = first
        .resolve_type_reference(
            "/p/main.ts".into(),
            "pkg".into(),
            ResolutionMode::CommonJs,
            false,
            &token,
        )
        .unwrap();
    cache.publish(first).unwrap();
    let mut next = cache.begin(ChangeBatch::default(), &host, &options, &upper);
    let after = next
        .resolve_type_reference(
            "/p/main.ts".into(),
            "pkg".into(),
            ResolutionMode::CommonJs,
            false,
            &token,
        )
        .unwrap();
    let mut fresh_cache = ResolutionCache::new(RetentionLimits::default());
    let mut fresh = fresh_cache.begin(ChangeBatch::default(), &host, &options, &upper);
    let expected = fresh
        .resolve_type_reference(
            "/p/main.ts".into(),
            "pkg".into(),
            ResolutionMode::CommonJs,
            false,
            &token,
        )
        .unwrap();
    eprintln!(
        "before={} after={} fresh={}",
        before.value().summary_json(),
        after.value().summary_json(),
        expected.value().summary_json()
    );
    assert_eq!(after.value(), expected.value());
}

#[test]
fn containing_directory_spelling_matches_fresh() {
    let host = MemoryCompilerHost::builder_js("/p")
        .case_sensitive(false)
        .file_js("/p/util.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    let options = CompilerOptions {
        module_resolution: Some(99),
        module: Some(199),
        ..CompilerOptions::default()
    };
    let program = ProgramOptions::default();
    let token = CancellationToken::default();
    let mut cache = ResolutionCache::new(RetentionLimits::default());
    let mut c = cache.begin(ChangeBatch::default(), &host, &options, &program);
    c.resolve_module(
        "/p/main.ts".into(),
        "./util".into(),
        ResolutionMode::CommonJs,
        &token,
    )
    .unwrap();
    let after = c
        .resolve_module(
            "/P/main.ts".into(),
            "./util".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    let mut fresh_cache = ResolutionCache::new(RetentionLimits::default());
    let mut fresh = fresh_cache.begin(ChangeBatch::default(), &host, &options, &program);
    let expected = fresh
        .resolve_module(
            "/P/main.ts".into(),
            "./util".into(),
            ResolutionMode::CommonJs,
            &token,
        )
        .unwrap();
    eprintln!(
        "after={} fresh={}",
        after.value().summary_json(),
        expected.value().summary_json()
    );
    assert_eq!(after.value(), expected.value());
}

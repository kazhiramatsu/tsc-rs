//! tsgo `project/ata`: automatic type acquisition. The typings a project's
//! JavaScript needs are found ([`discover_typings`]) and installed from
//! npm's `@types` into a global cache ([`TypingsInstaller`]), whose files
//! the project then takes as roots.

mod discover_typings;
mod types_map;
mod validate_package_name;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use tsc_diagnostics::JsStr;
use tsc_host::vfs::{FileSystem, VfsCompilerHost};
use tsc_types::{Version, TYPESCRIPT_VERSION_MAJOR_MINOR};

pub use discover_typings::discover_typings;
pub use validate_package_name::{validate_package_name, NameValidationResult};

use discover_typings::{go_list, is_typing_up_to_date};
use validate_package_name::render_package_name_validation_failure;

/// tsgo `core.TypeAcquisition`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypeAcquisition {
    pub enable: Option<bool>,
    pub include: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub disable_filename_based_type_acquisition: Option<bool>,
}

/// tsgo `TypingsInfo`: what decides a project's typings.
#[derive(Clone, Debug)]
pub struct TypingsInfo {
    pub type_acquisition: TypeAcquisition,
    /// tsgo `CompilerOptions.GetAllowJS`.
    pub allow_js: bool,
    /// tsgo `CompilerOptions.Types != nil`: the config names the global
    /// typings, so no manifest is searched.
    pub has_types_option: bool,
    pub unresolved_imports: Option<BTreeSet<String>>,
}

impl PartialEq for TypingsInfo {
    /// tsgo `Equals`: the type acquisition, `allowJs` and the unresolved
    /// imports.
    fn eq(&self, other: &Self) -> bool {
        self.type_acquisition == other.type_acquisition
            && self.allow_js == other.allow_js
            && self.unresolved_imports == other.unresolved_imports
    }
}

/// tsgo `CachedTyping`: an installed typing's file and version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedTyping {
    pub typings_location: String,
    pub version: Version,
}

/// tsgo `NpmExecutor`: runs `npm` with `args` in `cwd`, its output or why
/// it failed.
pub trait NpmExecutor: Send + Sync {
    fn npm_install(&self, cwd: &str, args: &[String]) -> Result<Vec<u8>, String>;
}

/// tsgo `TypingsInstallRequest`.
pub struct TypingsInstallRequest<'a> {
    pub project_id: &'a str,
    pub typings_info: &'a TypingsInfo,
    pub file_names: &'a [String],
    pub project_root_path: &'a str,
    pub fs: &'a dyn FileSystem,
    pub logger: &'a mut dyn FnMut(String),
}

/// tsgo `TypingsInstallResult`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypingsInstallResult {
    pub typings_files: Vec<String>,
    pub files_to_watch: Vec<String>,
}

/// tsgo `TypingsInstaller`: the typings cache in `typings_location`, which
/// npm installs the registry and the `@types` packages into.
pub struct TypingsInstaller {
    typings_location: String,
    fs: Arc<dyn FileSystem>,
    npm_executor: Arc<dyn NpmExecutor>,
    /// tsgo `initOnce` and `typesRegistry`.
    types_registry: OnceLock<BTreeMap<String, BTreeMap<String, String>>>,
    package_name_to_typing_location: Mutex<BTreeMap<String, CachedTyping>>,
    missing_typings_set: Mutex<HashSet<String>>,
    install_run_count: AtomicI32,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// tsgo `tsVersionToUse`: the `@types` version installed.
const TS_VERSION_TO_USE: &str = "latest";

impl TypingsInstaller {
    /// tsgo `NewTypingsInstaller`.
    pub fn new(
        typings_location: String,
        fs: Arc<dyn FileSystem>,
        npm_executor: Arc<dyn NpmExecutor>,
    ) -> Self {
        Self {
            typings_location,
            fs,
            npm_executor,
            types_registry: OnceLock::new(),
            package_name_to_typing_location: Mutex::default(),
            missing_typings_set: Mutex::default(),
            install_run_count: AtomicI32::new(0),
        }
    }

    /// tsgo `IsKnownTypesPackageName`: a valid npm name the registry has
    /// typings for.
    pub fn is_known_types_package_name(
        &self,
        name: &str,
        fs: &dyn FileSystem,
        logger: &mut dyn FnMut(String),
    ) -> bool {
        if validate_package_name(name).0 != NameValidationResult::NameOk {
            return false;
        }
        self.types_registry(fs, logger).contains_key(name)
    }

    /// tsgo `InstallTypings`: the project's typing files (sorted) and the
    /// files to watch for a change of them.
    pub fn install_typings(
        &self,
        request: TypingsInstallRequest<'_>,
    ) -> Result<TypingsInstallResult, String> {
        let project_id = request.project_id;
        let logger = &mut *request.logger;
        let types_registry = self.types_registry(request.fs, logger);
        let (cached_typing_paths, new_typing_names, files_to_watch) = {
            let cache = lock(&self.package_name_to_typing_location).clone();
            discover_typings(
                request.fs,
                logger,
                request.typings_info,
                request.file_names,
                request.project_root_path,
                &cache,
                types_registry,
            )
        };
        let request_id = self.install_run_count.fetch_add(1, Ordering::SeqCst) + 1;
        let mut result = TypingsInstallResult {
            typings_files: cached_typing_paths,
            files_to_watch,
        };
        if !new_typing_names.is_empty() {
            let filtered_typings = self.filter_typings(logger, &new_typing_names, types_registry);
            if !filtered_typings.is_empty() {
                result.typings_files = self.install(
                    request_id,
                    result.typings_files,
                    &filtered_typings,
                    types_registry,
                    logger,
                )?;
            } else {
                logger(
                    "ATA:: All typings are known to be missing or invalid - no need to install more typings"
                        .to_owned(),
                );
            }
        } else {
            logger(
                "ATA:: No new typings were requested as a result of typings discovery".to_owned(),
            );
        }
        result.typings_files.sort();
        result.files_to_watch.sort();
        logger(format!("ATA:: Got install request for: {project_id}"));
        Ok(result)
    }

    /// tsgo `installTypings`: npm installs the `@types` packages, whose
    /// typing files join the cached ones; on a failure the packages are
    /// marked missing.
    fn install(
        &self,
        request_id: i32,
        currently_cached_typings: Vec<String>,
        filtered_typings: &[String],
        types_registry: &BTreeMap<String, BTreeMap<String, String>>,
        logger: &mut dyn FnMut(String),
    ) -> Result<Vec<String>, String> {
        let scoped_typings = filtered_typings
            .iter()
            .map(|package_name| format!("@types/{package_name}@{TS_VERSION_TO_USE}"))
            .collect::<Vec<_>>();
        if self.install_worker(request_id, &scoped_typings, logger) {
            logger(format!(
                "ATA:: Installed typings {}",
                go_list(&scoped_typings)
            ));
            let mut installed_typing_files = Vec::new();
            for package_name in filtered_typings {
                let Some(typing_file) = self.typing_to_file_name(package_name) else {
                    logger(format!(
                        "ATA:: Failed to find typing file for package '{package_name}'"
                    ));
                    lock(&self.missing_typings_set).insert(package_name.clone());
                    continue;
                };
                // The registry has the package (the filter kept only those).
                let dist_tags = &types_registry[package_name];
                let use_version = dist_tags
                    .get(&format!("ts{TYPESCRIPT_VERSION_MAJOR_MINOR}"))
                    .or_else(|| dist_tags.get("latest"));
                if let Some(version) = use_version.and_then(|version| Version::parse(version)) {
                    lock(&self.package_name_to_typing_location).insert(
                        package_name.clone(),
                        CachedTyping {
                            typings_location: typing_file.clone(),
                            version,
                        },
                    );
                }
                installed_typing_files.push(typing_file);
            }
            logger(format!(
                "ATA:: Installed typing files {}",
                go_list(&installed_typing_files)
            ));
            let mut typings = currently_cached_typings;
            typings.extend(installed_typing_files);
            return Ok(typings);
        }
        logger(format!(
            "ATA:: install request failed, marking packages as missing to prevent repeated requests: {}",
            go_list(filtered_typings)
        ));
        lock(&self.missing_typings_set).extend(filtered_typings.iter().cloned());
        Err("npm install failed".to_owned())
    }

    /// tsgo `installWorker`: `npm install --ignore-scripts` of the packages
    /// in the cache.
    fn install_worker(
        &self,
        request_id: i32,
        package_names: &[String],
        logger: &mut dyn FnMut(String),
    ) -> bool {
        logger(format!(
            "ATA:: #{request_id} with cwd: {} arguments: {}",
            self.typings_location,
            go_list(package_names)
        ));
        let result = install_npm_packages(package_names, |packages| {
            let mut args = vec!["install".to_owned(), "--ignore-scripts".to_owned()];
            args.extend(packages.iter().cloned());
            args.push("--save-dev".to_owned());
            args.push(format!(
                "--user-agent=\"typesInstaller/{}\"",
                tsc_types::TYPESCRIPT_VERSION
            ));
            self.npm_executor
                .npm_install(&self.typings_location, &args)
                .map(|_| ())
                .inspect_err(|error| logger(format!("ATA:: Output is: {error}")))
        });
        logger(format!("TI:: npm install #{request_id} completed"));
        result.is_ok()
    }

    /// tsgo `filterTypings`: the typings to install — valid npm names the
    /// registry has, not known to be missing, without an up-to-date typing
    /// in the cache.
    fn filter_typings(
        &self,
        logger: &mut dyn FnMut(String),
        typings_to_install: &[String],
        types_registry: &BTreeMap<String, BTreeMap<String, String>>,
    ) -> Vec<String> {
        let mut result = Vec::new();
        for typing in typings_to_install {
            let typing_key = tsc_program::mangle_scoped_package_name(JsStr::from_str(typing))
                .to_string_lossy()
                .into_owned();
            if lock(&self.missing_typings_set).contains(&typing_key) {
                logger(format!(
                    "ATA:: '{typing}':: '{typing_key}' is in missingTypingsSet - skipping..."
                ));
                continue;
            }
            let (validation_result, name, is_scope_name) = validate_package_name(typing);
            if validation_result != NameValidationResult::NameOk {
                // Never processed again.
                lock(&self.missing_typings_set).insert(typing_key);
                logger(format!(
                    "ATA:: {}",
                    render_package_name_validation_failure(
                        typing,
                        validation_result,
                        &name,
                        is_scope_name
                    )
                ));
                continue;
            }
            let Some(types_registry_entry) = types_registry.get(&typing_key) else {
                logger(format!(
                    "ATA:: '{typing}':: Entry for package '{typing_key}' does not exist in local types registry - skipping..."
                ));
                continue;
            };
            if lock(&self.package_name_to_typing_location)
                .get(&typing_key)
                .is_some_and(|location| is_typing_up_to_date(location, types_registry_entry))
            {
                logger(format!(
                    "ATA:: '{typing}':: '{typing_key}' already has an up-to-date typing - skipping..."
                ));
                continue;
            }
            result.push(typing_key);
        }
        result
    }

    /// tsgo `init` (once) and `typesRegistry`: the cache's typings, the
    /// registry updated by npm and read.
    fn types_registry(
        &self,
        fs: &dyn FileSystem,
        logger: &mut dyn FnMut(String),
    ) -> &BTreeMap<String, BTreeMap<String, String>> {
        self.types_registry.get_or_init(|| {
            logger(format!(
                "ATA:: Global cache location '{}'",
                self.typings_location
            ));
            self.process_cache_location(fs, logger);
            self.ensure_typings_location_exists(fs, logger);
            logger("ATA:: Updating types-registry@latest npm package...".to_owned());
            let args = ["install", "--ignore-scripts", "types-registry@latest"].map(str::to_owned);
            match self.npm_executor.npm_install(&self.typings_location, &args) {
                Ok(_) => logger("ATA:: Updated types-registry npm package".to_owned()),
                Err(error) => logger(format!(
                    "ATA:: Error updating types-registry package: {error}"
                )),
            }
            self.load_types_registry_file(fs, logger)
        })
    }

    /// tsgo `processCacheLocation`: the typings the cache's `package.json`
    /// and lock file list (installed with a version) are cached.
    fn process_cache_location(&self, fs: &dyn FileSystem, logger: &mut dyn FnMut(String)) {
        logger(format!(
            "ATA:: Processing cache location {}",
            self.typings_location
        ));
        let package_json = self.resolve(&["package.json"]);
        let package_lock_json = self.resolve(&["package-lock.json"]);
        logger(format!("ATA:: Trying to find '{package_json}'..."));
        if fs.is_file(&package_json) && fs.is_file(&package_lock_json) {
            let read = |path: &str| {
                fs.read(path)
                    .ok()
                    .and_then(|bytes| tsc_program::decode_host_text(bytes).ok())
                    .unwrap_or_default()
            };
            let npm_config_contents = read(&package_json);
            let npm_lock_contents = read(&package_lock_json);
            logger(format!(
                "ATA:: Loaded content of {package_json}: {npm_config_contents}"
            ));
            logger(format!(
                "ATA:: Loaded content of {package_lock_json}: {npm_lock_contents}"
            ));
            let parse = |text: &str| {
                serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::Value::Null)
            };
            let (npm_config, npm_lock) = (parse(&npm_config_contents), parse(&npm_lock_contents));
            let object = |value: &serde_json::Value, field: &str| {
                value
                    .get(field)
                    .and_then(serde_json::Value::as_object)
                    .cloned()
            };
            let dev_dependencies = object(&npm_config, "devDependencies");
            let lock_packages = object(&npm_lock, "packages");
            let lock_dependencies = object(&npm_lock, "dependencies");
            if let (Some(dev_dependencies), true) = (
                dev_dependencies,
                lock_packages.is_some() || lock_dependencies.is_some(),
            ) {
                for key in dev_dependencies.keys() {
                    let lock_entry = lock_packages
                        .as_ref()
                        .and_then(|packages| packages.get(&format!("node_modules/{key}")))
                        .or_else(|| {
                            lock_dependencies
                                .as_ref()
                                .and_then(|dependencies| dependencies.get(key))
                        });
                    // A package without a lock entry is installed again on
                    // its next use.
                    let Some(lock_entry) = lock_entry else {
                        continue;
                    };
                    // The key is `@types/<package name>`.
                    let package_name = key.rsplit('/').next().unwrap_or_default();
                    if package_name.is_empty() {
                        continue;
                    }
                    let Some(typing_file) = self.typing_to_file_name(package_name) else {
                        lock(&self.missing_typings_set).insert(package_name.to_owned());
                        continue;
                    };
                    let mut cache = lock(&self.package_name_to_typing_location);
                    if let Some(existing) = cache.get(package_name) {
                        if existing.typings_location == typing_file {
                            continue;
                        }
                        logger(format!(
                            "ATA:: New typing for package {package_name} from {typing_file} conflicts with existing typing file {}",
                            existing.typings_location
                        ));
                    }
                    logger(format!(
                        "ATA:: Adding entry into typings cache: {package_name} => {typing_file}"
                    ));
                    let Some(version) = lock_entry
                        .get("version")
                        .and_then(serde_json::Value::as_str)
                        .filter(|version| !version.is_empty())
                    else {
                        continue;
                    };
                    if let Some(version) = Version::parse(version) {
                        cache.insert(
                            package_name.to_owned(),
                            CachedTyping {
                                typings_location: typing_file,
                                version,
                            },
                        );
                    }
                }
            }
        }
        logger(format!(
            "ATA:: Finished processing cache location {}",
            self.typings_location
        ));
    }

    /// tsgo `ensureTypingsLocationExists`: a private `package.json` in the
    /// cache.
    fn ensure_typings_location_exists(&self, fs: &dyn FileSystem, logger: &mut dyn FnMut(String)) {
        let npm_config_path = self.resolve(&["package.json"]);
        logger(format!("ATA:: Npm config file: {npm_config_path}"));
        if !fs.is_file(&npm_config_path) {
            logger(format!(
                "ATA:: Npm config file: '{npm_config_path}' is missing, creating new one..."
            ));
            if let Err(error) = fs.write_creating_dirs(&npm_config_path, b"{ \"private\": true }") {
                logger(format!("ATA:: Npm config file write failed: {error}"));
            }
        }
    }

    /// tsgo `typingToFileName`: where the cache's `@types` package of the
    /// package resolves (NodeNext, from the cache).
    fn typing_to_file_name(&self, package_name: &str) -> Option<String> {
        let host = VfsCompilerHost::new(Arc::clone(&self.fs), self.typings_location.clone());
        let options = tsc_program::CompilerOptions {
            module_resolution: Some(99),
            ..tsc_program::CompilerOptions::default()
        };
        let mut resolver = tsc_program::ModuleResolver::new(&host, &options).ok()?;
        match resolver
            .resolve(
                JsStr::from_str(&self.resolve(&["index.d.ts"])),
                JsStr::from_str(package_name),
                tsc_program::ResolutionMode::Unspecified,
            )
            .ok()?
        {
            tsc_program::ResolutionOutcome::Resolved(module) => Some(
                module
                    .resolved_file()
                    .display()
                    .to_string_lossy()
                    .into_owned(),
            ),
            tsc_program::ResolutionOutcome::NotFound => None,
        }
    }

    /// tsgo `loadTypesRegistryFile`: the registry's `entries`, each package's
    /// versions by tag.
    fn load_types_registry_file(
        &self,
        fs: &dyn FileSystem,
        logger: &mut dyn FnMut(String),
    ) -> BTreeMap<String, BTreeMap<String, String>> {
        let types_registry_file = self.resolve(&["node_modules", "types-registry", "index.json"]);
        let Some(contents) = fs
            .read(&types_registry_file)
            .ok()
            .and_then(|bytes| tsc_program::decode_host_text(bytes).ok())
        else {
            logger(format!(
                "ATA:: Error reading types registry file '{types_registry_file}'"
            ));
            return BTreeMap::new();
        };
        match serde_json::from_str::<BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>>(
            &contents,
        ) {
            Ok(mut entries) => {
                if let Some(types_registry) = entries.remove("entries") {
                    return types_registry;
                }
                logger(format!(
                    "ATA:: Error when loading types registry file '{types_registry_file}': <nil>"
                ));
            }
            Err(error) => logger(format!(
                "ATA:: Error when loading types registry file '{types_registry_file}': {error}"
            )),
        }
        BTreeMap::new()
    }

    fn resolve(&self, segments: &[&str]) -> String {
        let mut path = self.typings_location.trim_end_matches('/').to_owned();
        for segment in segments {
            path.push('/');
            path.push_str(segment);
        }
        path
    }
}

/// tsgo `installNpmPackages`: the packages in commands of under 8,000
/// characters, each run (all of them, also after one failed); the first
/// failure is the result.
pub(crate) fn install_npm_packages(
    package_names: &[String],
    mut install_packages: impl FnMut(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    let mut first_error = None;
    let mut run = |packages: &[String]| {
        if let Err(error) = install_packages(packages) {
            first_error.get_or_insert(error);
        }
    };
    let mut start = 0;
    let mut end = 0;
    let mut command_size = 100;
    for package_name in package_names {
        command_size += package_name.len() + 1;
        if command_size < 8000 {
            end += 1;
        } else {
            run(&package_names[start..end]);
            start = end;
            command_size = 100 + package_name.len() + 1;
            end += 1;
        }
    }
    if start < package_names.len() {
        run(&package_names[start..end]);
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
#[path = "../../tests/unit/ata.rs"]
mod tests;

//! tsgo `ls/autoimport/aliasresolver.go`: the checker the registry
//! extracts a package's exports with. Its program's source files are the
//! package's entrypoints; a module the checker resolves from them is read
//! outside the program, so its declarations join no global scope. A
//! non-relative module the checker cannot resolve is recorded: it may be an
//! ambient module another package declares (the registry's second pass).
//!
//! tsgo reads a module's file when the checker first resolves it, a
//! JavaScript one included (`GetSourceFileForResolvedModule` parses any
//! file, and an alias resolution reports no resolution diagnostic); tsc-rs's
//! checker takes its files up front, so every module the files' requests
//! resolve to is read before the checker is made, and the checker's lookups
//! are answered from those resolutions.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use tsc_checker::live::{LiveChecker, LiveCheckerInputs};
use tsc_checker::state::CheckerState;
use tsc_checker::{
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
    AuthoritativeModuleResolution, AuthoritativeNotFoundModule, AuthoritativePackageId,
    AuthoritativeResolutionMode, AuthoritativeResolvedModule, AuthoritativeSourceMetadata,
    AuthoritativeSourceToken, AuthoritativeUntypedModule, CompilerOptions, InputFile,
};
use tsc_host::CompilerHost;
use tsc_program::{
    HostResolvedModule, ModuleExtension, ModuleResolver, PreparedSourceFile, ProgramPath,
    ResolutionMode, ResolutionOutcome, WorkerBudget,
};
use tsc_types::{JsStr, JsString};

/// tsgo `onFailedAmbientModuleLookup`'s records.
#[derive(Debug, Default)]
pub(super) struct FailedLookups {
    /// The module names (tsgo `failedAmbientModuleLookupTargets`).
    pub(super) targets: BTreeSet<String>,
    /// The files that asked, by path, with their names (tsgo
    /// `failedAmbientModuleLookupSources`).
    pub(super) sources: BTreeMap<String, String>,
}

/// tsgo `aliasResolver` with its checker.
pub(super) struct AliasResolver {
    pub(super) checker: LiveChecker,
    /// The checker's files, the program's files first: name and path.
    pub(super) files: Vec<(String, String)>,
    /// How many of `files` are the program's.
    pub(super) program_file_count: usize,
    pub(super) failed: Arc<Mutex<FailedLookups>>,
}

/// One file the checker reads.
struct AliasFile {
    file_name: String,
    path: String,
    text: String,
    /// The module requests' resolutions, by specifier.
    resolutions: Vec<(String, AuthoritativeModuleResolution)>,
}

impl AliasResolver {
    /// tsgo `newAliasResolver` with `checker.NewChecker`: a program of
    /// `program_files`, with `other_files` read outside it (the second
    /// pass's files that failed a lookup); a file that cannot be read is
    /// left out. `resolver` resolves the checker's modules (tsgo's
    /// `GetResolvedModule`, ESM mode); failed lookups are recorded unless
    /// `record_failures` is off.
    pub(super) fn new(
        host: &dyn CompilerHost,
        resolver: &mut ModuleResolver<'_>,
        program_files: &[String],
        other_files: &[String],
        path_of: &dyn Fn(&str) -> String,
        record_failures: bool,
    ) -> Option<Self> {
        let options = CompilerOptions::default();
        let mut files: Vec<AliasFile> = Vec::new();
        let mut index_by_path: HashMap<String, usize> = HashMap::new();
        let mut read = |file_name: &str, files: &mut Vec<AliasFile>| -> Option<usize> {
            let path = path_of(file_name);
            if let Some(&index) = index_by_path.get(&path) {
                return Some(index);
            }
            let text = host
                .read_file_js(JsStr::from_str(file_name))
                .ok()
                .flatten()
                .and_then(|bytes| tsc_program::decode_host_text(bytes).ok())?;
            index_by_path.insert(path.clone(), files.len());
            files.push(AliasFile {
                file_name: file_name.to_owned(),
                path,
                text,
                resolutions: Vec::new(),
            });
            Some(files.len() - 1)
        };
        for file_name in program_files {
            read(file_name, &mut files);
        }
        let program_file_count = files.len();
        if program_file_count == 0 && other_files.is_empty() {
            return None;
        }
        for file_name in other_files {
            read(file_name, &mut files);
        }
        let mut next = 0;
        while next < files.len() {
            let source = {
                let file = &files[next];
                let path = ProgramPath::from_js_parts(
                    JsStr::from_str(&file.file_name),
                    JsStr::from_str(&file.path),
                )
                .ok()?;
                PreparedSourceFile::new(path, file.text.clone())
            };
            let specifiers: Vec<String> = tsc_program::plan_source_requests(&source, &options)
                .map(|plan| {
                    let mut seen = BTreeSet::new();
                    plan.module_requests()
                        .iter()
                        .map(|key| key.specifier().to_string_lossy().into_owned())
                        .filter(|specifier| seen.insert(specifier.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let containing = files[next].file_name.clone();
            for specifier in specifiers {
                let outcome = resolver
                    .resolve(
                        JsStr::from_str(&containing),
                        JsStr::from_str(&specifier),
                        ResolutionMode::EsNext,
                    )
                    .unwrap_or(ResolutionOutcome::NotFound);
                let resolution = match outcome {
                    ResolutionOutcome::Resolved(module) if is_source(module.extension()) => {
                        let target_name = module
                            .resolved_file()
                            .display()
                            .to_string_lossy()
                            .into_owned();
                        match read(&target_name, &mut files) {
                            Some(target) => resolved(&module, target),
                            None => AuthoritativeModuleResolution::NotFound(
                                AuthoritativeNotFoundModule::default(),
                            ),
                        }
                    }
                    ResolutionOutcome::Resolved(module) => untyped(&module),
                    ResolutionOutcome::NotFound => AuthoritativeModuleResolution::NotFound(
                        AuthoritativeNotFoundModule::default(),
                    ),
                };
                files[next].resolutions.push((specifier, resolution));
            }
            next += 1;
        }

        let failed = Arc::new(Mutex::new(FailedLookups::default()));
        let provider = AliasModuleProvider {
            resolutions: files
                .iter()
                .enumerate()
                .flat_map(|(index, file)| {
                    file.resolutions.iter().map(move |(specifier, resolution)| {
                        ((index as u32, specifier.clone()), resolution.clone())
                    })
                })
                .collect(),
            files: files
                .iter()
                .map(|file| (file.file_name.clone(), file.path.clone()))
                .collect(),
            failed: record_failures.then(|| Arc::clone(&failed)),
        };
        let file_metadata = files
            .iter()
            .enumerate()
            .map(|(index, file)| AuthoritativeSourceMetadata {
                token: AuthoritativeSourceToken(index as u32),
                file_name: JsString::from(file.file_name.as_str()),
                may_be_emitted: false,
                // tsgo `aliasResolver`'s format hooks (`GetImpliedNodeFormatForEmit`,
                // `GetEmitSyntaxForUsageLocation`, `GetDefaultResolutionModeForFile`,
                // ...) answer ESNext for every file: a `.d.cts` file's
                // `export { default as x } from "./x.cjs"` of an `export =`
                // module has no synthetic default there.
                implied_node_format: Some(AuthoritativeResolutionMode::EsNext),
                implied_node_format_for_emit: Some(AuthoritativeResolutionMode::EsNext),
                project_reference: None,
            })
            .collect();
        let names = files
            .iter()
            .map(|file| (file.file_name.clone(), file.path.clone()))
            .collect::<Vec<_>>();
        let files_outside_program = files.len() - program_file_count;
        let inputs = LiveCheckerInputs {
            libs: Vec::new(),
            files: files
                .into_iter()
                .map(|file| InputFile::new(file.file_name, file.text))
                .collect(),
            lib_metadata: Vec::new(),
            file_metadata,
            // tsgo `aliasResolver.Options`.
            options: CompilerOptions {
                no_check: Some(true),
                ..CompilerOptions::default()
            },
            current_directory: host
                .current_directory_js()
                .unwrap_or_else(|_| JsString::from("/")),
            files_outside_program,
        };
        let checker =
            LiveChecker::new(inputs, Box::new(provider), WorkerBudget::default(), None).ok()?;
        Some(Self {
            checker,
            files: names,
            program_file_count,
            failed,
        })
    }

    /// Run `query` over the checker.
    pub(super) fn with_checker<T>(
        &mut self,
        query: impl FnOnce(&mut CheckerState<'_>) -> T,
    ) -> Option<T> {
        self.checker.with_diagnostics_checker(query)
    }

    /// The failed lookups recorded so far.
    pub(super) fn failed_lookups(&self) -> std::sync::MutexGuard<'_, FailedLookups> {
        self.failed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether the checker reads a resolved module's file: a TypeScript or
/// declaration file (tsgo's resolver for the empty options finds no other).
/// A file tsgo's alias resolver parses when the checker resolves to it: a
/// TypeScript or JavaScript one.
fn is_source(extension: &ModuleExtension) -> bool {
    matches!(
        extension,
        ModuleExtension::Ts
            | ModuleExtension::Tsx
            | ModuleExtension::Dts
            | ModuleExtension::Mts
            | ModuleExtension::Dmts
            | ModuleExtension::Cts
            | ModuleExtension::Dcts
            | ModuleExtension::Js
            | ModuleExtension::Jsx
            | ModuleExtension::Mjs
            | ModuleExtension::Cjs
    )
}

fn resolved(module: &HostResolvedModule, target: usize) -> AuthoritativeModuleResolution {
    AuthoritativeModuleResolution::Resolved(AuthoritativeResolvedModule {
        target_token: AuthoritativeSourceToken(target as u32),
        resolved_file_name: module.resolved_file().display().to_owned(),
        resolved_using_ts_extension: module.resolved_using_ts_extension(),
        is_custom_resolution: false,
        is_tsx: matches!(module.extension(), ModuleExtension::Tsx),
        is_arbitrary_extension: false,
        is_external_library_import: module.is_external_library_import(),
        package_id: module
            .package_id()
            .map(|package_id| AuthoritativePackageId {
                name: package_id.name().to_owned(),
                submodule_name: package_id.submodule_name().to_owned(),
                version: package_id.version().to_owned(),
                peer_dependencies: package_id.peer_dependencies().map(JsStr::to_owned),
            }),
        alternate_result: module
            .alternate_result()
            .map(|path| path.display().to_owned()),
        types_package_exists: false,
        package_bundles_types: false,
    })
}

fn untyped(module: &HostResolvedModule) -> AuthoritativeModuleResolution {
    AuthoritativeModuleResolution::Untyped(AuthoritativeUntypedModule {
        resolved_file_name: module.resolved_file().display().to_owned(),
        package_name: module
            .package_id()
            .map(|package_id| package_id.name().to_owned()),
        alternate_result: module
            .alternate_result()
            .map(|path| path.display().to_owned()),
        types_package_exists: false,
        package_bundles_types: false,
    })
}

/// tsgo `aliasResolver.GetResolvedModule`: the resolutions computed for the
/// checker's files, a failed non-relative lookup recorded.
struct AliasModuleProvider {
    /// By file token and specifier.
    resolutions: HashMap<(u32, String), AuthoritativeModuleResolution>,
    /// The files by token: name and path.
    files: Vec<(String, String)>,
    failed: Option<Arc<Mutex<FailedLookups>>>,
}

impl AuthoritativeModuleProvider for AliasModuleProvider {
    fn resolve_module(
        &self,
        request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
        let specifier = request.specifier.to_string_lossy().into_owned();
        let resolution = self
            .resolutions
            .get(&(request.source_token.0, specifier.clone()))
            .cloned()
            .unwrap_or_else(|| {
                AuthoritativeModuleResolution::NotFound(AuthoritativeNotFoundModule::default())
            });
        if let (AuthoritativeModuleResolution::NotFound(_), Some(failed)) =
            (&resolution, &self.failed)
        {
            if !tsc_program::path_is_relative(JsStr::from_str(&specifier)) {
                if let Some((file_name, path)) = self.files.get(request.source_token.0 as usize) {
                    let mut failed = failed.lock().unwrap_or_else(PoisonError::into_inner);
                    failed.targets.insert(specifier);
                    failed
                        .sources
                        .entry(path.clone())
                        .or_insert_with(|| file_name.clone());
                }
            }
        }
        Ok(resolution)
    }

    /// tsgo `aliasResolver.GetJSXRuntimeImportSpecifier`: none.
    fn synthesizes_jsx_runtime_import(&self) -> bool {
        false
    }
}

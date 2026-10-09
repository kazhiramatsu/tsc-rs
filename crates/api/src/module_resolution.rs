//! tsgo `api/module_resolution.go` (19dadef8): the module resolvers a client
//! creates (`createModuleResolver`): static resolutions (tsgo
//! `module.StaticResolutions`) and the client's callback in front of the
//! default resolver, for `resolveModuleName` and for the programs a snapshot
//! creates with them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use serde::{Deserialize, Serialize};
use tsc_diagnostics::JsStr;
use tsc_program::{
    HostResolvedModule, ModuleResolutionOverride, OverriddenResolution, PackageId, ProgramOptions,
    ProgramPath, ResolutionMode,
};
use tsc_types::CompilerOptions;

use crate::ipc::Conn;
use crate::proto::{DocumentIdentifier, SnapshotId};

/// tsgo `ModuleResolutionSpec`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ModuleResolutionSpec {
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub entries: Option<Vec<Option<ModuleResolutionEntry>>>,
}

/// tsgo `ModuleResolutionEntry`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleResolutionEntry {
    #[serde(default)]
    pub module_name: Option<String>,
    #[serde(default)]
    pub containing_directory: Option<DocumentIdentifier>,
    #[serde(default)]
    pub resolution_mode: Option<i64>,
    #[serde(default)]
    pub result: Option<StaticModuleResolution>,
}

/// tsgo `StaticModuleResolution`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticModuleResolution {
    #[serde(default)]
    pub resolved_file_name: Option<DocumentIdentifier>,
    #[serde(default)]
    pub original_path: Option<DocumentIdentifier>,
    #[serde(default, rename = "packageId")]
    pub package_id: Option<PackageIdParam>,
}

/// tsgo `PackageId` as a client sends it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageIdParam {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sub_module_name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub peer_dependencies: String,
}

/// tsgo `ResolveModuleNameCallbackParams`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveModuleNameCallbackParams {
    pub module_name: String,
    pub containing_directory: String,
    pub resolution_mode: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_progress_snapshot: Option<u64>,
}

/// tsgo `core.ModuleKind.String()`.
pub fn module_kind_name(kind: i64) -> String {
    match kind {
        0 => "None",
        1 => "CommonJS",
        2 => "AMD",
        3 => "UMD",
        4 => "System",
        5 => "ES2015",
        6 => "ES2020",
        7 => "ES2022",
        99 => "ESNext",
        100 => "Node16",
        101 => "Node18",
        102 => "Node20",
        199 => "NodeNext",
        200 => "Preserve",
        _ => return format!("ModuleKind({kind})"),
    }
    .to_owned()
}

/// tsgo's `core.ResolutionMode` number as a resolver takes it: none,
/// CommonJS or ESNext.
pub fn resolution_mode(mode: i64) -> Option<ResolutionMode> {
    match mode {
        0 => Some(ResolutionMode::Unspecified),
        1 => Some(ResolutionMode::CommonJs),
        99 => Some(ResolutionMode::EsNext),
        _ => None,
    }
}

fn mode_number(mode: ResolutionMode) -> u32 {
    match mode {
        ResolutionMode::Unspecified => 0,
        ResolutionMode::CommonJs => 1,
        ResolutionMode::EsNext => 99,
    }
}

/// tsgo `tspath.ToPath` against the session's current directory.
#[derive(Clone, Debug)]
pub struct Paths {
    pub current_directory: String,
    pub case_sensitive: bool,
}

impl Paths {
    pub fn absolute(&self, file_name: &str) -> String {
        tsc_program::get_normalized_absolute_path(
            JsStr::from_str(file_name),
            JsStr::from_str(&self.current_directory),
        )
        .to_string_lossy()
        .into_owned()
    }

    pub fn to_path(&self, file_name: &str) -> String {
        let absolute = self.absolute(file_name);
        if self.case_sensitive {
            absolute
        } else {
            tsc_host::to_file_name_lower_case(&absolute)
        }
    }

    fn program_path(&self, file_name: &str) -> Option<ProgramPath> {
        let display = self.absolute(file_name);
        let canonical = self.to_path(&display);
        ProgramPath::from_js_parts(JsStr::from_str(&display), JsStr::from_str(&canonical)).ok()
    }
}

/// tsgo `staticModuleResolutionToResolvedModule`: none without a resolved
/// file name.
pub fn static_resolved_module(
    resolution: &StaticModuleResolution,
    paths: &Paths,
) -> Option<HostResolvedModule> {
    let resolved = resolution.resolved_file_name.as_ref()?;
    let file_name = |document: &DocumentIdentifier| {
        document
            .to_absolute_file_name(&paths.current_directory)
            .ok()
            .and_then(|name| paths.program_path(&name))
    };
    let package_id = resolution.package_id.as_ref().and_then(|package| {
        (!package.name.is_empty()).then(|| {
            PackageId::new(
                package.name.as_str(),
                package.sub_module_name.as_str(),
                package.version.as_str(),
            )
            .with_peer_dependencies(package.peer_dependencies.as_str())
        })
    });
    Some(HostResolvedModule::from_api(
        file_name(resolved)?,
        resolution.original_path.as_ref().and_then(file_name),
        package_id,
    ))
}

/// tsgo `module.StaticResolutions`.
#[derive(Debug)]
pub struct StaticResolutions {
    fallback_to_resolver: bool,
    entries: HashMap<(String, Option<String>, Option<u32>), Option<HostResolvedModule>>,
    paths: Paths,
}

impl StaticResolutions {
    /// tsgo `compileModuleResolutionSpec` and `NewStaticResolutions`, with
    /// their client errors.
    pub fn compile(spec: &ModuleResolutionSpec, paths: &Paths) -> Result<Self, String> {
        let fallback_to_resolver = match spec.fallback.as_deref().unwrap_or_default() {
            "resolve" => true,
            "unresolved" => false,
            other => return Err(format!("invalid module resolution fallback {other:?}")),
        };
        let mut entries = HashMap::new();
        for (index, entry) in spec.entries.iter().flatten().enumerate() {
            let Some(entry) = entry else {
                return Err(format!("module resolution entry {index} is null"));
            };
            let module_name = entry.module_name.clone().unwrap_or_default();
            if module_name.is_empty() {
                return Err(format!(
                    "module resolution entry {index} has an empty moduleName"
                ));
            }
            let Some(result) = &entry.result else {
                return Err(format!("module resolution entry {index} has no result"));
            };
            let directory = entry
                .containing_directory
                .as_ref()
                .map(|directory| {
                    directory
                        .to_absolute_file_name(&paths.current_directory)
                        .map(|name| paths.to_path(&name))
                })
                .transpose()?
                .filter(|directory| !directory.is_empty());
            let mode = match entry.resolution_mode {
                None => None,
                Some(mode @ (0 | 1 | 99)) => Some(mode as u32),
                Some(mode) => {
                    return Err(format!(
                        "module resolution entry {index} has invalid resolutionMode {}",
                        module_kind_name(mode)
                    ))
                }
            };
            let key = (module_name, directory, mode);
            if entries.contains_key(&key) {
                return Err(format!(
                    "duplicate static module resolution for {:?}",
                    key.0
                ));
            }
            entries.insert(key, static_resolved_module(result, paths));
        }
        Ok(Self {
            fallback_to_resolver,
            entries,
            paths: paths.clone(),
        })
    }

    /// tsgo `lookup`: the most specific entry for the name, by directory
    /// and mode, then directory, then mode, then the name alone.
    fn lookup(
        &self,
        module_name: &str,
        containing_directory: &str,
        mode: ResolutionMode,
    ) -> Option<&Option<HostResolvedModule>> {
        let directory = Some(self.paths.to_path(containing_directory));
        let mode = Some(mode_number(mode));
        [
            (directory.clone(), mode),
            (directory, None),
            (None, mode),
            (None, None),
        ]
        .into_iter()
        .find_map(|(directory, mode)| self.entries.get(&(module_name.to_owned(), directory, mode)))
    }
}

/// tsgo `moduleResolverRegistration`: what a client's module resolver is.
#[derive(Debug)]
pub struct ModuleResolverRegistration {
    pub compiler_options: CompilerOptions,
    pub program_options: ProgramOptions,
    pub resolutions: Option<StaticResolutions>,
    pub callback: String,
}

/// What the resolutions a session's module resolvers make share: the
/// connection for the client's callbacks, the snapshot a request is
/// building, and the first error of the request's callbacks.
#[derive(Default)]
pub struct ResolutionState {
    conn: OnceLock<Weak<Conn>>,
    in_flight: Mutex<Option<InFlight>>,
    request_error: Mutex<Option<String>>,
    /// tsgo `nextProgramResolutionContextID`.
    next_context: std::sync::atomic::AtomicU64,
}

/// The snapshot a request is building (tsgo `programResolutionContext`):
/// its number for the client (0 until a callback needs one), and the file
/// system it reads.
#[derive(Clone)]
pub struct InFlight {
    pub context: u64,
    pub fs: Arc<dyn tsc_host::vfs::FileSystem>,
}

impl ResolutionState {
    pub fn set_connection(&self, conn: &Arc<Conn>) {
        let _ = self.conn.set(Arc::downgrade(conn));
    }

    pub fn connection(&self) -> Option<Arc<Conn>> {
        self.conn.get().and_then(Weak::upgrade)
    }

    /// The snapshot being built, while a request builds one.
    pub fn in_flight(&self) -> Option<InFlight> {
        self.lock_in_flight().clone()
    }

    /// The number the client knows the snapshot being built by: one per
    /// request whose resolutions call the client.
    fn in_flight_context(&self) -> Option<u64> {
        let mut in_flight = self.lock_in_flight();
        let in_flight = in_flight.as_mut()?;
        if in_flight.context == 0 {
            in_flight.context = self
                .next_context
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
        }
        Some(in_flight.context)
    }

    /// Start building a snapshot: its context, and no error yet.
    pub fn begin(&self, in_flight: InFlight) {
        *self.lock_in_flight() = Some(in_flight);
        *self.lock_error() = None;
    }

    /// The building ends: the first error of its callbacks.
    pub fn end(&self) -> Option<String> {
        *self.lock_in_flight() = None;
        self.lock_error().take()
    }

    fn record_error(&self, error: String) {
        self.lock_error().get_or_insert(error);
    }

    fn lock_in_flight(&self) -> std::sync::MutexGuard<'_, Option<InFlight>> {
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_error(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.request_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// tsgo `callbackModuleResolver.resolveModuleName`: the client's answer,
    /// none for `null`.
    pub fn call_resolver(
        &self,
        callback: &str,
        params: &ResolveModuleNameCallbackParams,
        paths: &Paths,
    ) -> Result<Option<HostResolvedModule>, String> {
        let conn = self
            .connection()
            .ok_or_else(|| "API connection is not initialized".to_owned())?;
        let result = conn
            .call(
                callback,
                &serde_json::to_string(params).expect("the parameters serialize"),
            )
            .map_err(|error| format!("resolveModuleName callback failed: {error}"))?;
        if result.is_empty() || result == b"null" {
            return Ok(None);
        }
        let resolution: StaticModuleResolution = serde_json::from_slice(&result)
            .map_err(|error| format!("invalid resolveModuleName callback result: {error}"))?;
        Ok(static_resolved_module(&resolution, paths))
    }
}

/// tsgo's resolver chain of a client's module resolver: its static
/// resolutions, then its callback or (without one) the default resolver.
pub enum Chain<'a> {
    /// A static entry or the callback's answer.
    Answered(Option<Box<HostResolvedModule>>),
    /// The default resolver's, with the callback's context when there is one.
    Default(&'a ModuleResolverRegistration),
}

impl ModuleResolverRegistration {
    /// The static lookup: an answer, or what comes next.
    pub fn chain(
        &self,
        module_name: &str,
        containing_directory: &str,
        mode: ResolutionMode,
    ) -> Chain<'_> {
        if let Some(resolutions) = &self.resolutions {
            if let Some(result) = resolutions.lookup(module_name, containing_directory, mode) {
                return Chain::Answered(result.clone().map(Box::new));
            }
            if !resolutions.fallback_to_resolver {
                return Chain::Answered(None);
            }
        }
        Chain::Default(self)
    }
}

/// tsgo `moduleResolverFactory.NewResolver`: a program's resolution through
/// a client's module resolver, in front of the program's resolver.
pub struct ProgramModuleResolution {
    pub registration: Arc<ModuleResolverRegistration>,
    pub state: Arc<ResolutionState>,
    pub paths: Paths,
}

impl ModuleResolutionOverride for ProgramModuleResolution {
    fn resolver_options(&self) -> Option<(&CompilerOptions, &ProgramOptions)> {
        Some((
            &self.registration.compiler_options,
            &self.registration.program_options,
        ))
    }

    fn resolve(
        &self,
        specifier: JsStr<'_>,
        containing_directory: JsStr<'_>,
        mode: ResolutionMode,
    ) -> OverriddenResolution {
        let specifier = specifier.to_string_lossy();
        let directory = containing_directory.to_string_lossy();
        match self.registration.chain(&specifier, &directory, mode) {
            Chain::Answered(Some(module)) => OverriddenResolution::Resolved(module),
            Chain::Answered(None) => OverriddenResolution::Unresolved,
            Chain::Default(registration) if registration.callback.is_empty() => {
                OverriddenResolution::Program
            }
            Chain::Default(registration) => {
                let params = ResolveModuleNameCallbackParams {
                    module_name: specifier.into_owned(),
                    containing_directory: directory.into_owned(),
                    resolution_mode: mode_number(mode),
                    snapshot: None,
                    in_progress_snapshot: self.state.in_flight_context(),
                };
                match self
                    .state
                    .call_resolver(&registration.callback, &params, &self.paths)
                {
                    Ok(Some(module)) => OverriddenResolution::Resolved(Box::new(module)),
                    Ok(None) => OverriddenResolution::Unresolved,
                    Err(error) => {
                        // tsgo records the program's ModuleResolutionError,
                        // which the request answers.
                        self.state.record_error(error);
                        OverriddenResolution::Unresolved
                    }
                }
            }
        }
    }
}

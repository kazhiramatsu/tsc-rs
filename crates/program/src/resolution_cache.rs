//! L2.3 isolated prototype: a project-owned, generation-versioned resolution
//! cache placed *above* [`ModuleResolver`].
//!
//! The cache never reimplements resolution. Every miss runs the ordinary
//! resolver against an [`ObservationHost`], which memoizes host answers for
//! the duration of one candidate generation (the analogue of the Go
//! `SnapshotFS`/`cachedvfs` layer) and records, per request, the exact set of
//! host facts the resolver consulted (the analogue of Go's `sourceFS` seen
//! files / missing directories and of tsc's `failedLookupLocations` /
//! `affectingLocations`). A cached value is reused only while every recorded
//! fact is untouched by the explicit [`ChangeBatch`] that opens the next
//! generation.
//!
//! Ownership follows the L2 design: generations are immutable once published,
//! an old reader keeps its own [`GenerationHandle`], a candidate that fails or
//! is cancelled never mutates the published view, and retention is bounded by
//! entries, bytes and live generation handles. Values are host-level facts
//! ([`HostModuleResolution`] and friends) without any Program
//! [`SourceFileId`](crate::SourceFileId); rebinding to a generation's source
//! membership stays with the Program owner.
//!
//! This module is single-owner (`Rc`, `RefCell`) on purpose; the handoff
//! forbids a speculative `Send + Sync` conversion before the ownership model
//! is proven.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use serde_json::{json, Value};
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{to_file_name_lower_case_js, CompilerHost, HostError};
use tsc_types::CompilerOptions;

use crate::module_resolution::{
    normalize_absolute_js_path, HostModuleResolution, HostResolvedModule,
    HostResolvedTypeReferenceDirective, ModuleResolver,
};
use crate::path::ProgramPath;
use crate::prepared::{PackageJsonType, ProgramOptions};
use crate::resolution::{ResolutionError, ResolutionMode, ResolutionOutcome};

// ---------------------------------------------------------------------------
// Canonical path keys and digests
// ---------------------------------------------------------------------------

/// A host-profile canonical path used for dependency matching. Case folding
/// applies TypeScript's `toFileNameLowerCase` exactly as the resolver's
/// `canonical_text` does; lexical normalization is the caller's job.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PathKey(JsString);

impl PathKey {
    pub fn new(path: JsStr<'_>, case_sensitive: bool) -> Self {
        if case_sensitive {
            Self(path.to_owned())
        } else {
            Self(to_file_name_lower_case_js(path))
        }
    }

    pub fn as_js(&self) -> JsStr<'_> {
        self.0.as_js()
    }

    fn parent(&self) -> Option<PathKey> {
        let text = self.0.as_js();
        let (head, _) = text.rsplit_once("/")?;
        if head.is_empty() {
            if text.len_units() > 1 {
                return Some(PathKey(JsString::from("/")));
            }
            return None;
        }
        Some(PathKey(head.to_owned()))
    }

    fn is_within(&self, directory: &PathKey) -> bool {
        let path = self.0.as_js();
        let directory = directory.0.as_js();
        if directory.as_bytes() == b"/" {
            return path.as_bytes().len() > 1 && path.starts_with("/");
        }
        path.starts_with_js(directory)
            && path
                .as_bytes()
                .get(directory.as_bytes().len())
                .is_some_and(|byte| *byte == b'/')
    }
}

impl fmt::Display for PathKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.to_string_lossy())
    }
}

/// FNV-1a over raw bytes: a deterministic content digest that does not depend
/// on Rust's hasher randomization.
pub fn content_digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn digest_js_list(entries: &[JsString]) -> u64 {
    let mut bytes = Vec::new();
    for entry in entries {
        bytes.extend_from_slice(entry.as_bytes());
        bytes.push(0);
    }
    content_digest(&bytes)
}

// ---------------------------------------------------------------------------
// Dependencies
// ---------------------------------------------------------------------------

/// One host fact consumed by a cached computation.
///
/// Negative facts are first-class: a `NotFound` resolution depends on the
/// candidates that did not exist, and the directories that were probed and
/// were absent. Errors are never dependencies because errored requests are
/// never cached.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Dependency {
    FileExists {
        path: PathKey,
        exists: bool,
    },
    DirectoryExists {
        path: PathKey,
        exists: bool,
    },
    /// `None` = the read returned no bytes (absent) at read time.
    FileContent {
        path: PathKey,
        digest: Option<u64>,
    },
    Realpath {
        path: PathKey,
        target: Option<PathKey>,
    },
    /// A `read_directory`/`get_directories` listing digest.
    DirectoryEntries {
        path: PathKey,
        digest: u64,
    },
}

impl Dependency {
    pub fn path(&self) -> &PathKey {
        match self {
            Self::FileExists { path, .. }
            | Self::DirectoryExists { path, .. }
            | Self::FileContent { path, .. }
            | Self::Realpath { path, .. }
            | Self::DirectoryEntries { path, .. } => path,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::FileExists { .. } => "file_exists",
            Self::DirectoryExists { .. } => "directory_exists",
            Self::FileContent { .. } => "file_content",
            Self::Realpath { .. } => "realpath",
            Self::DirectoryEntries { .. } => "directory_entries",
        }
    }

    fn estimated_bytes(&self) -> usize {
        24 + self.path().0.as_bytes().len()
            + match self {
                Self::Realpath {
                    target: Some(target),
                    ..
                } => target.0.as_bytes().len(),
                _ => 0,
            }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Self::FileExists { path, exists } => {
                json!({"kind": "file_exists", "path": path.to_string(), "exists": exists})
            }
            Self::DirectoryExists { path, exists } => {
                json!({"kind": "directory_exists", "path": path.to_string(), "exists": exists})
            }
            Self::FileContent { path, digest } => json!({
                "kind": "file_content",
                "path": path.to_string(),
                "digest": digest.map(|digest| format!("{digest:016x}")),
            }),
            Self::Realpath { path, target } => json!({
                "kind": "realpath",
                "path": path.to_string(),
                "target": target.as_ref().map(ToString::to_string),
            }),
            Self::DirectoryEntries { path, digest } => json!({
                "kind": "directory_entries",
                "path": path.to_string(),
                "digest": format!("{digest:016x}"),
            }),
        }
    }
}

/// The complete, deduplicated dependency set of one cached value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DependencySet {
    entries: BTreeSet<Dependency>,
}

impl DependencySet {
    pub fn iter(&self) -> impl Iterator<Item = &Dependency> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every distinct path mentioned by any dependency, in key order.
    pub fn paths(&self) -> BTreeSet<PathKey> {
        self.entries
            .iter()
            .map(|dependency| dependency.path().clone())
            .collect()
    }

    fn estimated_bytes(&self) -> usize {
        self.entries.iter().map(Dependency::estimated_bytes).sum()
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.entries.iter().map(Dependency::to_json).collect())
    }
}

// ---------------------------------------------------------------------------
// Observation host: per-generation memo + per-request dependency recorder
// ---------------------------------------------------------------------------

#[derive(Default)]
struct HostMemo {
    file_exists: BTreeMap<PathKey, bool>,
    directory_exists: BTreeMap<PathKey, bool>,
    file_content: BTreeMap<PathKey, Option<Rc<Vec<u8>>>>,
    realpath: BTreeMap<PathKey, Option<JsString>>,
    read_directory: BTreeMap<PathKey, Vec<JsString>>,
    get_directories: BTreeMap<PathKey, Vec<JsString>>,
}

/// Counters for one generation's host traffic. `memo_hits` are answers served
/// without touching the underlying host.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostTrafficStats {
    pub host_calls: usize,
    pub memo_hits: usize,
    pub host_errors: usize,
}

/// A read-only [`CompilerHost`] view for one candidate generation.
///
/// Answers are memoized for the generation (a filesystem snapshot for the
/// build), and every answer consumed while a request recorder is open is
/// appended to that request's [`DependencySet`]. Host errors are propagated
/// unchanged and are never memoized, so a later successful answer in the same
/// generation is still observed.
pub struct ObservationHost<'h> {
    inner: &'h dyn CompilerHost,
    case_sensitive: bool,
    memo: RefCell<HostMemo>,
    recorder: RefCell<Option<DependencySet>>,
    stats: Cell<HostTrafficStats>,
}

impl<'h> ObservationHost<'h> {
    pub fn new(inner: &'h dyn CompilerHost) -> Self {
        Self {
            inner,
            case_sensitive: inner.use_case_sensitive_file_names(),
            memo: RefCell::new(HostMemo::default()),
            recorder: RefCell::new(None),
            stats: Cell::new(HostTrafficStats::default()),
        }
    }

    pub fn stats(&self) -> HostTrafficStats {
        self.stats.get()
    }

    fn key(&self, path: JsStr<'_>) -> PathKey {
        PathKey::new(path, self.case_sensitive)
    }

    /// Run `action` while recording every host fact it consumes. The
    /// recorded set is the dependency set of whatever `action` computed.
    pub fn record<T>(&self, action: impl FnOnce(&Self) -> T) -> (T, DependencySet) {
        self.begin_recording();
        let result = action(self);
        (result, self.finish_recording())
    }

    fn begin_recording(&self) {
        *self.recorder.borrow_mut() = Some(DependencySet::default());
    }

    fn finish_recording(&self) -> DependencySet {
        self.recorder.borrow_mut().take().unwrap_or_default()
    }

    fn note(&self, dependency: Dependency) {
        if let Some(recorder) = self.recorder.borrow_mut().as_mut() {
            recorder.entries.insert(dependency);
        }
    }

    fn count_call(&self, memo_hit: bool, error: bool) {
        let mut stats = self.stats.get();
        if memo_hit {
            stats.memo_hits += 1;
        } else {
            stats.host_calls += 1;
        }
        if error {
            stats.host_errors += 1;
        }
        self.stats.set(stats);
    }
}

impl CompilerHost for ObservationHost<'_> {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.inner.current_directory_js()
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        let key = self.key(path);
        let cached = self.memo.borrow().file_content.get(&key).cloned();
        let bytes = match cached {
            Some(bytes) => {
                self.count_call(true, false);
                bytes
            }
            None => match self.inner.read_file_js(path) {
                Ok(bytes) => {
                    self.count_call(false, false);
                    let bytes = bytes.map(Rc::new);
                    self.memo
                        .borrow_mut()
                        .file_content
                        .insert(key.clone(), bytes.clone());
                    bytes
                }
                Err(error) => {
                    self.count_call(false, true);
                    return Err(error);
                }
            },
        };
        self.note(Dependency::FileContent {
            path: key,
            digest: bytes.as_deref().map(|bytes| content_digest(bytes)),
        });
        Ok(bytes.map(|bytes| bytes.as_ref().clone()))
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let key = self.key(path);
        let cached = self.memo.borrow().file_exists.get(&key).copied();
        let exists = match cached {
            Some(exists) => {
                self.count_call(true, false);
                exists
            }
            None => match self.inner.file_exists_js(path) {
                Ok(exists) => {
                    self.count_call(false, false);
                    self.memo
                        .borrow_mut()
                        .file_exists
                        .insert(key.clone(), exists);
                    exists
                }
                Err(error) => {
                    self.count_call(false, true);
                    return Err(error);
                }
            },
        };
        self.note(Dependency::FileExists { path: key, exists });
        Ok(exists)
    }

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let key = self.key(path);
        let cached = self.memo.borrow().directory_exists.get(&key).copied();
        let exists = match cached {
            Some(exists) => {
                self.count_call(true, false);
                exists
            }
            None => match self.inner.directory_exists_js(path) {
                Ok(exists) => {
                    self.count_call(false, false);
                    self.memo
                        .borrow_mut()
                        .directory_exists
                        .insert(key.clone(), exists);
                    exists
                }
                Err(error) => {
                    self.count_call(false, true);
                    return Err(error);
                }
            },
        };
        self.note(Dependency::DirectoryExists { path: key, exists });
        Ok(exists)
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        let key = self.key(path);
        let cached = self.memo.borrow().read_directory.get(&key).cloned();
        let entries = match cached {
            Some(entries) => {
                self.count_call(true, false);
                entries
            }
            None => match self.inner.read_directory_js(path) {
                Ok(entries) => {
                    self.count_call(false, false);
                    self.memo
                        .borrow_mut()
                        .read_directory
                        .insert(key.clone(), entries.clone());
                    entries
                }
                Err(error) => {
                    self.count_call(false, true);
                    return Err(error);
                }
            },
        };
        self.note(Dependency::DirectoryEntries {
            path: key,
            digest: digest_js_list(&entries),
        });
        Ok(entries)
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        let key = self.key(path);
        let cached = self.memo.borrow().get_directories.get(&key).cloned();
        let entries = match cached {
            Some(entries) => {
                self.count_call(true, false);
                entries
            }
            None => match self.inner.get_directories_js(path) {
                Ok(entries) => {
                    self.count_call(false, false);
                    self.memo
                        .borrow_mut()
                        .get_directories
                        .insert(key.clone(), entries.clone());
                    entries
                }
                Err(error) => {
                    self.count_call(false, true);
                    return Err(error);
                }
            },
        };
        // A directories-only listing changes whenever the immediate entry
        // set changes; the same dependency kind covers both projections.
        self.note(Dependency::DirectoryEntries {
            path: key,
            digest: digest_js_list(&entries),
        });
        Ok(entries)
    }

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        let key = self.key(path);
        let cached = self.memo.borrow().realpath.get(&key).cloned();
        let target = match cached {
            Some(target) => {
                self.count_call(true, false);
                target
            }
            None => match self.inner.realpath_js(path) {
                Ok(target) => {
                    self.count_call(false, false);
                    self.memo
                        .borrow_mut()
                        .realpath
                        .insert(key.clone(), target.clone());
                    target
                }
                Err(error) => {
                    self.count_call(false, true);
                    return Err(error);
                }
            },
        };
        self.note(Dependency::Realpath {
            path: key,
            target: target.as_ref().map(|target| self.key(target.as_js())),
        });
        Ok(target)
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.read_file_js(native_js(path)?.as_js())
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.file_exists_js(native_js(path)?.as_js())
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.directory_exists_js(native_js(path)?.as_js())
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        Ok(self
            .read_directory_js(native_js(path)?.as_js())?
            .into_iter()
            .map(|entry| PathBuf::from(entry.to_string_lossy().into_owned()))
            .collect())
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        Ok(self
            .get_directories_js(native_js(path)?.as_js())?
            .into_iter()
            .map(|entry| PathBuf::from(entry.to_string_lossy().into_owned()))
            .collect())
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        Ok(self
            .realpath_js(native_js(path)?.as_js())?
            .map(|target| PathBuf::from(target.to_string_lossy().into_owned())))
    }
}

fn native_js(path: &Path) -> Result<JsString, HostError> {
    path.to_str().map(JsString::from).ok_or_else(|| {
        HostError::new(
            tsc_host::HostErrorKind::InvalidInput,
            tsc_host::HostOperation::ReadFile,
            Some(path.to_path_buf()),
            "observation host requires Unicode native paths",
        )
    })
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// One typed component value of an [`OptionsIdentity`].
///
/// Identity equality is structural: lists keep their length and element
/// boundaries, texts keep their exact UTF-16 code units (a lone surrogate is
/// distinct from U+FFFD), absent and empty are distinct, and no value is ever
/// flattened into a delimiter-joined string.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IdentityValue {
    Undefined,
    Bool(bool),
    Integer(i64),
    /// The canonical IEEE-754 bits of a numeric option.
    NumberBits(u64),
    Text(JsString),
    List(Vec<IdentityValue>),
    /// Ordered `(name, value)` entries (the `paths` map keeps its order).
    Entries(Vec<(JsString, IdentityValue)>),
    /// A host query failed while building the identity; the message keeps
    /// the failure distinct from any successful answer.
    HostError(String),
}

impl IdentityValue {
    fn optional_bool(value: Option<bool>) -> Self {
        value.map_or(Self::Undefined, Self::Bool)
    }

    fn optional_integer(value: Option<i32>) -> Self {
        value.map_or(Self::Undefined, |value| Self::Integer(i64::from(value)))
    }

    fn optional_text(value: Option<&JsString>) -> Self {
        value.map_or(Self::Undefined, |value| Self::Text(value.clone()))
    }

    fn optional_text_list(value: Option<&[JsString]>) -> Self {
        value.map_or(Self::Undefined, |list| {
            Self::List(list.iter().map(|entry| Self::Text(entry.clone())).collect())
        })
    }

    fn estimated_bytes(&self) -> usize {
        8 + match self {
            Self::Undefined | Self::Bool(_) | Self::Integer(_) | Self::NumberBits(_) => 0,
            Self::Text(text) => text.as_bytes().len(),
            Self::List(values) => values.iter().map(Self::estimated_bytes).sum(),
            Self::Entries(entries) => entries
                .iter()
                .map(|(name, value)| name.as_bytes().len() + value.estimated_bytes())
                .sum(),
            Self::HostError(message) => message.len(),
        }
    }

    /// Length-prefixed canonical encoding for the trace digest. Every value
    /// starts with a tag byte and every variable-length part carries its
    /// byte length, so distinct values never share an encoding.
    fn encode(&self, out: &mut Vec<u8>) {
        fn push_len(out: &mut Vec<u8>, length: usize) {
            out.extend_from_slice(&(length as u64).to_le_bytes());
        }
        match self {
            Self::Undefined => out.push(0),
            Self::Bool(value) => {
                out.push(1);
                out.push(u8::from(*value));
            }
            Self::Integer(value) => {
                out.push(2);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Self::NumberBits(bits) => {
                out.push(3);
                out.extend_from_slice(&bits.to_le_bytes());
            }
            Self::Text(text) => {
                out.push(4);
                push_len(out, text.as_bytes().len());
                out.extend_from_slice(text.as_bytes());
            }
            Self::List(values) => {
                out.push(5);
                push_len(out, values.len());
                for value in values {
                    value.encode(out);
                }
            }
            Self::Entries(entries) => {
                out.push(6);
                push_len(out, entries.len());
                for (name, value) in entries {
                    push_len(out, name.as_bytes().len());
                    out.extend_from_slice(name.as_bytes());
                    value.encode(out);
                }
            }
            Self::HostError(message) => {
                out.push(7);
                push_len(out, message.len());
                out.extend_from_slice(message.as_bytes());
            }
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::NumberBits(bits) => format!("{}", f64::from_bits(*bits)),
            Self::Text(text) => format!("{:?}", text.to_string_lossy()),
            Self::List(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::describe)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Entries(entries) => format!(
                "{{{}}}",
                entries
                    .iter()
                    .map(|(name, value)| format!(
                        "{:?}: {}",
                        name.to_string_lossy(),
                        value.describe()
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::HostError(message) => format!("<host error: {message}>"),
        }
    }
}

/// The semantic identity of the option/host profile a resolution depends on.
///
/// tsc keys redirect caches by `getKeyForCompilerOptions` over every option
/// declaration flagged `affectsModuleResolution` plus `pathsBasePath`
/// (`_tsc.js:40343-40345`). This value renders the same facts from the Rust
/// option contracts as typed components, adds the program-level resolver
/// inputs that tsc passes out of band (`preserveSymlinks`, `types`,
/// `typeRoots`, `rootDirs`, config identity) and the host profile (case
/// sensitivity, current directory). Equality is structural; the readable
/// [`describe`](Self::describe) text and the [`digest`](Self::digest) are
/// trace aids and never take part in equality.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OptionsIdentity {
    components: Vec<(&'static str, IdentityValue)>,
}

impl OptionsIdentity {
    pub fn new(
        options: &CompilerOptions,
        program_options: &ProgramOptions,
        host: &dyn CompilerHost,
    ) -> Self {
        let canonical_list = |paths: Option<&[ProgramPath]>| {
            paths.map_or(IdentityValue::Undefined, |paths| {
                IdentityValue::List(
                    paths
                        .iter()
                        .map(|path| IdentityValue::Text(path.canonical().as_js().to_owned()))
                        .collect(),
                )
            })
        };
        let paths = program_options
            .paths()
            .map_or(IdentityValue::Undefined, |paths| {
                IdentityValue::Entries(
                    paths
                        .iter()
                        .map(|mapping| {
                            (
                                mapping.pattern().to_owned(),
                                IdentityValue::List(
                                    mapping
                                        .substitutions()
                                        .iter()
                                        .map(|entry| IdentityValue::Text(entry.clone()))
                                        .collect(),
                                ),
                            )
                        })
                        .collect(),
                )
            });
        let module_suffixes =
            options
                .module_suffixes
                .as_ref()
                .map_or(IdentityValue::Undefined, |suffixes| {
                    IdentityValue::List(
                        suffixes
                            .iter()
                            .map(|suffix| IdentityValue::Text(suffix.runtime_text().to_owned()))
                            .collect(),
                    )
                });
        let current_directory = match host.current_directory_js() {
            Ok(directory) => IdentityValue::Text(directory),
            Err(error) => IdentityValue::HostError(error.to_string()),
        };
        let components: Vec<(&'static str, IdentityValue)> = vec![
            // moduleResolutionOptionDeclarations @6.0.3 (19 declarations with
            // affectsModuleResolution: true), in declaration order.
            ("allowJs", IdentityValue::Bool(options.allow_js)),
            (
                "forceConsistentCasingInFileNames",
                IdentityValue::optional_bool(options.force_consistent_casing_in_file_names),
            ),
            ("checkJs", IdentityValue::optional_bool(options.check_js)),
            ("jsx", IdentityValue::optional_integer(options.jsx)),
            ("locale", IdentityValue::Undefined),
            (
                "moduleDetection",
                IdentityValue::optional_integer(options.module_detection),
            ),
            (
                "moduleResolution",
                IdentityValue::optional_integer(options.module_resolution),
            ),
            (
                "baseUrl",
                IdentityValue::optional_text(options.base_url.as_ref()),
            ),
            ("paths", paths),
            ("rootDirs", canonical_list(program_options.root_dirs())),
            ("typeRoots", canonical_list(program_options.type_roots())),
            ("moduleSuffixes", module_suffixes),
            (
                "resolvePackageJsonExports",
                IdentityValue::optional_bool(options.resolve_package_json_exports),
            ),
            (
                "resolvePackageJsonImports",
                IdentityValue::optional_bool(options.resolve_package_json_imports),
            ),
            (
                "customConditions",
                IdentityValue::optional_text_list(options.custom_conditions.as_deref()),
            ),
            (
                "resolveJsonModule",
                IdentityValue::optional_bool(options.resolve_json_module),
            ),
            (
                "maxNodeModuleJsDepth",
                options
                    .max_node_module_js_depth
                    .map_or(IdentityValue::Undefined, |value| {
                        IdentityValue::NumberBits(value.value().to_bits())
                    }),
            ),
            (
                "noResolve",
                IdentityValue::optional_bool(options.no_resolve),
            ),
            (
                "jsxImportSource",
                IdentityValue::optional_text(options.jsx_import_source.as_ref()),
            ),
            (
                "pathsBasePath",
                program_options
                    .paths_base_path()
                    .map_or(IdentityValue::Undefined, |base| {
                        IdentityValue::Text(base.to_owned())
                    }),
            ),
            // Rust program-level resolver inputs that tsc supplies through the
            // host or `createProgram` arguments rather than options identity.
            ("module", IdentityValue::optional_integer(options.module)),
            ("target", IdentityValue::optional_integer(options.target)),
            (
                "noDtsResolution",
                IdentityValue::optional_bool(options.no_dts_resolution),
            ),
            (
                "allowArbitraryExtensions",
                IdentityValue::optional_bool(options.allow_arbitrary_extensions),
            ),
            (
                "allowImportingTsExtensions",
                IdentityValue::optional_bool(options.allow_importing_ts_extensions),
            ),
            (
                "libReplacement",
                IdentityValue::optional_bool(options.lib_replacement),
            ),
            (
                "preserveSymlinks",
                IdentityValue::Bool(program_options.preserve_symlinks_effective()),
            ),
            (
                "types",
                IdentityValue::optional_text_list(program_options.types()),
            ),
            (
                "configFilePath",
                program_options
                    .config_file_path()
                    .map_or(IdentityValue::Undefined, |path| {
                        IdentityValue::Text(path.canonical().as_js().to_owned())
                    }),
            ),
            (
                "useCaseSensitiveFileNames",
                IdentityValue::Bool(host.use_case_sensitive_file_names()),
            ),
            ("currentDirectory", current_directory),
        ];
        Self { components }
    }

    pub fn components(&self) -> &[(&'static str, IdentityValue)] {
        &self.components
    }

    pub fn component(&self, name: &str) -> Option<&IdentityValue> {
        self.components
            .iter()
            .find(|(component, _)| *component == name)
            .map(|(_, value)| value)
    }

    /// Readable rendering for traces. Not an identity: never compare it.
    pub fn describe(&self) -> String {
        self.components
            .iter()
            .map(|(name, value)| format!("{name}={}", value.describe()))
            .collect::<Vec<_>>()
            .join("|")
    }

    fn estimated_bytes(&self) -> usize {
        self.components
            .iter()
            .map(|(_, value)| 16 + value.estimated_bytes())
            .sum()
    }

    /// A short stable digest of the length-prefixed canonical encoding, for
    /// traces and trace-file names only.
    pub fn digest(&self) -> u64 {
        let mut bytes = Vec::new();
        for (name, value) in &self.components {
            bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            value.encode(&mut bytes);
        }
        content_digest(&bytes)
    }
}

/// The request kinds this cache distinguishes. Different kinds never share a
/// key, even when the same specifier and directory are involved.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RequestKind {
    Module,
    TypeReference,
    AutomaticTypeReference,
    Library,
    /// The nearest `package.json` scope of one source file and the implied
    /// node format derived from it (tsc `impliedFormatPackageJsons`).
    PackageScope,
}

impl RequestKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::TypeReference => "type_reference",
            Self::AutomaticTypeReference => "automatic_type_reference",
            Self::Library => "library",
            Self::PackageScope => "package_scope",
        }
    }
}

/// One cache request key.
///
/// `containing_directory` is the canonical directory of the containing file
/// (tsc's per-directory cache identity, `_tsc.js:40444`; Go's
/// `moduleResolutionCacheKey.containingDirectory`). Type-reference requests
/// additionally keep the automatic (`__inferred type names__.ts`) origin
/// distinct, following Go's `fromInferredTypesContainingFile` rather than the
/// vendored per-directory sharing, because the resolver's secondary lookup
/// differs for that origin. Library requests carry the logical lib file name
/// and the synthetic resolve-from directory. The options identity is part of
/// every key so a config change can never alias an old entry.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RequestKey {
    kind: RequestKind,
    containing_directory: PathKey,
    specifier: JsString,
    mode: ResolutionMode,
    identity: Rc<OptionsIdentity>,
}

impl RequestKey {
    fn new(
        kind: RequestKind,
        containing_directory: PathKey,
        specifier: JsStr<'_>,
        mode: ResolutionMode,
        identity: Rc<OptionsIdentity>,
    ) -> Self {
        Self {
            kind,
            containing_directory,
            specifier: specifier.to_owned(),
            mode,
            identity,
        }
    }

    pub const fn kind(&self) -> RequestKind {
        self.kind
    }

    pub fn containing_directory(&self) -> &PathKey {
        &self.containing_directory
    }

    pub fn specifier(&self) -> JsStr<'_> {
        self.specifier.as_js()
    }

    pub const fn mode(&self) -> ResolutionMode {
        self.mode
    }

    pub fn identity(&self) -> &OptionsIdentity {
        &self.identity
    }

    fn estimated_bytes(&self) -> usize {
        32 + self.containing_directory.0.as_bytes().len()
            + self.specifier.as_bytes().len()
            + self.identity.estimated_bytes()
    }

    pub fn display(&self) -> String {
        format!(
            "{}:{}:{}:{:?}",
            self.kind.name(),
            self.containing_directory,
            self.specifier.to_string_lossy(),
            self.mode
        )
    }
}

// ---------------------------------------------------------------------------
// Values and entries
// ---------------------------------------------------------------------------

/// A cached host-level value. No variant carries a Program source id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CachedValue {
    Module(HostModuleResolution),
    TypeReference(ResolutionOutcome<HostResolvedTypeReferenceDirective>),
    Library(ResolutionOutcome<HostResolvedModule>),
    PackageScope(PackageScopeFacts),
}

/// The package-boundary facts a source file's implied node format depends on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageScopeFacts {
    pub package_json: Option<ProgramPath>,
    pub module_type: PackageJsonType,
    pub implied_node_format: Option<ResolutionMode>,
}

impl CachedValue {
    pub fn module(&self) -> Option<&HostModuleResolution> {
        match self {
            Self::Module(value) => Some(value),
            _ => None,
        }
    }

    pub fn type_reference(&self) -> Option<&ResolutionOutcome<HostResolvedTypeReferenceDirective>> {
        match self {
            Self::TypeReference(value) => Some(value),
            _ => None,
        }
    }

    pub fn library(&self) -> Option<&ResolutionOutcome<HostResolvedModule>> {
        match self {
            Self::Library(value) => Some(value),
            _ => None,
        }
    }

    pub fn package_scope(&self) -> Option<&PackageScopeFacts> {
        match self {
            Self::PackageScope(value) => Some(value),
            _ => None,
        }
    }

    fn estimated_bytes(&self) -> usize {
        fn module_bytes(module: &HostResolvedModule) -> usize {
            module.resolved_file().display().as_bytes().len()
                + module
                    .original_path()
                    .map_or(0, |path| path.display().as_bytes().len())
                + module
                    .package_metadata()
                    .map_or(0, |metadata| metadata.text().len())
                + 64
        }
        match self {
            Self::Module(resolution) => {
                let outcome = match resolution.outcome() {
                    ResolutionOutcome::Resolved(module) => module_bytes(module),
                    ResolutionOutcome::NotFound => 16,
                };
                outcome
                    + resolution.diagnostics().len() * 96
                    + resolution
                        .alternate_result()
                        .map_or(0, |path| path.display().as_bytes().len())
            }
            Self::TypeReference(outcome) => match outcome {
                ResolutionOutcome::Resolved(directive) => {
                    directive.resolved_file().display().as_bytes().len()
                        + directive
                            .package_metadata()
                            .map_or(0, |metadata| metadata.text().len())
                        + 64
                }
                ResolutionOutcome::NotFound => 16,
            },
            Self::Library(outcome) => match outcome {
                ResolutionOutcome::Resolved(module) => module_bytes(module),
                ResolutionOutcome::NotFound => 16,
            },
            Self::PackageScope(facts) => {
                16 + facts
                    .package_json
                    .as_ref()
                    .map_or(0, |path| path.display().as_bytes().len())
            }
        }
    }

    pub fn summary_json(&self) -> Value {
        fn module_json(module: &HostResolvedModule) -> Value {
            json!({
                "resolved_file": module.resolved_file().display().to_string_lossy(),
                "extension": module.extension().as_js().to_string_lossy(),
                "original_path": module.original_path().map(|path| path.display().to_string_lossy().into_owned()),
                "is_external_library_import": module.is_external_library_import(),
                "resolved_using_ts_extension": module.resolved_using_ts_extension(),
                "package_id": module.package_id().map(|id| format!(
                    "{}@{}{}",
                    id.name().to_string_lossy(),
                    id.version().to_string_lossy(),
                    if id.submodule_name().is_empty() { String::new() } else { format!("/{}", id.submodule_name().to_string_lossy()) }
                )),
            })
        }
        match self {
            Self::Module(resolution) => json!({
                "kind": "module",
                "outcome": match resolution.outcome() {
                    ResolutionOutcome::Resolved(module) => module_json(module),
                    ResolutionOutcome::NotFound => Value::Null,
                },
                "alternate_result": resolution.alternate_result().map(|path| path.display().to_string_lossy().into_owned()),
                "diagnostic_codes": resolution.diagnostics().iter().map(|diagnostic| diagnostic.code()).collect::<Vec<_>>(),
            }),
            Self::TypeReference(outcome) => json!({
                "kind": "type_reference",
                "outcome": match outcome {
                    ResolutionOutcome::Resolved(directive) => json!({
                        "resolved_file": directive.resolved_file().display().to_string_lossy(),
                        "primary": directive.primary(),
                        "original_path": directive.original_path().map(|path| path.display().to_string_lossy().into_owned()),
                        "is_external_library_import": directive.is_external_library_import(),
                        "package_id": directive.package_id().map(|id| format!(
                            "{}@{}{}",
                            id.name().to_string_lossy(),
                            id.version().to_string_lossy(),
                            if id.submodule_name().is_empty() { String::new() } else { format!("/{}", id.submodule_name().to_string_lossy()) }
                        )),
                    }),
                    ResolutionOutcome::NotFound => Value::Null,
                },
            }),
            Self::Library(outcome) => json!({
                "kind": "library",
                "outcome": match outcome {
                    ResolutionOutcome::Resolved(module) => module_json(module),
                    ResolutionOutcome::NotFound => Value::Null,
                },
            }),
            Self::PackageScope(facts) => json!({
                "kind": "package_scope",
                "package_json": facts.package_json.as_ref().map(|path| path.display().to_string_lossy().into_owned()),
                "module_type": format!("{:?}", facts.module_type),
                "implied_node_format": facts.implied_node_format.map(|mode| format!("{mode:?}")),
            }),
        }
    }
}

/// One immutable cache entry. Shared between generation views by `Rc`; the
/// strong count is the entry's owner count. Nothing in an entry changes after
/// construction: usage stamps used for LRU eviction belong to each
/// [`GenerationView`] (and to an unpublished candidate's working view), so a
/// dropped or refused candidate can never alter what a published reader sees.
#[derive(Debug)]
pub struct CacheEntry {
    key: RequestKey,
    value: CachedValue,
    dependencies: DependencySet,
    created_generation: u64,
    estimated_bytes: usize,
}

impl CacheEntry {
    pub fn key(&self) -> &RequestKey {
        &self.key
    }

    pub fn value(&self) -> &CachedValue {
        &self.value
    }

    pub fn dependencies(&self) -> &DependencySet {
        &self.dependencies
    }

    pub const fn created_generation(&self) -> u64 {
        self.created_generation
    }

    pub const fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }
}

// ---------------------------------------------------------------------------
// Change batches and invalidation
// ---------------------------------------------------------------------------

/// The explicit, deterministic description of everything that changed since
/// the previously published generation. There are no watchers or timers: the
/// driver supplies the batch (Go's `FileChangeSummary`; tsc's failed-lookup /
/// affecting-location watcher callbacks collapsed into one value).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangeBatch {
    pub created_files: BTreeSet<PathKey>,
    pub changed_files: BTreeSet<PathKey>,
    pub deleted_files: BTreeSet<PathKey>,
    pub created_directories: BTreeSet<PathKey>,
    pub deleted_directories: BTreeSet<PathKey>,
    /// Paths whose `realpath` answer changed (a retargeted or removed link).
    /// A directory link retarget covers every path below it.
    pub realpath_changed: BTreeSet<PathKey>,
    /// Drop every cached value regardless of dependencies.
    pub invalidate_all: bool,
}

impl ChangeBatch {
    pub fn is_empty(&self) -> bool {
        !self.invalidate_all
            && self.created_files.is_empty()
            && self.changed_files.is_empty()
            && self.deleted_files.is_empty()
            && self.created_directories.is_empty()
            && self.deleted_directories.is_empty()
            && self.realpath_changed.is_empty()
    }

    fn any_created_under(&self, directory: &PathKey) -> bool {
        self.created_files
            .iter()
            .chain(self.created_directories.iter())
            .any(|path| path.is_within(directory))
    }

    fn has_deleted_ancestor(&self, path: &PathKey) -> bool {
        self.deleted_directories
            .iter()
            .any(|directory| path.is_within(directory))
    }

    fn has_created_ancestor(&self, path: &PathKey) -> bool {
        self.created_directories
            .iter()
            .any(|directory| path.is_within(directory))
    }

    fn realpath_affected(&self, path: &PathKey) -> bool {
        self.realpath_changed
            .iter()
            .any(|changed| changed == path || path.is_within(changed))
    }

    fn immediate_children_changed(&self, directory: &PathKey) -> bool {
        self.created_files
            .iter()
            .chain(self.deleted_files.iter())
            .chain(self.created_directories.iter())
            .chain(self.deleted_directories.iter())
            .any(|path| path.parent().as_ref() == Some(directory))
    }

    /// Return the first dependency violated by this batch, if any.
    ///
    /// Matching is deliberately conservative in the same directions as tsc's
    /// `isInvalidatedFailedLookup` (`_tsc.js:128984-128986`): exact path
    /// matches, `startsWith` for a deleted or created directory subtree, and
    /// the implicit parent-directory creation of any created path. The
    /// resulting over-invalidation is measured, not hidden.
    pub fn violated_dependency<'d>(
        &self,
        dependencies: &'d DependencySet,
    ) -> Option<&'d Dependency> {
        if self.invalidate_all {
            return dependencies.iter().next();
        }
        dependencies.iter().find(|dependency| match dependency {
            Dependency::FileExists { path, exists } => {
                self.created_files.contains(path)
                    || self.deleted_files.contains(path)
                    || (*exists && self.has_deleted_ancestor(path))
                    || (!*exists && self.has_created_ancestor(path))
                    || self.realpath_affected(path)
            }
            Dependency::DirectoryExists { path, exists } => {
                self.created_directories.contains(path)
                    || self.deleted_directories.contains(path)
                    || (*exists && self.has_deleted_ancestor(path))
                    || (!*exists
                        && (self.has_created_ancestor(path) || self.any_created_under(path)))
                    || self.realpath_affected(path)
            }
            Dependency::FileContent { path, digest } => {
                self.changed_files.contains(path)
                    || self.deleted_files.contains(path)
                    || self.created_files.contains(path)
                    || self.has_deleted_ancestor(path)
                    || (digest.is_none() && self.has_created_ancestor(path))
                    || self.realpath_affected(path)
            }
            Dependency::Realpath { path, target } => {
                self.realpath_affected(path)
                    || self.deleted_files.contains(path)
                    || self.created_files.contains(path)
                    || self.has_deleted_ancestor(path)
                    || target.as_ref().is_some_and(|target| {
                        self.realpath_affected(target)
                            || self.deleted_files.contains(target)
                            || self.has_deleted_ancestor(target)
                    })
            }
            Dependency::DirectoryEntries { path, .. } => {
                self.created_directories.contains(path)
                    || self.deleted_directories.contains(path)
                    || self.has_deleted_ancestor(path)
                    || self.has_created_ancestor(path)
                    || self.immediate_children_changed(path)
                    || self.realpath_affected(path)
            }
        })
    }

    pub fn to_json(&self) -> Value {
        fn list(set: &BTreeSet<PathKey>) -> Value {
            Value::Array(
                set.iter()
                    .map(|path| Value::String(path.to_string()))
                    .collect(),
            )
        }
        json!({
            "created_files": list(&self.created_files),
            "changed_files": list(&self.changed_files),
            "deleted_files": list(&self.deleted_files),
            "created_directories": list(&self.created_directories),
            "deleted_directories": list(&self.deleted_directories),
            "realpath_changed": list(&self.realpath_changed),
            "invalidate_all": self.invalidate_all,
        })
    }
}

// ---------------------------------------------------------------------------
// Generations
// ---------------------------------------------------------------------------

/// One entry as seen by one view: the shared immutable entry plus the
/// view-local usage stamp (the generation that last requested it).
#[derive(Clone, Debug)]
struct ViewEntry {
    entry: Rc<CacheEntry>,
    last_used: u64,
}

/// One published, immutable generation view.
#[derive(Debug)]
pub struct GenerationView {
    id: u64,
    parent: Option<u64>,
    entries: BTreeMap<RequestKey, ViewEntry>,
}

impl GenerationView {
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn parent(&self) -> Option<u64> {
        self.parent
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Rc<CacheEntry>> {
        self.entries.values().map(|entry| &entry.entry)
    }

    pub fn get(&self, key: &RequestKey) -> Option<&Rc<CacheEntry>> {
        self.entries.get(key).map(|entry| &entry.entry)
    }

    /// The generation that last requested `key` as recorded in this view.
    /// Views are immutable, so this never changes after publication.
    pub fn last_used(&self, key: &RequestKey) -> Option<u64> {
        self.entries.get(key).map(|entry| entry.last_used)
    }

    /// Every `(key, last used generation)` pair in key order.
    pub fn usage_stamps(&self) -> Vec<(RequestKey, u64)> {
        self.entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.last_used))
            .collect()
    }

    pub fn estimated_bytes(&self) -> usize {
        self.entries
            .values()
            .map(|entry| entry.entry.estimated_bytes)
            .sum()
    }
}

/// A reader's hold on one generation. Dropping the handle releases the
/// generation; entries stay alive while any handle references them.
#[derive(Clone, Debug)]
pub struct GenerationHandle {
    view: Rc<GenerationView>,
}

impl GenerationHandle {
    pub fn view(&self) -> &GenerationView {
        &self.view
    }

    pub fn id(&self) -> u64 {
        self.view.id
    }
}

/// Retention limits enforced at publish time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionLimits {
    pub max_entries: usize,
    /// Estimated payload bound, applied separately to published entries and
    /// eviction history. Each key is charged for its full option identity
    /// even when that allocation is shared. This is not allocator/RSS usage.
    pub max_bytes: usize,
    /// Maximum number of live generation handles (published views still
    /// held by a reader, including the cache's own current view).
    pub max_live_generations: usize,
    /// Maximum number of evicted keys remembered for the
    /// `Fresh { evicted_before }` trace disposition. Older history is
    /// forgotten (counted in [`ResidentStats::eviction_history_dropped`]).
    pub max_eviction_history: usize,
}

impl Default for RetentionLimits {
    fn default() -> Self {
        Self {
            max_entries: 4096,
            max_bytes: 8 << 20,
            max_live_generations: 8,
            max_eviction_history: 4096,
        }
    }
}

/// Everything the cache keeps resident besides the entries a reader holds:
/// the published view, the bounded eviction history, the live generation
/// records and the interned option identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResidentStats {
    pub published_entries: usize,
    pub published_bytes: usize,
    pub eviction_history_len: usize,
    pub eviction_history_bytes: usize,
    /// Evicted keys forgotten because the history bound was reached. While
    /// this is zero, `Fresh { evicted_before: false }` means "never evicted";
    /// afterwards it may also mean "evicted before the retained history".
    pub eviction_history_dropped: u64,
    pub live_generation_records: usize,
    /// Latest live interned identity; an alias already charged to its owning
    /// keys (or an active candidate), not additional cache-owned storage.
    pub interned_identity_bytes: usize,
}

impl ResidentStats {
    pub fn to_json(&self) -> Value {
        json!({
            "published_entries": self.published_entries,
            "published_bytes": self.published_bytes,
            "eviction_history_len": self.eviction_history_len,
            "eviction_history_bytes": self.eviction_history_bytes,
            "eviction_history_dropped": self.eviction_history_dropped,
            "live_generation_records": self.live_generation_records,
            "interned_identity_bytes": self.interned_identity_bytes,
        })
    }
}

/// The bounded, immutable record of evicted keys. Replaced as a whole when it
/// changes so unpublished candidates can share it without copying.
#[derive(Debug, Default)]
struct EvictionHistory {
    order: std::collections::VecDeque<RequestKey>,
    keys: BTreeSet<RequestKey>,
    dropped: u64,
}

impl EvictionHistory {
    fn contains(&self, key: &RequestKey) -> bool {
        self.keys.contains(key)
    }

    fn estimated_bytes(&self) -> usize {
        self.order.iter().map(RequestKey::estimated_bytes).sum()
    }

    /// A new history with `victims` appended and the oldest keys forgotten
    /// beyond `bound`.
    fn with_evicted(
        &self,
        victims: impl IntoIterator<Item = RequestKey>,
        bound: usize,
        byte_bound: usize,
    ) -> Self {
        let mut order = self.order.clone();
        let mut keys = self.keys.clone();
        let mut dropped = self.dropped;
        for victim in victims {
            if keys.insert(victim.clone()) {
                order.push_back(victim);
            } else if let Some(position) = order.iter().position(|key| *key == victim) {
                // Re-evicted: move to the most recent position.
                let key = order.remove(position).expect("position is valid");
                order.push_back(key);
            }
        }
        let mut bytes: usize = order.iter().map(RequestKey::estimated_bytes).sum();
        while order.len() > bound || bytes > byte_bound {
            let forgotten = order.pop_front().expect("history is non-empty");
            keys.remove(&forgotten);
            bytes -= forgotten.estimated_bytes();
            dropped += 1;
        }
        Self {
            order,
            keys,
            dropped,
        }
    }
}

/// Typed cache failures. None of them partially updates the published view.
#[derive(Debug)]
pub enum CacheError {
    Resolution(ResolutionError),
    Cancelled {
        after_requests: usize,
    },
    LiveGenerationLimit {
        live: usize,
        limit: usize,
    },
    /// The candidate's parent is not the destination's published view: the
    /// candidate is stale (another generation was published or everything was
    /// evicted since it began).
    ParentMismatch {
        expected: u64,
        actual: u64,
    },
    /// The candidate was begun on a different cache instance.
    ForeignCandidate,
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolution(error) => write!(formatter, "resolution failed: {error}"),
            Self::Cancelled { after_requests } => {
                write!(
                    formatter,
                    "candidate cancelled after {after_requests} requests"
                )
            }
            Self::LiveGenerationLimit { live, limit } => write!(
                formatter,
                "cannot publish: {live} live generations exceed the limit of {limit}"
            ),
            Self::ParentMismatch { expected, actual } => write!(
                formatter,
                "candidate parent generation {actual} is not the published generation {expected}"
            ),
            Self::ForeignCandidate => write!(
                formatter,
                "candidate belongs to a different resolution cache instance"
            ),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<ResolutionError> for CacheError {
    fn from(error: ResolutionError) -> Self {
        Self::Resolution(error)
    }
}

/// Explicit cooperative cancellation. Checked before every request.
#[derive(Debug, Default)]
pub struct CancellationToken {
    cancelled: Cell<bool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.set(true);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

/// What happened to one request inside a candidate generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Disposition {
    /// The parent view held a valid entry; no host traffic.
    Reused,
    /// The parent view held an entry, a dependency matched the batch, and the
    /// value was recomputed. `changed` reports whether the value differs
    /// (false = provable over-invalidation for this batch).
    Recomputed { violated: Dependency, changed: bool },
    /// No entry existed in the parent view (first request, or evicted).
    Fresh { evicted_before: bool },
}

impl Disposition {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Reused => "reused",
            Self::Recomputed { .. } => "recomputed",
            Self::Fresh { .. } => "fresh",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TraceRow {
    pub key: RequestKey,
    pub disposition: Disposition,
    pub dependency_count: usize,
}

impl TraceRow {
    pub fn to_json(&self) -> Value {
        let mut row = json!({
            "key": self.key.display(),
            "disposition": self.disposition.name(),
            "dependency_count": self.dependency_count,
        });
        if let Disposition::Recomputed { violated, changed } = &self.disposition {
            row["violated"] = violated.to_json();
            row["changed"] = Value::Bool(*changed);
        }
        if let Disposition::Fresh { evicted_before } = &self.disposition {
            row["evicted_before"] = Value::Bool(*evicted_before);
        }
        row
    }
}

/// The deterministic invalidation decision computed when a candidate opens.
#[derive(Clone, Debug, Default)]
pub struct InvalidationReport {
    pub invalidated: Vec<(RequestKey, Dependency)>,
    pub retained: usize,
}

impl InvalidationReport {
    pub fn to_json(&self) -> Value {
        json!({
            "invalidated": self.invalidated.iter().map(|(key, dependency)| json!({
                "key": key.display(),
                "violated": dependency.to_json(),
            })).collect::<Vec<_>>(),
            "retained": self.retained,
        })
    }
}

/// Aggregate counters of one published generation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GenerationStats {
    pub reused: usize,
    pub recomputed_changed: usize,
    pub recomputed_unchanged: usize,
    pub fresh: usize,
    pub evicted: usize,
    pub entries: usize,
    pub bytes: usize,
    pub eviction_history_len: usize,
    pub eviction_history_dropped: u64,
    pub host: HostTrafficStats,
}

impl GenerationStats {
    pub fn to_json(&self) -> Value {
        json!({
            "reused": self.reused,
            "recomputed_changed": self.recomputed_changed,
            "recomputed_unchanged": self.recomputed_unchanged,
            "fresh": self.fresh,
            "evicted": self.evicted,
            "entries": self.entries,
            "bytes": self.bytes,
            "eviction_history_len": self.eviction_history_len,
            "eviction_history_dropped": self.eviction_history_dropped,
            "host_calls": self.host.host_calls,
            "memo_hits": self.host.memo_hits,
            "host_errors": self.host.host_errors,
        })
    }
}

/// Marks one cache instance. Candidates carry a clone so a candidate begun
/// on one cache can never be published into another.
#[derive(Debug)]
struct OwnerToken;

/// The project-owned cache. It holds exactly one published view at a time and
/// tracks every live reader handle through weak references.
pub struct ResolutionCache {
    owner: Rc<OwnerToken>,
    published: Rc<GenerationView>,
    live: Vec<Weak<GenerationView>>,
    next_generation: u64,
    limits: RetentionLimits,
    eviction_history: Rc<EvictionHistory>,
    /// Share the most recent identity while a view/history/candidate owns it.
    /// A weak slot must not retain an oversized or abandoned identity.
    interned_identity: Option<Weak<OptionsIdentity>>,
}

impl ResolutionCache {
    pub fn new(limits: RetentionLimits) -> Self {
        let published = Rc::new(GenerationView {
            id: 0,
            parent: None,
            entries: BTreeMap::new(),
        });
        Self {
            owner: Rc::new(OwnerToken),
            live: vec![Rc::downgrade(&published)],
            published,
            next_generation: 1,
            limits,
            eviction_history: Rc::new(EvictionHistory::default()),
            interned_identity: None,
        }
    }

    pub const fn limits(&self) -> RetentionLimits {
        self.limits
    }

    /// Replace the retention limits; they apply at the next publish.
    pub fn set_limits(&mut self, limits: RetentionLimits) {
        self.limits = limits;
    }

    /// The current published view; the returned handle keeps it alive.
    pub fn current(&self) -> GenerationHandle {
        GenerationHandle {
            view: Rc::clone(&self.published),
        }
    }

    pub fn published_id(&self) -> u64 {
        self.published.id
    }

    /// Count live generation handles (the cache's own published view counts
    /// as one). Dead weak references are pruned.
    pub fn live_generation_count(&mut self) -> usize {
        self.live.retain(|weak| weak.strong_count() > 0);
        self.live.len()
    }

    /// The state resident in the cache itself (excluding entries kept alive
    /// only by reader handles).
    pub fn resident_stats(&mut self) -> ResidentStats {
        let live_generation_records = self.live_generation_count();
        ResidentStats {
            published_entries: self.published.len(),
            published_bytes: self.published.estimated_bytes(),
            eviction_history_len: self.eviction_history.order.len(),
            eviction_history_bytes: self.eviction_history.estimated_bytes(),
            eviction_history_dropped: self.eviction_history.dropped,
            live_generation_records,
            interned_identity_bytes: self
                .interned_identity
                .as_ref()
                .and_then(Weak::upgrade)
                .map_or(0, |identity| identity.estimated_bytes()),
        }
    }

    fn intern_identity(&mut self, identity: OptionsIdentity) -> Rc<OptionsIdentity> {
        match self.interned_identity.as_ref().and_then(Weak::upgrade) {
            Some(existing) if *existing == identity => existing,
            _ => {
                let identity = Rc::new(identity);
                self.interned_identity = Some(Rc::downgrade(&identity));
                identity
            }
        }
    }

    /// Open a candidate generation from the published view by applying an
    /// explicit change batch. Invalidation is computed here, once, and is
    /// visible in the returned candidate's report before any request runs.
    pub fn begin<'h>(
        &mut self,
        batch: ChangeBatch,
        host: &'h dyn CompilerHost,
        options: &'h CompilerOptions,
        program_options: &'h ProgramOptions,
    ) -> Candidate<'h> {
        let identity = self.intern_identity(OptionsIdentity::new(options, program_options, host));
        let parent = Rc::clone(&self.published);
        let mut report = InvalidationReport::default();
        let mut working = BTreeMap::new();
        for (key, view_entry) in &parent.entries {
            match batch.violated_dependency(&view_entry.entry.dependencies) {
                Some(violated) => report.invalidated.push((key.clone(), violated.clone())),
                None => {
                    working.insert(key.clone(), view_entry.clone());
                }
            }
        }
        report.retained = working.len();
        Candidate {
            owner: Rc::clone(&self.owner),
            id: self.next_generation,
            parent,
            working,
            batch,
            report,
            identity,
            host: ObservationHost::new(host),
            raw_host: host,
            options,
            program_options,
            case_sensitive: host.use_case_sensitive_file_names(),
            trace: Vec::new(),
            requests_run: 0,
            eviction_history: Rc::clone(&self.eviction_history),
        }
    }

    fn check_live_bound(&mut self) -> Result<(), CacheError> {
        // The new view will become one more live generation.
        let live = self.live_generation_count();
        if live + 1 > self.limits.max_live_generations {
            return Err(CacheError::LiveGenerationLimit {
                live: live + 1,
                limit: self.limits.max_live_generations,
            });
        }
        Ok(())
    }

    /// Publish a candidate atomically. Retention limits are applied to the
    /// new view only; older views keep their entries alive independently.
    /// On any error the published view, its usage stamps and the eviction
    /// history are unchanged.
    pub fn publish(
        &mut self,
        candidate: Candidate<'_>,
    ) -> Result<(GenerationHandle, GenerationStats), CacheError> {
        if !Rc::ptr_eq(&candidate.owner, &self.owner) {
            return Err(CacheError::ForeignCandidate);
        }
        if !Rc::ptr_eq(&candidate.parent, &self.published) {
            return Err(CacheError::ParentMismatch {
                expected: self.published.id,
                actual: candidate.parent.id,
            });
        }
        self.check_live_bound()?;
        let Candidate {
            id,
            parent,
            working,
            trace,
            host,
            ..
        } = candidate;
        let mut entries = working;
        let victims = Self::select_victims(&entries, self.limits);
        for victim in &victims {
            entries.remove(victim);
        }
        let evicted = victims.len();
        if evicted > 0
            || self.eviction_history.order.len() > self.limits.max_eviction_history
            || self.eviction_history.estimated_bytes() > self.limits.max_bytes
        {
            self.eviction_history = Rc::new(self.eviction_history.with_evicted(
                victims,
                self.limits.max_eviction_history,
                self.limits.max_bytes,
            ));
        }
        let mut stats = GenerationStats {
            evicted,
            ..GenerationStats::default()
        };
        for row in &trace {
            match &row.disposition {
                Disposition::Reused => stats.reused += 1,
                Disposition::Recomputed { changed: true, .. } => stats.recomputed_changed += 1,
                Disposition::Recomputed { changed: false, .. } => {
                    stats.recomputed_unchanged += 1;
                }
                Disposition::Fresh { .. } => stats.fresh += 1,
            }
        }
        stats.entries = entries.len();
        stats.bytes = entries
            .values()
            .map(|entry| entry.entry.estimated_bytes)
            .sum();
        stats.eviction_history_len = self.eviction_history.order.len();
        stats.eviction_history_dropped = self.eviction_history.dropped;
        stats.host = host.stats();
        let view = Rc::new(GenerationView {
            id,
            parent: Some(parent.id),
            entries,
        });
        self.published = Rc::clone(&view);
        self.live.push(Rc::downgrade(&view));
        self.next_generation += 1;
        Ok((GenerationHandle { view }, stats))
    }

    /// Choose the least recently used entries (ties broken by key order)
    /// until both size limits hold. Pure: nothing is mutated.
    fn select_victims(
        entries: &BTreeMap<RequestKey, ViewEntry>,
        limits: RetentionLimits,
    ) -> Vec<RequestKey> {
        let mut remaining: Vec<(&RequestKey, &ViewEntry)> = entries.iter().collect();
        remaining
            .sort_by(|left, right| (left.1.last_used, left.0).cmp(&(right.1.last_used, right.0)));
        let mut count = entries.len();
        let mut bytes: usize = entries
            .values()
            .map(|entry| entry.entry.estimated_bytes)
            .sum();
        let mut victims = Vec::new();
        let mut candidates = remaining.into_iter();
        while count > limits.max_entries || bytes > limits.max_bytes {
            let Some((key, entry)) = candidates.next() else {
                break;
            };
            count -= 1;
            bytes -= entry.entry.estimated_bytes;
            victims.push(key.clone());
        }
        victims
    }

    /// Drop every entry from the next published view. Existing readers are
    /// unaffected; the next candidate starts from an empty parent. The live
    /// generation bound applies exactly as it does to `publish`.
    pub fn evict_all(&mut self) -> Result<(), CacheError> {
        self.check_live_bound()?;
        let victims: Vec<RequestKey> = self.published.entries.keys().cloned().collect();
        if !victims.is_empty()
            || self.eviction_history.order.len() > self.limits.max_eviction_history
            || self.eviction_history.estimated_bytes() > self.limits.max_bytes
        {
            self.eviction_history = Rc::new(self.eviction_history.with_evicted(
                victims,
                self.limits.max_eviction_history,
                self.limits.max_bytes,
            ));
        }
        let view = Rc::new(GenerationView {
            id: self.next_generation,
            parent: Some(self.published.id),
            entries: BTreeMap::new(),
        });
        self.published = Rc::clone(&view);
        self.live.push(Rc::downgrade(&view));
        self.next_generation += 1;
        Ok(())
    }
}

/// An unpublished candidate generation. Dropping it without `publish` is the
/// cancellation path: nothing it computed reaches the published view.
pub struct Candidate<'h> {
    owner: Rc<OwnerToken>,
    id: u64,
    parent: Rc<GenerationView>,
    working: BTreeMap<RequestKey, ViewEntry>,
    batch: ChangeBatch,
    report: InvalidationReport,
    identity: Rc<OptionsIdentity>,
    host: ObservationHost<'h>,
    raw_host: &'h dyn CompilerHost,
    options: &'h CompilerOptions,
    program_options: &'h ProgramOptions,
    case_sensitive: bool,
    trace: Vec<TraceRow>,
    requests_run: usize,
    eviction_history: Rc<EvictionHistory>,
}

impl<'h> Candidate<'h> {
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub fn parent_id(&self) -> u64 {
        self.parent.id
    }

    pub fn batch(&self) -> &ChangeBatch {
        &self.batch
    }

    pub fn invalidation(&self) -> &InvalidationReport {
        &self.report
    }

    pub fn identity(&self) -> &OptionsIdentity {
        &self.identity
    }

    pub fn trace(&self) -> &[TraceRow] {
        &self.trace
    }

    pub fn host_stats(&self) -> HostTrafficStats {
        self.host.stats()
    }

    /// Entries currently in the candidate's working view (reused + computed).
    pub fn working_entries(&self) -> impl Iterator<Item = &Rc<CacheEntry>> {
        self.working.values().map(|entry| &entry.entry)
    }

    /// The candidate-local usage stamp for `key`, if it is in the working
    /// view. Published views are never updated by a candidate.
    pub fn working_last_used(&self, key: &RequestKey) -> Option<u64> {
        self.working.get(key).map(|entry| entry.last_used)
    }

    fn containing_directory_key(
        &self,
        containing_file: JsStr<'_>,
    ) -> Result<PathKey, ResolutionError> {
        let current_directory = self.raw_host.current_directory_js()?;
        let normalized =
            normalize_absolute_js_path(containing_file, Some(current_directory.as_js()), true)?;
        let directory = crate::js_path::directory_name(normalized.as_js());
        Ok(PathKey::new(directory.as_js(), self.case_sensitive))
    }

    fn check_cancelled(&self, token: &CancellationToken) -> Result<(), CacheError> {
        if token.is_cancelled() {
            return Err(CacheError::Cancelled {
                after_requests: self.requests_run,
            });
        }
        Ok(())
    }

    fn lookup_or_compute(
        &mut self,
        key: RequestKey,
        token: &CancellationToken,
        compute: impl FnOnce(
            &ObservationHost<'h>,
            &'h CompilerOptions,
            &'h ProgramOptions,
        ) -> Result<CachedValue, ResolutionError>,
    ) -> Result<Rc<CacheEntry>, CacheError> {
        self.check_cancelled(token)?;
        let generation = self.id;
        if let Some(view_entry) = self.working.get_mut(&key) {
            // The usage stamp lives in this candidate's working view only;
            // the shared entry and every published view stay untouched.
            view_entry.last_used = generation;
            let entry = Rc::clone(&view_entry.entry);
            // A key already computed in this candidate is not a "reuse" of
            // the parent; only parent-held entries are reported as reused.
            if !self.trace.iter().any(|row| row.key == key) {
                self.trace.push(TraceRow {
                    key: key.clone(),
                    disposition: Disposition::Reused,
                    dependency_count: entry.dependencies.len(),
                });
            }
            self.requests_run += 1;
            return Ok(entry);
        }
        self.host.begin_recording();
        let computed = compute(&self.host, self.options, self.program_options);
        let dependencies = self.host.finish_recording();
        let value = match computed {
            Ok(value) => value,
            Err(error) => {
                // Errors are never cached and leave no trace row behind; the
                // request may be retried in a later generation.
                self.requests_run += 1;
                return Err(CacheError::Resolution(error));
            }
        };
        let disposition = match self.parent.entries.get(&key).map(|entry| &entry.entry) {
            Some(previous) => Disposition::Recomputed {
                violated: self
                    .batch
                    .violated_dependency(&previous.dependencies)
                    .cloned()
                    .unwrap_or_else(|| {
                        // Only reachable for an invalidate_all batch with an
                        // empty dependency set; report a synthetic marker.
                        Dependency::DirectoryExists {
                            path: key.containing_directory.clone(),
                            exists: true,
                        }
                    }),
                changed: previous.value != value,
            },
            None => Disposition::Fresh {
                evicted_before: self.eviction_history.contains(&key),
            },
        };
        let estimated_bytes =
            key.estimated_bytes() + value.estimated_bytes() + dependencies.estimated_bytes();
        let entry = Rc::new(CacheEntry {
            key: key.clone(),
            value,
            dependencies,
            created_generation: self.id,
            estimated_bytes,
        });
        self.working.insert(
            key.clone(),
            ViewEntry {
                entry: Rc::clone(&entry),
                last_used: self.id,
            },
        );
        self.trace.push(TraceRow {
            key,
            disposition,
            dependency_count: entry.dependencies.len(),
        });
        self.requests_run += 1;
        Ok(entry)
    }

    /// Resolve one module request through the cache.
    pub fn resolve_module(
        &mut self,
        containing_file: JsStr<'_>,
        specifier: JsStr<'_>,
        mode: ResolutionMode,
        token: &CancellationToken,
    ) -> Result<Rc<CacheEntry>, CacheError> {
        let directory = self.containing_directory_key(containing_file)?;
        let key = RequestKey::new(
            RequestKind::Module,
            directory,
            specifier,
            mode,
            Rc::clone(&self.identity),
        );
        let containing_file = containing_file.to_owned();
        let specifier = specifier.to_owned();
        self.lookup_or_compute(key, token, move |host, options, program_options| {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options)?;
            let resolution =
                resolver.resolve_with_facts(containing_file.as_js(), specifier.as_js(), mode)?;
            Ok(CachedValue::Module(resolution))
        })
    }

    /// Resolve one source-owned or automatic type-reference request.
    pub fn resolve_type_reference(
        &mut self,
        containing_file: JsStr<'_>,
        specifier: JsStr<'_>,
        mode: ResolutionMode,
        automatic: bool,
        token: &CancellationToken,
    ) -> Result<Rc<CacheEntry>, CacheError> {
        let directory = self.containing_directory_key(containing_file)?;
        let key = RequestKey::new(
            if automatic {
                RequestKind::AutomaticTypeReference
            } else {
                RequestKind::TypeReference
            },
            directory,
            specifier,
            mode,
            Rc::clone(&self.identity),
        );
        let containing_file = containing_file.to_owned();
        let specifier = specifier.to_owned();
        self.lookup_or_compute(key, token, move |host, options, program_options| {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options)?;
            let type_roots = program_options.type_roots();
            let outcome = resolver.resolve_type_reference(
                containing_file.as_js(),
                specifier.as_js(),
                mode,
                type_roots,
            )?;
            Ok(CachedValue::TypeReference(outcome))
        })
    }

    /// Resolve one `libReplacement` library package (`@typescript/lib-*`)
    /// from the synthetic containing file, with tsc's isolated Node10 option
    /// set (`getOptionsForLibraryResolution`, `_tsc.js:40643`).
    pub fn resolve_library(
        &mut self,
        resolve_from: JsStr<'_>,
        package_name: JsStr<'_>,
        token: &CancellationToken,
    ) -> Result<Rc<CacheEntry>, CacheError> {
        let directory = self.containing_directory_key(resolve_from)?;
        let key = RequestKey::new(
            RequestKind::Library,
            directory,
            package_name,
            ResolutionMode::Unspecified,
            Rc::clone(&self.identity),
        );
        let resolve_from = resolve_from.to_owned();
        let package_name = package_name.to_owned();
        self.lookup_or_compute(key, token, move |host, _, _| {
            let library_options = CompilerOptions {
                module_resolution: Some(2),
                ..CompilerOptions::default()
            };
            let mut resolver = ModuleResolver::new(host, &library_options)?;
            let outcome = resolver.resolve(
                resolve_from.as_js(),
                package_name.as_js(),
                ResolutionMode::Unspecified,
            )?;
            Ok(CachedValue::Library(outcome))
        })
    }

    /// Observe the nearest `package.json` scope of one source file and the
    /// implied node format the Program derives from it. The key is per file
    /// (directory plus base name) because the format also depends on the
    /// file's own extension.
    pub fn package_scope(
        &mut self,
        file: JsStr<'_>,
        token: &CancellationToken,
    ) -> Result<Rc<CacheEntry>, CacheError> {
        let directory = self.containing_directory_key(file)?;
        let base_name = crate::js_path::base_file_name(file);
        let key = RequestKey::new(
            RequestKind::PackageScope,
            directory,
            base_name.as_js(),
            ResolutionMode::Unspecified,
            Rc::clone(&self.identity),
        );
        let file = file.to_owned();
        self.lookup_or_compute(key, token, move |host, options, program_options| {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options)?;
            let scope = resolver.package_scope_for_file(file.as_js())?;
            let implied = crate::loader::implied_node_format(file.as_js(), scope.as_ref(), options);
            Ok(CachedValue::PackageScope(PackageScopeFacts {
                package_json: scope.as_ref().map(|scope| scope.package_json().clone()),
                module_type: scope
                    .as_ref()
                    .map_or(PackageJsonType::Unspecified, |scope| scope.module_type()),
                implied_node_format: implied,
            }))
        })
    }

    /// Discover wildcard automatic type directive names with the recorded
    /// directory-listing dependencies (tsc `getAutomaticTypeDirectiveNames`
    /// with `types: ["*"]`-style wildcard membership). The returned names are
    /// not cached themselves: their dependency set is returned so the driver
    /// can decide whether the name set must be recomputed.
    pub fn discover_automatic_type_directive_names(
        &mut self,
        token: &CancellationToken,
    ) -> Result<(Vec<JsString>, DependencySet), CacheError> {
        self.check_cancelled(token)?;
        self.host.begin_recording();
        let result = (|| -> Result<Vec<JsString>, ResolutionError> {
            let resolver = ModuleResolver::new_with_program_options(
                &self.host,
                self.options,
                self.program_options,
            )?;
            let roots = resolver.effective_type_roots(self.program_options.type_roots())?;
            crate::loader::discover_wildcard_type_directive_names(&self.host, &roots).map_err(
                |error| match error {
                    crate::loader::WildcardDiscoveryError::Host { error, .. } => {
                        ResolutionError::from(error)
                    }
                    crate::loader::WildcardDiscoveryError::Decode { path, source } => {
                        ResolutionError::invalid_data(format!(
                            "cannot decode {}: {source}",
                            path.to_string_lossy()
                        ))
                    }
                    crate::loader::WildcardDiscoveryError::InvalidData { path, detail } => {
                        ResolutionError::invalid_data(format!(
                            "{detail}: {}",
                            path.to_string_lossy()
                        ))
                    }
                },
            )
        })();
        let dependencies = self.host.finish_recording();
        self.requests_run += 1;
        Ok((result?, dependencies))
    }

    pub fn to_json(&self) -> Value {
        json!({
            "generation": self.id,
            "parent": self.parent.id,
            "identity_digest": format!("{:016x}", self.identity.digest()),
            "identity": self.identity.describe(),
            "eviction_history": {
                "len": self.eviction_history.order.len(),
                "dropped": self.eviction_history.dropped,
                "complete": self.eviction_history.dropped == 0,
            },
            "batch": self.batch.to_json(),
            "invalidation": self.report.to_json(),
            "trace": self.trace.iter().map(TraceRow::to_json).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::PathMapping;
    use tsc_types::ModuleSuffix;

    fn key(path: &str) -> PathKey {
        PathKey::new(path.into(), true)
    }

    #[test]
    fn path_key_containment_and_parent() {
        assert!(key("/p/a/b.ts").is_within(&key("/p/a")));
        assert!(key("/p/a/b.ts").is_within(&key("/")));
        assert!(!key("/p/ab.ts").is_within(&key("/p/a")));
        assert!(!key("/p/a").is_within(&key("/p/a")));
        assert_eq!(key("/p/a/b.ts").parent(), Some(key("/p/a")));
        assert_eq!(key("/p").parent(), Some(key("/")));
        assert_eq!(key("/").parent(), None);
    }

    #[test]
    fn negative_file_dependency_is_violated_by_creation_and_parent_creation() {
        let mut dependencies = DependencySet::default();
        dependencies.entries.insert(Dependency::FileExists {
            path: key("/p/lib/index.ts"),
            exists: false,
        });
        let mut batch = ChangeBatch::default();
        assert!(batch.violated_dependency(&dependencies).is_none());
        batch.created_directories.insert(key("/p/lib"));
        assert!(batch.violated_dependency(&dependencies).is_some());
        let mut batch = ChangeBatch::default();
        batch.created_files.insert(key("/p/lib/index.ts"));
        assert!(batch.violated_dependency(&dependencies).is_some());
        let mut batch = ChangeBatch::default();
        batch.created_files.insert(key("/p/other.ts"));
        assert!(batch.violated_dependency(&dependencies).is_none());
    }

    fn memory_host() -> tsc_host::MemoryCompilerHost {
        tsc_host::MemoryCompilerHost::builder_js("/p")
            .file_js("/p/main.ts", b"import 'pkg';".to_vec())
            .file_js(
                "/p/node_modules/pkg/package.json",
                br#"{"name":"pkg","exports":{"a,b":"./joined.d.ts","a":"./split.d.ts"}}"#.to_vec(),
            )
            .file_js("/p/node_modules/pkg/joined.d.ts", b"export {};".to_vec())
            .file_js("/p/node_modules/pkg/split.d.ts", b"export {};".to_vec())
            .file_js("/p/other.ts", b"export {};".to_vec())
            .file_js("/p/util.ts", b"export {};".to_vec())
            .build()
            .expect("memory host")
    }

    fn nodenext(conditions: Option<&[&str]>) -> CompilerOptions {
        CompilerOptions {
            module_resolution: Some(99),
            module: Some(199),
            custom_conditions: conditions
                .map(|list| list.iter().map(|entry| JsString::from(*entry)).collect()),
            ..CompilerOptions::default()
        }
    }

    fn identity_of(options: &CompilerOptions, program_options: &ProgramOptions) -> OptionsIdentity {
        OptionsIdentity::new(options, program_options, &memory_host())
    }

    fn resolve_pkg(candidate: &mut Candidate<'_>, specifier: &str) -> Rc<CacheEntry> {
        candidate
            .resolve_module(
                "/p/main.ts".into(),
                specifier.into(),
                ResolutionMode::CommonJs,
                &CancellationToken::default(),
            )
            .expect("resolve")
    }

    #[test]
    fn options_identity_is_structural_not_serialized() {
        let program = ProgramOptions::default();
        let joined = identity_of(&nodenext(Some(&["a,b"])), &program);
        let split = identity_of(&nodenext(Some(&["a", "b"])), &program);
        let reordered = identity_of(&nodenext(Some(&["b", "a"])), &program);
        let empty = identity_of(&nodenext(Some(&[])), &program);
        let absent = identity_of(&nodenext(None), &program);
        let identities = [&joined, &split, &reordered, &empty, &absent];
        for (left_index, left) in identities.iter().enumerate() {
            for (right_index, right) in identities.iter().enumerate() {
                assert_eq!(
                    left_index == right_index,
                    left == right,
                    "identities {left_index} and {right_index}"
                );
                assert_eq!(
                    left_index == right_index,
                    left.digest() == right.digest(),
                    "digests {left_index} and {right_index}"
                );
            }
        }
        assert_eq!(joined, identity_of(&nodenext(Some(&["a,b"])), &program));

        // Exact UTF-16 code units: a lone surrogate is not the replacement
        // character it would become under a lossy rendering.
        let lone = CompilerOptions {
            custom_conditions: Some(vec![JsString::from_code_units(&[0xD800])]),
            ..nodenext(None)
        };
        let replaced = CompilerOptions {
            custom_conditions: Some(vec![JsString::from("\u{FFFD}")]),
            ..nodenext(None)
        };
        assert_ne!(
            identity_of(&lone, &program),
            identity_of(&replaced, &program)
        );

        // moduleSuffixes element boundaries.
        let one = CompilerOptions {
            module_suffixes: Some(vec![ModuleSuffix::value(".a,b")]),
            ..nodenext(None)
        };
        let two = CompilerOptions {
            module_suffixes: Some(vec![ModuleSuffix::value(".a"), ModuleSuffix::value(".b")]),
            ..nodenext(None)
        };
        assert_ne!(identity_of(&one, &program), identity_of(&two, &program));

        // paths substitutions and patterns keep their boundaries.
        let single = ProgramOptions::default().with_config_paths(
            vec![PathMapping::new("@app/*", vec!["src,alt/*".into()])],
            "/p",
        );
        let pair = ProgramOptions::default().with_config_paths(
            vec![PathMapping::new(
                "@app/*",
                vec!["src/*".into(), "alt/*".into()],
            )],
            "/p",
        );
        let pattern_split = ProgramOptions::default().with_config_paths(
            vec![
                PathMapping::new("@app", vec!["src,alt/*".into()]),
                PathMapping::new("/*", vec![]),
            ],
            "/p",
        );
        let base = nodenext(None);
        assert_ne!(identity_of(&base, &single), identity_of(&base, &pair));
        assert_ne!(
            identity_of(&base, &single),
            identity_of(&base, &pattern_split)
        );

        // types, and the readable rendering, are distinct as well.
        let types_joined = ProgramOptions::default().with_types(vec!["a,b".into()]);
        let types_split = ProgramOptions::default().with_types(vec!["a".into(), "b".into()]);
        assert_ne!(
            identity_of(&base, &types_joined),
            identity_of(&base, &types_split)
        );
        assert_ne!(
            identity_of(&base, &types_joined).describe(),
            identity_of(&base, &types_split).describe()
        );
    }

    #[test]
    fn distinct_condition_arrays_never_reuse_a_resolution() {
        let host = memory_host();
        let program = ProgramOptions::default();
        let joined = nodenext(Some(&["a,b"]));
        let split = nodenext(Some(&["a", "b"]));
        let mut cache = ResolutionCache::new(RetentionLimits::default());
        let mut first = cache.begin(ChangeBatch::default(), &host, &joined, &program);
        let original = resolve_pkg(&mut first, "pkg");
        cache.publish(first).expect("publish joined");
        let mut next = cache.begin(ChangeBatch::default(), &host, &split, &program);
        let after = resolve_pkg(&mut next, "pkg");
        let mut fresh_cache = ResolutionCache::new(RetentionLimits::default());
        let mut fresh = fresh_cache.begin(ChangeBatch::default(), &host, &split, &program);
        let expected = resolve_pkg(&mut fresh, "pkg");
        assert_ne!(
            original.value(),
            expected.value(),
            "fixture selects different exports"
        );
        assert_eq!(after.value(), expected.value());
        assert!(matches!(
            next.trace().last().map(|row| &row.disposition),
            Some(Disposition::Fresh { .. })
        ));
    }

    #[test]
    fn dropped_candidate_leaves_published_stamps_and_lru_untouched() {
        let host = memory_host();
        let program = ProgramOptions::default();
        let options = nodenext(None);
        let mut cache = ResolutionCache::new(RetentionLimits::default());
        let mut first = cache.begin(ChangeBatch::default(), &host, &options, &program);
        let other = resolve_pkg(&mut first, "./other");
        let util = resolve_pkg(&mut first, "./util");
        let (held, _) = cache.publish(first).expect("publish");
        let stamps_before = held.view().usage_stamps();
        assert_eq!(held.view().last_used(other.key()), Some(1));
        assert_eq!(held.view().last_used(util.key()), Some(1));

        // A candidate touches `./other`, then is dropped.
        let mut dropped = cache.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut dropped, "./other");
        assert_eq!(dropped.working_last_used(other.key()), Some(2));
        assert_eq!(
            held.view().last_used(other.key()),
            Some(1),
            "published stamp untouched"
        );
        drop(dropped);
        assert_eq!(cache.published_id(), held.id());
        assert_eq!(held.view().usage_stamps(), stamps_before);

        // A refused publish also leaves the stamps alone.
        cache.set_limits(RetentionLimits {
            max_live_generations: 1,
            ..RetentionLimits::default()
        });
        let mut refused = cache.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut refused, "./other");
        assert!(matches!(
            cache.publish(refused),
            Err(CacheError::LiveGenerationLimit { .. })
        ));
        assert_eq!(held.view().usage_stamps(), stamps_before);

        // LRU decision with the untouched stamps: both entries were last
        // used in generation 1, so the smaller key (`./other`) is evicted.
        // Had the dropped candidate leaked its stamp, `./util` would go.
        cache.set_limits(RetentionLimits {
            max_entries: 1,
            max_live_generations: 8,
            ..RetentionLimits::default()
        });
        let quiet = cache.begin(ChangeBatch::default(), &host, &options, &program);
        let (published, stats) = cache.publish(quiet).expect("publish quiet");
        assert_eq!(stats.evicted, 1);
        assert!(
            published.view().get(util.key()).is_some(),
            "./util retained"
        );
        assert!(
            published.view().get(other.key()).is_none(),
            "./other evicted"
        );
        assert_eq!(
            held.view().usage_stamps(),
            stamps_before,
            "old reader immutable"
        );
    }

    #[test]
    fn foreign_and_stale_candidates_are_rejected() {
        let host = memory_host();
        let program = ProgramOptions::default();
        let options = nodenext(None);
        let mut source = ResolutionCache::new(RetentionLimits::default());
        let mut destination = ResolutionCache::new(RetentionLimits::default());
        let mut foreign = source.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut foreign, "./util");
        let before = destination.current();
        assert!(matches!(
            destination.publish(foreign),
            Err(CacheError::ForeignCandidate)
        ));
        assert_eq!(destination.published_id(), 0);
        assert!(Rc::ptr_eq(&before.view, &destination.current().view));

        // A stale candidate: another generation was published after it began.
        let mut stale = source.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut stale, "./util");
        let mut newer = source.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut newer, "./other");
        let (published, _) = source.publish(newer).expect("publish newer");
        assert!(matches!(
            source.publish(stale),
            Err(CacheError::ParentMismatch {
                expected: 1,
                actual: 0
            })
        ));
        assert!(Rc::ptr_eq(&published.view, &source.current().view));

        // A candidate begun before evict_all is stale too.
        let older = source.begin(ChangeBatch::default(), &host, &options, &program);
        source.evict_all().expect("evict all");
        assert!(matches!(
            source.publish(older),
            Err(CacheError::ParentMismatch { .. })
        ));
        assert!(source.current().view().is_empty());
    }

    #[test]
    fn eviction_history_is_bounded_and_reported() {
        let host = memory_host();
        let program = ProgramOptions::default();
        let options = nodenext(None);
        let limits = RetentionLimits {
            max_entries: 2,
            max_bytes: 1 << 20,
            max_live_generations: 4,
            max_eviction_history: 3,
        };
        let mut cache = ResolutionCache::new(limits);
        let mut evicted_total = 0;
        for generation in 0..20 {
            let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
            resolve_pkg(&mut candidate, &format!("./gen-{generation}"));
            let (_, stats) = cache.publish(candidate).expect("publish");
            evicted_total += stats.evicted;
            let resident = cache.resident_stats();
            assert!(resident.published_entries <= 2);
            assert!(resident.eviction_history_len <= 3, "{resident:?}");
            assert_eq!(resident.live_generation_records, 1, "no held readers");
        }
        let resident = cache.resident_stats();
        assert_eq!(evicted_total, 18);
        assert_eq!(resident.eviction_history_len, 3);
        assert_eq!(resident.eviction_history_dropped, 15);

        // A key evicted within the retained window is reported as such; one
        // forgotten beyond the window is not, and the candidate says so.
        let mut candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
        resolve_pkg(&mut candidate, "./gen-17");
        resolve_pkg(&mut candidate, "./gen-0");
        let dispositions: Vec<_> = candidate
            .trace()
            .iter()
            .map(|row| row.disposition.clone())
            .collect();
        assert!(matches!(
            dispositions[0],
            Disposition::Fresh {
                evicted_before: true
            }
        ));
        assert!(matches!(
            dispositions[1],
            Disposition::Fresh {
                evicted_before: false
            }
        ));
        assert_eq!(candidate.to_json()["eviction_history"]["complete"], false);
        cache.publish(candidate).expect("publish");

        // evict_all respects the live bound and prunes dead records. The
        // newest held handle is also the current published view, so four
        // held handles are four live records (three older views plus the
        // published one).
        let held: Vec<GenerationHandle> = (0..4)
            .map(|_| {
                let candidate = cache.begin(ChangeBatch::default(), &host, &options, &program);
                cache.publish(candidate).map(|(handle, _)| handle)
            })
            .collect::<Result<_, _>>()
            .expect("publish held generations");
        assert_eq!(cache.live_generation_count(), 4);
        assert!(matches!(
            cache.evict_all(),
            Err(CacheError::LiveGenerationLimit { live: 5, limit: 4 })
        ));
        drop(held);
        for _ in 0..10 {
            cache.evict_all().expect("evict all");
            assert!(cache.live_generation_count() <= 2);
        }
        let resident = cache.resident_stats();
        assert_eq!(resident.published_entries, 0);
        assert!(resident.eviction_history_len <= 3);
        assert!(resident.live_generation_records <= 2);
    }

    #[test]
    fn content_dependency_ignores_unrelated_changes() {
        let mut dependencies = DependencySet::default();
        dependencies.entries.insert(Dependency::FileContent {
            path: key("/p/node_modules/pkg/package.json"),
            digest: Some(1),
        });
        let mut batch = ChangeBatch::default();
        batch.changed_files.insert(key("/p/main.ts"));
        assert!(batch.violated_dependency(&dependencies).is_none());
        batch
            .changed_files
            .insert(key("/p/node_modules/pkg/package.json"));
        assert!(batch.violated_dependency(&dependencies).is_some());
        let mut batch = ChangeBatch::default();
        batch.deleted_directories.insert(key("/p/node_modules"));
        assert!(batch.violated_dependency(&dependencies).is_some());
    }
}

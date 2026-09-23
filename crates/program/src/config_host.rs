//! `CompilerHost` adapter for config-file discovery.
//!
//! Config parsing observes a deliberately smaller host surface than program
//! loading. Keeping the adapter in `tsc_program` makes the filesystem and
//! in-memory hosts use the same recursive enumeration, exclusion, decoding,
//! and TypeScript UTF-16 ordering rules instead of duplicating them in the CLI.

use std::collections::{BTreeSet, VecDeque};
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{CompilerHost, DirectoryListingEntry, DirectoryListingKind, HostError};

use crate::config::{ConfigHostError, ConfigHostOperation, ConfigParseHost};
use crate::config_matcher::{ConfigFilePattern, InputComponent, MatchInput};
use crate::decode_host_text;
use crate::js_path::{
    base_file_name, directory_name, eq_ignore_case, file_name_key, normalize_slashes, root_parts,
    uppercase,
};
use crate::module_resolution::normalize_absolute_js_path;

const MAX_DIRECTORY_DEPTH: usize = 256;

/// Threads that list directories ahead of the walk. Four read a 10,000
/// directory tree in about 55% of the time of one on APFS; eight are slower
/// than four.
const LISTING_THREADS: usize = 4;

/// Directory listings read ahead of the walk, by the directory spelling the
/// walk queries.
type Listings = rustc_hash::FxHashMap<JsString, Vec<DirectoryListingEntry>>;

/// Adapts any read-only [`CompilerHost`] to the config parser's
/// [`ConfigParseHost`] contract.
#[derive(Clone, Copy)]
pub struct CompilerConfigHost<'a> {
    host: &'a dyn CompilerHost,
}

impl<'a> CompilerConfigHost<'a> {
    pub const fn new(host: &'a dyn CompilerHost) -> Self {
        Self { host }
    }

    fn host_error(
        &self,
        operation: ConfigHostOperation,
        path: JsStr<'_>,
        error: HostError,
    ) -> ConfigHostError {
        ConfigHostError::new(operation, path, error.to_string())
    }

    /// `canonical` is the visited-set key of `directory` when the caller
    /// already knows it: a directory reached through its plain (non-link)
    /// entry has its parent's real path plus its name, so only a base
    /// directory and a linked entry need the host's realpath. `states` are
    /// every include and exclude pattern's automaton states at `directory`
    /// (see [`ConfigFilePattern::directory_states`]): each entry is then
    /// matched by its own name alone.
    #[allow(clippy::too_many_arguments)]
    fn walk_directory(
        &self,
        directory: JsStr<'_>,
        canonical: Option<JsString>,
        extensions: &[&str],
        patterns: &WalkPatterns,
        states: &PatternStates,
        depth: usize,
        files: &mut [Vec<JsString>],
        visited: &mut BTreeSet<JsString>,
        listings: &mut Listings,
    ) -> Result<(), ConfigHostError> {
        if depth == 0 {
            return Ok(());
        }
        let canonical_directory = match canonical {
            Some(canonical) => canonical,
            None => self.canonical_directory(directory)?,
        };
        if !visited.insert(canonical_directory.clone()) {
            return Ok(());
        }
        // A listing read ahead is the host's answer to this same query.
        let entries = match listings.remove(&directory.to_owned()) {
            Some(entries) => entries,
            None => self
                .host
                .read_directory_listing_js(directory)
                .map_err(|error| {
                    self.host_error(ConfigHostOperation::ReadDirectory, directory, error)
                })?,
        };
        // matchFiles.visitDirectory visits current files before child
        // directories (_tsc.js:18539–18571). CompilerHost already supplies
        // UTF-16 name order; partitioning preserves that order within each.
        let case_sensitive = self.host.use_case_sensitive_file_names();
        let mut child_directories = Vec::new();
        for entry in entries {
            let text = entry.path.as_js();
            if entry.kind == DirectoryListingKind::Directory {
                child_directories.push(entry);
                continue;
            }
            if !extensions.iter().any(|extension| text.ends_with(extension)) {
                continue;
            }
            let name = InputComponent::new(entry_name(text), case_sensitive);
            if states.excludes_entry(patterns, &name) {
                continue;
            }
            if patterns.includes.is_empty() {
                files[0].push(text.to_owned());
            } else if let Some(include_index) = states.include_index(patterns, &name) {
                files[include_index].push(text.to_owned());
            }
        }
        for entry in child_directories {
            let text = entry.path.as_js();
            let name = InputComponent::new(entry_name(text), case_sensitive);
            if let Some(child_states) = states.enter_child_directory(patterns, text, &name) {
                let child_canonical = (!entry.symlink).then(|| {
                    let mut key = canonical_directory.clone();
                    key.push_str("/");
                    key.push_js(file_name_key(entry_name(text), case_sensitive).as_js());
                    key
                });
                self.walk_directory(
                    text,
                    child_canonical,
                    extensions,
                    patterns,
                    &child_states,
                    depth - 1,
                    files,
                    visited,
                    listings,
                )?;
            }
        }
        Ok(())
    }

    /// List, on `LISTING_THREADS` threads, every directory the walk from
    /// `bases` will list through a plain (non-link) entry, so the walk's
    /// reads come from memory: the descent decision is the walk's own
    /// (`enter_child_directory`), a linked directory is left to the walk
    /// (its real path decides whether it is visited), and a listing that
    /// fails is left for the walk to read and report. Over a host that
    /// cannot be shared across threads nothing is read ahead.
    fn prefetch_listings(
        &self,
        bases: &[(JsString, PatternStates)],
        patterns: &WalkPatterns,
        depth: usize,
    ) -> Listings {
        let Some(host) = self.host.parallel_resolution_host() else {
            return Listings::default();
        };
        let threads = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(LISTING_THREADS);
        if threads < 2 || depth == 0 {
            return Listings::default();
        }
        struct Queue {
            pending: VecDeque<(JsString, PatternStates, usize)>,
            in_flight: usize,
            closed: bool,
        }
        let queue = std::sync::Mutex::new(Queue {
            pending: bases
                .iter()
                .map(|(base, states)| (base.clone(), states.clone(), depth))
                .collect(),
            in_flight: 0,
            closed: false,
        });
        let ready = std::sync::Condvar::new();
        let listings = std::sync::Mutex::new(Listings::default());
        let case_sensitive = self.host.use_case_sensitive_file_names();
        let run = || loop {
            let (directory, states, depth) = {
                let mut queue = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                loop {
                    if let Some(item) = queue.pending.pop_front() {
                        queue.in_flight += 1;
                        break item;
                    }
                    if queue.closed || queue.in_flight == 0 {
                        queue.closed = true;
                        ready.notify_all();
                        return;
                    }
                    queue = ready
                        .wait(queue)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
            };
            let listed = host.read_directory_listing_js(directory.as_js()).ok();
            let mut children = Vec::new();
            if let Some(entries) = &listed {
                if depth > 1 {
                    for entry in entries {
                        if entry.kind != DirectoryListingKind::Directory || entry.symlink {
                            continue;
                        }
                        let text = entry.path.as_js();
                        let name = InputComponent::new(entry_name(text), case_sensitive);
                        if let Some(child_states) =
                            states.enter_child_directory(patterns, text, &name)
                        {
                            children.push((entry.path.clone(), child_states, depth - 1));
                        }
                    }
                }
            }
            if let Some(entries) = listed {
                listings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(directory, entries);
            }
            let mut queue = queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue.in_flight -= 1;
            queue.pending.extend(children);
            if queue.pending.is_empty() && queue.in_flight == 0 {
                queue.closed = true;
                ready.notify_all();
            } else {
                ready.notify_all();
            }
        };
        std::thread::scope(|scope| {
            for _ in 1..threads {
                // A refused thread is not an error: the others list.
                let _ = std::thread::Builder::new()
                    .name("tsc-rs-worker".to_owned())
                    .spawn_scoped(scope, run);
            }
            run();
        });
        listings
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn canonical_directory(&self, directory: JsStr<'_>) -> Result<JsString, ConfigHostError> {
        let observed = self
            .host
            .realpath_js(directory)
            .map_err(|error| self.host_error(ConfigHostOperation::ReadDirectory, directory, error))?
            .unwrap_or_else(|| directory.to_owned());
        Ok(file_name_key(
            normalize_slashes(observed.as_js()).as_js(),
            self.host.use_case_sensitive_file_names(),
        ))
    }
}

impl ConfigParseHost for CompilerConfigHost<'_> {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    fn file_exists(&self, path: JsStr<'_>) -> Result<bool, ConfigHostError> {
        self.host
            .file_exists_js(path)
            .map_err(|error| self.host_error(ConfigHostOperation::FileExists, path, error))
    }

    fn read_file(&self, path: JsStr<'_>) -> Result<Option<String>, ConfigHostError> {
        let Some(bytes) = self
            .host
            .read_file_js(path)
            .map_err(|error| self.host_error(ConfigHostOperation::ReadFile, path, error))?
        else {
            return Ok(None);
        };
        decode_host_text(bytes).map(Some).map_err(|error| {
            ConfigHostError::new(ConfigHostOperation::ReadFile, path, error.to_string())
        })
    }

    fn read_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError> {
        let case_sensitive = self.host.use_case_sensitive_file_names();
        let patterns = WalkPatterns {
            includes: compile_patterns(includes, directory, case_sensitive)?,
            excludes: compile_patterns(excludes, directory, case_sensitive)?,
        };
        let mut file_buckets = (0..patterns.includes.len().max(1))
            .map(|_| Vec::new())
            .collect::<Vec<Vec<JsString>>>();
        let mut visited = BTreeSet::new();
        let depth = depth.unwrap_or(MAX_DIRECTORY_DEPTH);
        let bases = discovery_base_paths(directory, includes, case_sensitive)?
            .into_iter()
            .map(|base| {
                let states = PatternStates::at_base(&patterns, base.as_js(), case_sensitive);
                (base, states)
            })
            .collect::<Vec<_>>();
        let mut listings = self.prefetch_listings(&bases, &patterns, depth);
        for (base, states) in &bases {
            self.walk_directory(
                base.as_js(),
                None,
                extensions,
                &patterns,
                states,
                depth,
                &mut file_buckets,
                &mut visited,
                &mut listings,
            )?;
        }
        Ok(file_buckets.into_iter().flatten().collect())
    }
}

// getBasePaths/getIncludeBasePath (_tsc.js:18573–18596). Keep the config
// directory first even when a later include adds one of its ancestors.
fn discovery_base_paths(
    directory: JsStr<'_>,
    includes: Option<&[JsString]>,
    case_sensitive: bool,
) -> Result<Vec<JsString>, ConfigHostError> {
    let normalize = |path: JsStr<'_>| {
        let path = if path.is_empty() { directory } else { path };
        let mut normalized =
            normalize_absolute_js_path(path, Some(directory), true).map_err(|error| {
                ConfigHostError::new(
                    ConfigHostOperation::ReadDirectory,
                    directory,
                    error.to_string(),
                )
            })?;
        // normalizePath preserves a trailing separator; the shared helper
        // implements getNormalizedAbsolutePath, which can remove one.
        if (path.ends_with("/") || path.ends_with("\\")) && !normalized.ends_with("/") {
            normalized.push('/');
        }
        Ok::<_, ConfigHostError>(normalized)
    };
    let mut bases = vec![normalize(directory)?];
    let mut candidates = Vec::new();
    for include in includes.unwrap_or(&[]) {
        let slashed = normalize_slashes(include.as_js());
        let rooted_disk =
            root_parts(slashed.as_js()).is_some_and(|(root, _)| !root.contains("://"));
        // TypeScript preserves rooted disk spelling at this step; relative
        // paths and URLs go through normalizePath(combinePaths(...)).
        let absolute = if rooted_disk {
            include.clone()
        } else {
            normalize(include.as_js())?
        };
        let base = if let Some(wildcard) = absolute
            .as_bytes()
            .iter()
            .position(|byte| matches!(byte, b'*' | b'?'))
        {
            let before = absolute
                .as_js()
                .split_at_byte(wildcard)
                .expect("ASCII wildcard boundary")
                .0;
            let separator = before
                .as_bytes()
                .iter()
                .rposition(|byte| *byte == b'/')
                .unwrap_or(0);
            before
                .split_at_byte(separator)
                .expect("ASCII directory boundary")
                .0
                .to_owned()
        } else if discovery_has_extension(absolute.as_js()) {
            let parent = directory_name(absolute.as_js());
            parent
                .as_js()
                .strip_suffix("/")
                .unwrap_or(parent.as_js())
                .to_owned()
        } else {
            absolute
        };
        candidates.push(base);
    }
    candidates.sort_by_cached_key(|path| {
        if case_sensitive {
            path.clone()
        } else {
            uppercase(path.as_js())
        }
        .to_utf16()
    });
    for candidate in candidates {
        let normalized_candidate = normalize(candidate.as_js())?;
        let mut covered = false;
        for base in &bases {
            if discovery_path_contains(
                normalize(base.as_js())?.as_js(),
                normalized_candidate.as_js(),
                case_sensitive,
            ) {
                covered = true;
                break;
            }
        }
        if !covered {
            bases.push(candidate);
        }
    }
    Ok(bases)
}

/// The compiled include and exclude patterns of one walk.
struct WalkPatterns {
    includes: Vec<ConfigFilePattern>,
    excludes: Vec<ConfigFilePattern>,
}

/// Every pattern's automaton states at one directory of the walk (`None`:
/// nothing below the directory can match that pattern).
#[derive(Clone)]
struct PatternStates {
    includes: Vec<Option<Vec<usize>>>,
    excludes: Vec<Option<Vec<usize>>>,
}

impl PatternStates {
    /// The states at a base directory of the walk, from its whole path.
    fn at_base(patterns: &WalkPatterns, base: JsStr<'_>, case_sensitive: bool) -> Self {
        let input = MatchInput::new(base, case_sensitive);
        let states = |pattern: &ConfigFilePattern| {
            input
                .as_ref()
                .and_then(|input| pattern.directory_states(input))
        };
        Self {
            includes: patterns.includes.iter().map(states).collect(),
            excludes: patterns.excludes.iter().map(states).collect(),
        }
    }

    /// Whether an exclude pattern matches the entry `name` of this directory.
    fn excludes_entry(&self, patterns: &WalkPatterns, name: &InputComponent) -> bool {
        patterns
            .excludes
            .iter()
            .zip(&self.excludes)
            .any(|(pattern, states)| {
                states
                    .as_ref()
                    .is_some_and(|states| pattern.accepts_entry(states, name))
            })
    }

    /// The first include pattern matching the entry `name` of this directory.
    fn include_index(&self, patterns: &WalkPatterns, name: &InputComponent) -> Option<usize> {
        patterns
            .includes
            .iter()
            .zip(&self.includes)
            .position(|(pattern, states)| {
                states
                    .as_ref()
                    .is_some_and(|states| pattern.accepts_entry(states, name))
            })
    }

    /// matchFiles.visitDirectory's descent decision for the child directory
    /// `name` (_tsc.js:18562–18570), with the states below it when it is
    /// entered: a package directory is excluded by the implicit recursive
    /// wildcard unless an explicit include such as `node_modules/**/*.ts`
    /// can still enter it, and otherwise the directory is entered when no
    /// exclude matches it and some include could match a descendant.
    fn enter_child_directory(
        &self,
        patterns: &WalkPatterns,
        text: JsStr<'_>,
        name: &InputComponent,
    ) -> Option<Self> {
        let includes = patterns
            .includes
            .iter()
            .zip(&self.includes)
            .map(|(pattern, states)| {
                states
                    .as_ref()
                    .and_then(|states| pattern.advance_directory(states, name))
            })
            .collect::<Vec<_>>();
        let some_include_could_match = includes.iter().any(Option::is_some);
        if !patterns.includes.is_empty()
            && is_implicit_excluded_directory(text)
            && !some_include_could_match
        {
            return None;
        }
        if self.excludes_entry(patterns, name)
            || !(patterns.includes.is_empty() || some_include_could_match)
        {
            return None;
        }
        let excludes = patterns
            .excludes
            .iter()
            .zip(&self.excludes)
            .map(|(pattern, states)| {
                states
                    .as_ref()
                    .and_then(|states| pattern.advance_directory(states, name))
            })
            .collect::<Vec<_>>();
        Some(Self { includes, excludes })
    }
}

/// The last path component of an entry path spelled by the listing.
fn entry_name(text: JsStr<'_>) -> JsStr<'_> {
    text.split_ascii(b'/').next_back().unwrap_or(text)
}

fn discovery_has_extension(path: JsStr<'_>) -> bool {
    let slashed = normalize_slashes(path);
    if root_parts(slashed.as_js()).is_some_and(|(_, tail)| tail.is_empty()) {
        return false;
    }
    // getBaseFileName removes one trailing separator, not all of them.
    let result = slashed
        .as_js()
        .strip_suffix("/")
        .unwrap_or(slashed.as_js())
        .split_ascii(b'/')
        .next_back()
        .is_some_and(|name| name.contains("."));
    result
}

fn discovery_path_contains(parent: JsStr<'_>, child: JsStr<'_>, case_sensitive: bool) -> bool {
    let (parent_root, parent_tail) = root_parts(parent).expect("normalized parent");
    let (child_root, child_tail) = root_parts(child).expect("normalized child");
    // containsPath compares roots without case even on a case-sensitive host.
    if !eq_ignore_case(parent_root, child_root) {
        return false;
    }
    let mut child_components = child_tail.split_ascii(b'/').filter(|part| !part.is_empty());
    parent_tail
        .split_ascii(b'/')
        .filter(|part| !part.is_empty())
        .all(|parent| {
            child_components.next().is_some_and(|child| {
                if case_sensitive {
                    parent == child
                } else {
                    eq_ignore_case(parent, child)
                }
            })
        })
}

fn compile_patterns(
    patterns: Option<&[JsString]>,
    directory: JsStr<'_>,
    case_sensitive: bool,
) -> Result<Vec<ConfigFilePattern>, ConfigHostError> {
    let mut compiled = Vec::new();
    for pattern in patterns.unwrap_or(&[]) {
        let pattern =
            ConfigFilePattern::new(pattern, directory, case_sensitive).map_err(|detail| {
                ConfigHostError::new(ConfigHostOperation::ReadDirectory, directory, detail)
            })?;
        if let Some(pattern) = pattern {
            compiled.push(pattern);
        }
    }
    Ok(compiled)
}

fn is_implicit_excluded_directory(path: JsStr<'_>) -> bool {
    // This finite ASCII catalog cannot match a non-scalar basename. The
    // directory path itself remains a JS value throughout the walk.
    base_file_name(path).as_str().is_some_and(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "node_modules" | "bower_components" | "jspm_packages"
        )
    })
}

#[cfg(test)]
#[path = "../tests/unit/config_host/tests.rs"]
mod tests;

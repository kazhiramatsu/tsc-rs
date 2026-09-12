//! `CompilerHost` adapter for config-file discovery.
//!
//! Config parsing observes a deliberately smaller host surface than program
//! loading. Keeping the adapter in `tsc_program` makes the filesystem and
//! in-memory hosts use the same recursive enumeration, exclusion, decoding,
//! and TypeScript UTF-16 ordering rules instead of duplicating them in the CLI.

use std::collections::BTreeSet;
use std::path::Path;

use tsc_host::{to_file_name_lower_case, CompilerHost, HostError};

use crate::config::{ConfigHostError, ConfigHostOperation, ConfigParseHost};
use crate::config_matcher::ConfigFilePattern;
use crate::decode_host_text;
use crate::module_resolution::{directory_name, normalize_absolute_path, normalized_root_parts};

const MAX_DIRECTORY_DEPTH: usize = 256;

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
        path: &str,
        error: HostError,
    ) -> ConfigHostError {
        ConfigHostError::new(operation, path, error.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_directory(
        &self,
        directory: &Path,
        extensions: &[&str],
        includes: &[ConfigFilePattern],
        excludes: &[ConfigFilePattern],
        depth: usize,
        files: &mut [Vec<String>],
        visited: &mut BTreeSet<String>,
    ) -> Result<(), ConfigHostError> {
        if depth == 0 {
            return Ok(());
        }
        let canonical_directory = self.canonical_directory(directory)?;
        if !visited.insert(canonical_directory) {
            return Ok(());
        }
        let entries = self.host.read_directory(directory).map_err(|error| {
            let path = directory.display().to_string();
            self.host_error(ConfigHostOperation::ReadDirectory, &path, error)
        })?;
        // matchFiles.visitDirectory visits current files before child
        // directories (_tsc.js:18539–18571). CompilerHost already supplies
        // UTF-16 name order; partitioning preserves that order within each.
        let mut child_directories = Vec::new();
        for entry in entries {
            let text = entry.to_str().ok_or_else(|| {
                ConfigHostError::new(
                    ConfigHostOperation::ReadDirectory,
                    entry.display().to_string(),
                    "filesystem entry is not Unicode",
                )
            })?;
            if self
                .host
                .directory_exists(&entry)
                .map_err(|error| self.host_error(ConfigHostOperation::ReadDirectory, text, error))?
            {
                child_directories.push(entry);
                continue;
            }
            if !extensions.iter().any(|extension| text.ends_with(extension)) {
                continue;
            }
            if excludes.iter().any(|pattern| pattern.matches(text)) {
                continue;
            }
            if includes.is_empty() {
                files[0].push(text.to_owned());
            } else if let Some(include_index) =
                includes.iter().position(|pattern| pattern.matches(text))
            {
                files[include_index].push(text.to_owned());
            }
        }
        for entry in child_directories {
            let text = entry.to_str().expect("directory entry was validated above");
            if !includes.is_empty() && is_implicit_excluded_directory(&entry) {
                // A package directory is excluded by the implicit recursive
                // wildcard, but an explicit include such as
                // `node_modules/**/*.ts` must still be able to enter it.
                if !includes
                    .iter()
                    .any(|pattern| pattern.could_match_descendant(text))
                {
                    continue;
                }
            }
            if !excludes.iter().any(|pattern| pattern.matches(text))
                && (includes.is_empty()
                    || includes
                        .iter()
                        .any(|pattern| pattern.could_match_descendant(text)))
            {
                self.walk_directory(
                    &entry,
                    extensions,
                    includes,
                    excludes,
                    depth - 1,
                    files,
                    visited,
                )?;
            }
        }
        Ok(())
    }

    fn canonical_directory(&self, directory: &Path) -> Result<String, ConfigHostError> {
        let observed = self
            .host
            .realpath(directory)
            .map_err(|error| {
                let path = directory.display().to_string();
                self.host_error(ConfigHostOperation::ReadDirectory, &path, error)
            })?
            .unwrap_or_else(|| directory.to_path_buf());
        let text = observed.to_str().ok_or_else(|| {
            ConfigHostError::new(
                ConfigHostOperation::ReadDirectory,
                observed.display().to_string(),
                "filesystem path is not Unicode",
            )
        })?;
        let normalized = text.replace('\\', "/");
        Ok(if self.host.use_case_sensitive_file_names() {
            normalized
        } else {
            to_file_name_lower_case(&normalized)
        })
    }
}

impl ConfigParseHost for CompilerConfigHost<'_> {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        self.host
            .file_exists(Path::new(path))
            .map_err(|error| self.host_error(ConfigHostOperation::FileExists, path, error))
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        let Some(bytes) = self
            .host
            .read_file(Path::new(path))
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
        directory: &str,
        extensions: &[&str],
        excludes: Option<&[String]>,
        includes: Option<&[String]>,
        depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        let case_sensitive = self.host.use_case_sensitive_file_names();
        let include_patterns = compile_patterns(includes, directory, case_sensitive)?;
        let exclude_patterns = compile_patterns(excludes, directory, case_sensitive)?;
        let mut file_buckets = (0..include_patterns.len().max(1))
            .map(|_| Vec::new())
            .collect::<Vec<Vec<String>>>();
        let mut visited = BTreeSet::new();
        for base in discovery_base_paths(directory, includes, case_sensitive)? {
            self.walk_directory(
                Path::new(&base),
                extensions,
                &include_patterns,
                &exclude_patterns,
                depth.unwrap_or(MAX_DIRECTORY_DEPTH),
                &mut file_buckets,
                &mut visited,
            )?;
        }
        Ok(file_buckets.into_iter().flatten().collect())
    }
}

// getBasePaths/getIncludeBasePath (_tsc.js:18573–18596). Keep the config
// directory first even when a later include adds one of its ancestors.
fn discovery_base_paths(
    directory: &str,
    includes: Option<&[String]>,
    case_sensitive: bool,
) -> Result<Vec<String>, ConfigHostError> {
    let normalize = |path: &str| {
        let path = if path.is_empty() { directory } else { path };
        let mut normalized =
            normalize_absolute_path(Path::new(path), Some(directory)).map_err(|error| {
                ConfigHostError::new(
                    ConfigHostOperation::ReadDirectory,
                    directory,
                    error.to_string(),
                )
            })?;
        // normalizePath preserves a trailing separator; the shared helper
        // implements getNormalizedAbsolutePath, which can remove one.
        if path.ends_with(['/', '\\']) && !normalized.ends_with('/') {
            normalized.push('/');
        }
        Ok::<_, ConfigHostError>(normalized)
    };
    let mut bases = vec![normalize(directory)?];
    let mut candidates = Vec::new();
    for include in includes.unwrap_or(&[]) {
        let slashed = include.replace('\\', "/");
        let rooted_disk =
            normalized_root_parts(&slashed).is_some_and(|(root, _)| !root.contains("://"));
        // TypeScript preserves rooted disk spelling at this step; relative
        // paths and URLs go through normalizePath(combinePaths(...)).
        let absolute = if rooted_disk {
            include.clone()
        } else {
            normalize(include)?
        };
        let base = if let Some(wildcard) = absolute.find(['*', '?']) {
            absolute[..absolute[..wildcard].rfind('/').unwrap_or(0)].to_owned()
        } else if discovery_has_extension(&absolute) {
            let parent = directory_name(&absolute);
            parent.strip_suffix('/').unwrap_or(&parent).to_owned()
        } else {
            absolute
        };
        candidates.push(base);
    }
    candidates.sort_by_cached_key(|path| {
        if case_sensitive {
            path.clone()
        } else {
            path.to_uppercase()
        }
        .encode_utf16()
        .collect::<Vec<_>>()
    });
    for candidate in candidates {
        let normalized_candidate = normalize(&candidate)?;
        let mut covered = false;
        for base in &bases {
            if discovery_path_contains(&normalize(base)?, &normalized_candidate, case_sensitive) {
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

fn discovery_has_extension(path: &str) -> bool {
    let slashed = path.replace('\\', "/");
    if normalized_root_parts(&slashed).is_some_and(|(_, tail)| tail.is_empty()) {
        return false;
    }
    // getBaseFileName removes one trailing separator, not all of them.
    slashed
        .strip_suffix('/')
        .unwrap_or(&slashed)
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
}

fn discovery_path_contains(parent: &str, child: &str, case_sensitive: bool) -> bool {
    let (parent_root, parent_tail) = normalized_root_parts(parent).expect("normalized parent");
    let (child_root, child_tail) = normalized_root_parts(child).expect("normalized child");
    // containsPath compares roots without case even on a case-sensitive host.
    if parent_root.to_uppercase() != child_root.to_uppercase() {
        return false;
    }
    let mut child_components = child_tail.split('/').filter(|part| !part.is_empty());
    parent_tail
        .split('/')
        .filter(|part| !part.is_empty())
        .all(|parent| {
            child_components.next().is_some_and(|child| {
                if case_sensitive {
                    parent == child
                } else {
                    parent.to_uppercase() == child.to_uppercase()
                }
            })
        })
}

fn compile_patterns(
    patterns: Option<&[String]>,
    directory: &str,
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

fn is_implicit_excluded_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "node_modules" | "bower_components" | "jspm_packages"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::discovery_base_paths;
    use serde_json::{json, Value};

    #[test]
    fn config_discovery_base_paths_match_typescript() {
        let oracle: Value = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/h2-8b-config-discovery-paths.json"
        ))
        .expect("frozen base path observations");
        assert_path_cases(&oracle, 16);
    }

    #[test]
    fn config_discovery_spelling_matches_typescript() {
        let oracle: Value = serde_json::from_slice(include_bytes!(
            "../tests/fixtures/h2-8b-config-discovery-spelling.json"
        ))
        .expect("frozen spelling observations");
        assert_path_cases(&oracle, 6);
    }

    fn assert_path_cases(oracle: &Value, count: usize) {
        assert_eq!(oracle["typescript"], "6.0.3");
        let cases = oracle["cases"].as_array().expect("path cases");
        assert_eq!(cases.len(), count);
        let mut failures = Vec::new();
        for case in cases {
            let includes = case["includes"].as_array().map(|values| {
                values
                    .iter()
                    .map(|value| value.as_str().expect("include").to_owned())
                    .collect::<Vec<_>>()
            });
            for repetition in 1..=2 {
                let actual = discovery_base_paths(
                    case["directory"].as_str().expect("directory"),
                    includes.as_deref(),
                    case["case_sensitive"].as_bool().expect("case policy"),
                )
                .expect("valid discovery paths");
                let exact = json!(actual) == case["base_paths"];
                eprintln!(
                    "H2.8b-CFG1b-path {}",
                    json!({
                        "case_id": case["case_id"], "repetition": repetition, "actual": actual, "exact": exact,
                    })
                );
                if !exact {
                    failures.push(format!(
                        "{}: {:?} != {}",
                        case["case_id"], actual, case["base_paths"]
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

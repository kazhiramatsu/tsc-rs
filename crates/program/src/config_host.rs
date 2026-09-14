//! `CompilerHost` adapter for config-file discovery.
//!
//! Config parsing observes a deliberately smaller host surface than program
//! loading. Keeping the adapter in `tsc_program` makes the filesystem and
//! in-memory hosts use the same recursive enumeration, exclusion, decoding,
//! and TypeScript UTF-16 ordering rules instead of duplicating them in the CLI.

use std::collections::BTreeSet;
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{CompilerHost, HostError};

use crate::config::{ConfigHostError, ConfigHostOperation, ConfigParseHost};
use crate::config_matcher::ConfigFilePattern;
use crate::decode_host_text;
use crate::js_path::{
    base_file_name, directory_name, eq_ignore_case, file_name_key, normalize_slashes, root_parts,
    uppercase,
};
use crate::module_resolution::normalize_absolute_js_path;

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
        path: JsStr<'_>,
        error: HostError,
    ) -> ConfigHostError {
        ConfigHostError::new(operation, path, error.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        includes: &[ConfigFilePattern],
        excludes: &[ConfigFilePattern],
        depth: usize,
        files: &mut [Vec<JsString>],
        visited: &mut BTreeSet<JsString>,
    ) -> Result<(), ConfigHostError> {
        if depth == 0 {
            return Ok(());
        }
        let canonical_directory = self.canonical_directory(directory)?;
        if !visited.insert(canonical_directory) {
            return Ok(());
        }
        let entries = self.host.read_directory_js(directory).map_err(|error| {
            self.host_error(ConfigHostOperation::ReadDirectory, directory, error)
        })?;
        // matchFiles.visitDirectory visits current files before child
        // directories (_tsc.js:18539–18571). CompilerHost already supplies
        // UTF-16 name order; partitioning preserves that order within each.
        let mut child_directories = Vec::new();
        for entry in entries {
            let text = entry.as_js();
            if self
                .host
                .directory_exists_js(entry.as_js())
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
            let text = entry.as_js();
            if !includes.is_empty() && is_implicit_excluded_directory(entry.as_js()) {
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
                    entry.as_js(),
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
        let include_patterns = compile_patterns(includes, directory, case_sensitive)?;
        let exclude_patterns = compile_patterns(excludes, directory, case_sensitive)?;
        let mut file_buckets = (0..include_patterns.len().max(1))
            .map(|_| Vec::new())
            .collect::<Vec<Vec<JsString>>>();
        let mut visited = BTreeSet::new();
        for base in discovery_base_paths(directory, includes, case_sensitive)? {
            self.walk_directory(
                base.as_js(),
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
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn config_host_keeps_js_directory_keys_patterns_and_read_names() {
        let base = JsString::from_code_units(&[0x2f, 0x77, 0x6f, 0x72, 0x6b, 0x2f, 0xd800]);
        let leaves = [0xd800, 0xd801, 0xfffd].map(|unit| {
            let mut path = base.clone();
            path.push('/');
            path.push_code_unit(unit);
            path.push_str("/a.ts");
            path
        });
        let mut builder = tsc_host::MemoryCompilerHost::builder_js("/work");
        for (index, path) in leaves.iter().enumerate() {
            builder = builder.file_js(path, format!("source {index}").into_bytes());
        }
        let host = builder.build().unwrap();
        let config = CompilerConfigHost::new(&host);
        let mut first_pattern = JsString::from_code_units(&[0xd801]);
        first_pattern.push_str("/**/*.ts");
        let includes = [first_pattern.clone(), JsString::from("**/*.ts")];
        assert_eq!(
            config
                .read_directory(base.as_js(), &[".ts"], None, Some(&includes), None)
                .unwrap(),
            [leaves[1].clone(), leaves[0].clone(), leaves[2].clone()]
        );
        assert_eq!(
            config
                .read_directory(
                    base.as_js(),
                    &[".ts"],
                    Some(&[first_pattern]),
                    Some(&includes),
                    None
                )
                .unwrap(),
            [leaves[0].clone(), leaves[2].clone()]
        );
        for (index, path) in leaves.iter().enumerate() {
            assert!(config.file_exists(path.as_js()).unwrap());
            assert_eq!(
                config.read_file(path.as_js()).unwrap(),
                Some(format!("source {index}"))
            );
        }
    }

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
                    .map(|value| JsString::from(value.as_str().expect("include")))
                    .collect::<Vec<_>>()
            });
            for repetition in 1..=2 {
                let actual = discovery_base_paths(
                    case["directory"].as_str().expect("directory").into(),
                    includes.as_deref(),
                    case["case_sensitive"].as_bool().expect("case policy"),
                )
                .expect("valid discovery paths");
                let actual = actual
                    .iter()
                    .map(|path| {
                        path.as_str()
                            .expect("these frozen controls contain scalar path values")
                    })
                    .collect::<Vec<_>>();
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

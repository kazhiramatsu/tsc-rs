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
                if !includes.is_empty() && is_implicit_excluded_directory(&entry) {
                    // A package directory is excluded by the implicit
                    // recursive wildcard, but an explicit include such as
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
        self.walk_directory(
            Path::new(directory),
            extensions,
            &include_patterns,
            &exclude_patterns,
            depth.unwrap_or(MAX_DIRECTORY_DEPTH),
            &mut file_buckets,
            &mut visited,
        )?;
        Ok(file_buckets.into_iter().flatten().collect())
    }
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

//! tsgo `DefaultResolver.GetEntrypointsFromPackageJsonInfo`: the files a
//! package offers its importers, each with the module specifier that
//! reaches it — the targets of its `exports` map, else its main entry and,
//! with a directory search, every other TypeScript file in it. The
//! auto-import registry indexes their exports.

use std::collections::BTreeSet;

use tsc_diagnostics::{JsStr, JsString};

use super::{
    canonical_text, is_applicable_versioned_types_key, join_normalized,
    json_object_entries_in_order, path_relative_to_directory, CachedPackage, ExtensionProbePass,
    LegacyResolutionContext, ModuleResolver, ResolutionError, ResolutionMode, Value,
};
use crate::config::{ConfigHostError, ConfigParseHost};
use crate::js_path::{change_full_extension, normalize_path};
use crate::resolution::ResolutionOutcome;
use crate::CompilerConfigHost;

/// tsgo `extensionsTypeScript | extensionsDeclaration` as `Array()`: the
/// files an entrypoint can be.
const ENTRYPOINT_EXTENSIONS: [&str; 7] =
    [".ts", ".tsx", ".mts", ".cts", ".d.ts", ".d.cts", ".d.mts"];

/// tsgo `module.Ending`: which part of an entrypoint's specifier an
/// importer may spell otherwise.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntrypointEnding {
    /// The specifier is as an `exports` map writes it.
    Fixed,
    /// The extension was inferred from a file on disk, so an
    /// interchangeable one could be used instead (`.d.ts` as `.js`).
    ExtensionChangeable,
    /// The file name and extension were inferred from a file on disk
    /// without an `exports` pattern, so the importer's module resolution
    /// rules may change them (an `/index.d.ts` dropped for CommonJS).
    Changeable,
}

/// tsgo `module.ResolvedEntrypoint`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedEntrypoint {
    /// The path the entrypoint was found at when that is a symlink.
    pub original_file_name: Option<String>,
    /// The real path of the entrypoint's file.
    pub resolved_file_name: String,
    /// `resolved_file_name` as a path key.
    pub resolved_path: String,
    pub module_specifier: String,
    pub ending: EntrypointEnding,
    /// The conditions a resolver must have to reach the entrypoint.
    pub include_conditions: BTreeSet<String>,
    /// The conditions a resolver must not have to reach it.
    pub exclude_conditions: BTreeSet<String>,
}

impl ResolvedEntrypoint {
    /// tsgo `SymlinkOrRealpath`.
    pub fn symlink_or_realpath(&self) -> &str {
        self.original_file_name
            .as_deref()
            .unwrap_or(&self.resolved_file_name)
    }
}

/// The probes of an entrypoint: none of a resolution's provenance, and the
/// path where it was found (tsgo resolves symlinks afterwards).
const ENTRYPOINT_PROBE: LegacyResolutionContext = LegacyResolutionContext {
    is_external_library_import: false,
    attach_package_id: false,
    resolved_using_ts_extension: false,
    follow_realpath: false,
};

fn read_directory_error(error: ConfigHostError) -> ResolutionError {
    ResolutionError::invalid_data(error.to_string())
}

impl ModuleResolver<'_> {
    /// tsgo `GetEntrypointsFromPackageJsonInfo`: the entrypoints of the
    /// package in `package_directory` (its package.json, if any, read
    /// there), named from `package_name`.
    pub fn get_entrypoints_from_package_json_info(
        &mut self,
        package_directory: JsStr<'_>,
        package_name: &str,
        enable_directory_search: bool,
    ) -> Result<Vec<ResolvedEntrypoint>, ResolutionError> {
        let package = self.load_package(&join_normalized(package_directory, "package.json"))?;
        if let Some(package) = package.as_deref().filter(|package| package.has_own_exports) {
            let exports = package.exports.clone().unwrap_or(Value::Null);
            return self.load_entrypoints_from_export_map(package, package_name, &exports);
        }

        let mut result = Vec::new();
        let main = match package.as_deref() {
            Some(package) => self.probe_legacy_directory_worker(
                package,
                package_directory,
                ExtensionProbePass::Preferred,
                ResolutionMode::Unspecified,
                ENTRYPOINT_PROBE,
            )?,
            None if self.host.directory_exists_js(package_directory)? => self.probe_legacy_file(
                None,
                &join_normalized(package_directory, "index"),
                ExtensionProbePass::Preferred,
                /* allow_implicit */ true,
                ENTRYPOINT_PROBE,
            )?,
            None => ResolutionOutcome::NotFound,
        };
        let main_path = match main {
            ResolutionOutcome::Resolved(main) => {
                let path = main.resolved_file().display().to_owned();
                result.push(self.create_resolved_entrypoint_handling_symlink(
                    path.as_js(),
                    package_name.to_owned(),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    EntrypointEnding::Fixed,
                )?);
                Some(path)
            }
            ResolutionOutcome::NotFound => None,
        };

        if enable_directory_search {
            let other_files = CompilerConfigHost::new(self.host)
                .read_directory(
                    package_directory,
                    &ENTRYPOINT_EXTENSIONS,
                    Some(&[JsString::from("node_modules")]),
                    Some(&[JsString::from("**/*")]),
                    None,
                )
                .map_err(read_directory_error)?;
            let case_sensitive = self.path_context.use_case_sensitive_file_names();
            for file in other_files {
                if main_path.as_ref().is_some_and(|main| {
                    canonical_text(file.as_js(), case_sensitive)
                        == canonical_text(main.as_js(), case_sensitive)
                }) {
                    continue;
                }
                let Some(relative) = path_relative_to_directory(file.as_js(), package_directory)
                else {
                    continue;
                };
                let specifier = format!("{package_name}/{}", relative.to_string_lossy());
                result.push(self.create_resolved_entrypoint_handling_symlink(
                    file.as_js(),
                    specifier,
                    BTreeSet::new(),
                    BTreeSet::new(),
                    EntrypointEnding::Changeable,
                )?);
            }
        }
        Ok(result)
    }

    /// tsgo `createResolvedEntrypointHandlingSymlink`: an entrypoint at its
    /// real path, remembering the symlink it was found at.
    fn create_resolved_entrypoint_handling_symlink(
        &self,
        file_name: JsStr<'_>,
        module_specifier: String,
        include_conditions: BTreeSet<String>,
        exclude_conditions: BTreeSet<String>,
        ending: EntrypointEnding,
    ) -> Result<ResolvedEntrypoint, ResolutionError> {
        let realpath = self.host.realpath_js(file_name)?;
        let (original_file_name, resolved_file_name) = match realpath {
            Some(realpath) if realpath.as_js() != file_name => (
                Some(file_name.to_string_lossy().into_owned()),
                realpath.to_string_lossy().into_owned(),
            ),
            _ => (None, file_name.to_string_lossy().into_owned()),
        };
        let resolved_path = canonical_text(
            JsStr::from_str(&resolved_file_name),
            self.path_context.use_case_sensitive_file_names(),
        )
        .to_string_lossy()
        .into_owned();
        Ok(ResolvedEntrypoint {
            original_file_name,
            resolved_file_name,
            resolved_path,
            module_specifier,
            ending,
            include_conditions,
            exclude_conditions,
        })
    }

    /// tsgo `loadEntrypointsFromExportMap`.
    fn load_entrypoints_from_export_map(
        &mut self,
        package: &CachedPackage,
        package_name: &str,
        exports: &Value,
    ) -> Result<Vec<ResolvedEntrypoint>, ResolutionError> {
        let mut entrypoints = Vec::new();
        match exports {
            Value::Array(elements) => {
                for element in elements {
                    self.load_entrypoints_from_target_exports(
                        package,
                        package_name,
                        ".",
                        &BTreeSet::new(),
                        BTreeSet::new(),
                        element,
                        &mut entrypoints,
                    )?;
                }
            }
            Value::Object(table) if is_subpaths(table) => {
                for (subpath, export) in json_object_entries_in_order(table) {
                    let subpath = subpath.to_string_lossy();
                    self.load_entrypoints_from_target_exports(
                        package,
                        package_name,
                        &subpath,
                        &BTreeSet::new(),
                        BTreeSet::new(),
                        export,
                        &mut entrypoints,
                    )?;
                }
            }
            _ => self.load_entrypoints_from_target_exports(
                package,
                package_name,
                ".",
                &BTreeSet::new(),
                BTreeSet::new(),
                exports,
                &mut entrypoints,
            )?,
        }
        Ok(entrypoints)
    }

    /// tsgo `loadEntrypointsFromTargetExports`: the entrypoints of one
    /// target of an `exports` map, reached under `include_conditions` and
    /// not under `exclude_conditions`.
    #[allow(clippy::too_many_arguments)]
    fn load_entrypoints_from_target_exports(
        &mut self,
        package: &CachedPackage,
        package_name: &str,
        subpath: &str,
        include_conditions: &BTreeSet<String>,
        mut exclude_conditions: BTreeSet<String>,
        exports: &Value,
        entrypoints: &mut Vec<ResolvedEntrypoint>,
    ) -> Result<(), ResolutionError> {
        match exports {
            Value::String(_) => {
                let target = exports.as_js().expect("a string").to_string_lossy();
                if !target.starts_with("./") {
                    return Ok(());
                }
                let ending = if target.ends_with('*') {
                    EntrypointEnding::ExtensionChangeable
                } else {
                    EntrypointEnding::Fixed
                };
                if target.contains('*') {
                    if target.find('*') != target.rfind('*') {
                        return Ok(());
                    }
                    let include_pattern = change_full_extension(
                        JsStr::from_str(&target.replacen('*', "**/*", 1)),
                        ".*",
                    );
                    let pattern_path = &target["./".len()..];
                    let (leading_slice, trailing_slice) =
                        pattern_path.split_once('*').expect("a star");
                    let package_directory = package.root.clone();
                    let files = CompilerConfigHost::new(self.host)
                        .read_directory(
                            package_directory.as_js(),
                            &ENTRYPOINT_EXTENSIONS,
                            None,
                            Some(&[include_pattern]),
                            None,
                        )
                        .map_err(read_directory_error)?;
                    for file in files {
                        let Some(relative_file) =
                            path_relative_to_directory(file.as_js(), package_directory.as_js())
                        else {
                            continue;
                        };
                        let relative_file = relative_file.to_string_lossy();
                        let Some(matched_star) = self.get_matched_star_for_pattern_entrypoint(
                            &relative_file,
                            leading_slice,
                            trailing_slice,
                        ) else {
                            continue;
                        };
                        let resolved_subpath = subpath.replacen('*', &matched_star, 1);
                        let resolved_subpath = resolved_subpath
                            .strip_prefix("./")
                            .unwrap_or(&resolved_subpath);
                        if resolved_subpath.is_empty() {
                            continue;
                        }
                        entrypoints.push(self.create_resolved_entrypoint_handling_symlink(
                            file.as_js(),
                            format!("{package_name}/{resolved_subpath}"),
                            include_conditions.clone(),
                            exclude_conditions.clone(),
                            ending,
                        )?);
                    }
                } else {
                    // tsgo `GetPathComponents(target)[2:]`: the components after `.`.
                    if target
                        .split('/')
                        .skip(1)
                        .any(|part| matches!(part, ".." | "." | "node_modules"))
                    {
                        return Ok(());
                    }
                    // tsgo `resolutionCandidateFromDirectoryPath`: a target
                    // naming a directory is no file.
                    if target.ends_with('/') {
                        return Ok(());
                    }
                    let candidate = normalize_path(
                        join_normalized(package.root.as_js(), JsStr::from_str(&target)).as_js(),
                    );
                    if let ResolutionOutcome::Resolved(resolved) = self
                        .probe_package_field_file_phase(
                            Some(package),
                            candidate.as_js(),
                            ExtensionProbePass::Preferred,
                            ENTRYPOINT_PROBE,
                        )?
                    {
                        let specifier = normalize_path(
                            crate::js_path::combine_paths(
                                JsStr::from_str(package_name),
                                JsStr::from_str(subpath),
                            )
                            .as_js(),
                        );
                        let path = resolved.resolved_file().display().to_owned();
                        entrypoints.push(self.create_resolved_entrypoint_handling_symlink(
                            path.as_js(),
                            specifier.to_string_lossy().into_owned(),
                            include_conditions.clone(),
                            exclude_conditions.clone(),
                            ending,
                        )?);
                    }
                }
            }
            Value::Array(elements) => {
                for element in elements {
                    self.load_entrypoints_from_target_exports(
                        package,
                        package_name,
                        subpath,
                        include_conditions,
                        exclude_conditions.clone(),
                        element,
                        entrypoints,
                    )?;
                }
            }
            Value::Object(table) => {
                let mut previous_conditions: Vec<String> = Vec::new();
                for (condition, export) in json_object_entries_in_order(table) {
                    let condition_text = condition.to_string_lossy().into_owned();
                    if exclude_conditions.contains(&condition_text) {
                        continue;
                    }
                    let condition_always_matches = condition == "default"
                        || condition == "types"
                        || is_applicable_versioned_types_key(condition);
                    let mut new_include_conditions = include_conditions.clone();
                    if !condition_always_matches {
                        new_include_conditions.insert(condition_text.clone());
                        // tsgo adds every earlier condition of this object to
                        // the excluded ones it keeps for the later ones.
                        exclude_conditions.extend(previous_conditions.iter().cloned());
                    }
                    previous_conditions.push(condition_text);
                    self.load_entrypoints_from_target_exports(
                        package,
                        package_name,
                        subpath,
                        &new_include_conditions,
                        exclude_conditions.clone(),
                        export,
                        entrypoints,
                    )?;
                    if condition_always_matches {
                        break;
                    }
                }
            }
            Value::Bool(_) | Value::Number(_) | Value::Null => {}
        }
        Ok(())
    }

    /// tsgo `getMatchedStarForPatternEntrypoint`: what the star of an
    /// `exports` pattern matches in a file's path in the package, also
    /// with the file's JavaScript extension.
    fn get_matched_star_for_pattern_entrypoint(
        &self,
        file: &str,
        leading_slice: &str,
        trailing_slice: &str,
    ) -> Option<String> {
        let case_sensitive = self.path_context.use_case_sensitive_file_names();
        let matched = |file: &str| {
            crate::has_prefix_and_suffix_without_overlap(
                JsStr::from_str(file),
                JsStr::from_str(leading_slice),
                JsStr::from_str(trailing_slice),
                case_sensitive,
            )
            .then(|| file[leading_slice.len()..file.len() - trailing_slice.len()].to_owned())
        };
        if let Some(star) = matched(file) {
            return Some(star);
        }
        let js_extension =
            crate::try_get_js_extension_for_file(JsStr::from_str(file), &self.options)?;
        let swapped = change_full_extension(JsStr::from_str(file), js_extension)
            .to_string_lossy()
            .into_owned();
        matched(&swapped)
    }
}

/// tsgo `ExportsOrImports.IsSubpaths`: an object with a key starting with
/// `.` and none starting otherwise than with `.` or `#`.
fn is_subpaths(table: &super::Map) -> bool {
    let (mut seen_dot, mut seen_hash, mut seen_other) = (false, false, false);
    for (key, _) in json_object_entries_in_order(table) {
        let Some(first) = key.code_units().next() else {
            continue;
        };
        seen_dot |= first == u16::from(b'.');
        seen_hash |= first == u16::from(b'#');
        seen_other |= first != u16::from(b'.') && first != u16::from(b'#');
        if seen_other && (seen_dot || seen_hash) {
            return false;
        }
    }
    seen_dot
}

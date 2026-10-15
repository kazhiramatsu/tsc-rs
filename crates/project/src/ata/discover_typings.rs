//! tsgo `discovertypings.go`: the typings a project's JavaScript files
//! need — those its config includes, its manifests' dependencies and the
//! packages installed without their own types, well-known script names,
//! React for JSX, and its unresolved imports — less those it excludes, with
//! the cached ones that are up to date.

use std::collections::{BTreeMap, BTreeSet};

use tsc_diagnostics::{JsStr, JsString};
use tsc_host::vfs::{FileSystem, VfsCompilerHost};
use tsc_program::ConfigParseHost;
use tsc_types::{Version, TYPESCRIPT_VERSION_MAJOR_MINOR};

use super::{CachedTyping, TypingsInfo};

/// The JavaScript extensions (tsgo `HasJSFileExtension`).
const JS_EXTENSIONS: [&str; 4] = [".js", ".jsx", ".mjs", ".cjs"];

/// tsgo `isTypingUpToDate`: the registry's version of the typing for this
/// TypeScript (else its latest) is not newer than the cached one.
pub(super) fn is_typing_up_to_date(
    cached_typing: &CachedTyping,
    available_typing_versions: &BTreeMap<String, String>,
) -> bool {
    let use_version = available_typing_versions
        .get(&format!("ts{TYPESCRIPT_VERSION_MAJOR_MINOR}"))
        .or_else(|| available_typing_versions.get("latest"));
    // tsgo's `MustParse` panics on a registry version it cannot read; such
    // a typing is not up to date here.
    use_version
        .and_then(|version| Version::parse(version))
        .is_some_and(|available| available <= cached_typing.version)
}

/// tsgo `DiscoverTypings`: the cached typings' paths, the typing names to
/// install and the files to watch for changes of the result.
pub fn discover_typings(
    fs: &dyn FileSystem,
    logger: &mut dyn FnMut(String),
    typings_info: &TypingsInfo,
    file_names: &[String],
    project_root_path: &str,
    package_name_to_typing_location: &BTreeMap<String, CachedTyping>,
    types_registry: &BTreeMap<String, BTreeMap<String, String>>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    // A typing name to its file (empty for one to install).
    let mut inferred_typings = BTreeMap::<String, String>::new();
    // Only infer typings for JavaScript files.
    let file_names = file_names
        .iter()
        .filter(|file_name| has_js_file_extension(file_name))
        .collect::<Vec<_>>();
    let type_acquisition = &typings_info.type_acquisition;
    if let Some(include) = &type_acquisition.include {
        add_inferred_typings(
            logger,
            &mut inferred_typings,
            include,
            "Explicitly included types",
        );
    }
    let mut files_to_watch = Vec::new();
    // The directories of the manifests and module folders.
    if !typings_info.has_types_option {
        let mut possible_search_dirs = file_names
            .iter()
            .map(|file_name| directory_path(file_name))
            .collect::<BTreeSet<_>>();
        possible_search_dirs.insert(project_root_path.to_owned());
        for search_dir in &possible_search_dirs {
            for (manifest_name, modules_dir_name) in [
                ("bower.json", "bower_components"),
                ("package.json", "node_modules"),
            ] {
                add_typing_names_and_get_files_to_watch(
                    fs,
                    logger,
                    &mut inferred_typings,
                    &mut files_to_watch,
                    search_dir,
                    manifest_name,
                    modules_dir_name,
                );
            }
        }
    }
    if type_acquisition.disable_filename_based_type_acquisition != Some(true) {
        get_typing_names_from_source_file_names(logger, &mut inferred_typings, &file_names);
    }
    // The typings of the unresolved imports.
    let modules = typings_info
        .unresolved_imports
        .iter()
        .flatten()
        .map(|module| non_relative_module_name_for_typing_cache(module))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    add_inferred_typings(
        logger,
        &mut inferred_typings,
        &modules,
        "Inferred typings from unresolved imports",
    );
    // Less those the config excludes.
    for exclude_typing_name in type_acquisition.exclude.iter().flatten() {
        inferred_typings.remove(exclude_typing_name);
        logger(format!(
            "ATA:: Typing for {exclude_typing_name} is in exclude list, will be ignored."
        ));
    }
    // The cached typings that are up to date (tsgo also takes those it did
    // not infer).
    for (name, typing) in package_name_to_typing_location {
        let installed = inferred_typings
            .get(name)
            .is_some_and(|location| !location.is_empty());
        if !installed
            && types_registry
                .get(name)
                .is_some_and(|entry| is_typing_up_to_date(typing, entry))
        {
            inferred_typings.insert(name.clone(), typing.typings_location.clone());
        }
    }
    let mut cached_typing_paths = Vec::new();
    let mut new_typing_names = Vec::new();
    for (typing, inferred) in inferred_typings {
        if inferred.is_empty() {
            new_typing_names.push(typing);
        } else {
            cached_typing_paths.push(inferred);
        }
    }
    logger(format!(
        "ATA:: Finished typings discovery: cachedTypingsPaths: {} newTypingNames: {}, filesToWatch {}",
        go_list(&cached_typing_paths),
        go_list(&new_typing_names),
        go_list(&files_to_watch)
    ));
    (cached_typing_paths, new_typing_names, files_to_watch)
}

/// Go's `%v` of a string slice.
pub(super) fn go_list(items: &[String]) -> String {
    format!("[{}]", items.join(" "))
}

fn has_js_file_extension(file_name: &str) -> bool {
    JS_EXTENSIONS
        .iter()
        .any(|extension| file_name.ends_with(extension))
}

/// tsgo `RootedFilePath.Directory`.
fn directory_path(file_name: &str) -> String {
    match file_name.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => file_name[..index].to_owned(),
        None => file_name.to_owned(),
    }
}

fn resolve(directory: &str, name: &str) -> String {
    if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

/// tsgo `core.NonRelativeModuleNameForTypingCache`: a Node core module's
/// typings are Node's.
fn non_relative_module_name_for_typing_cache(module_name: &str) -> String {
    if tsc_checker::modules::is_node_core_module(JsStr::from_str(module_name)) {
        "node".to_owned()
    } else {
        module_name.to_owned()
    }
}

/// tsgo `addInferredTyping`.
fn add_inferred_typing(inferred_typings: &mut BTreeMap<String, String>, typing_name: &str) {
    inferred_typings.entry(typing_name.to_owned()).or_default();
}

/// tsgo `addInferredTypings`.
fn add_inferred_typings(
    logger: &mut dyn FnMut(String),
    inferred_typings: &mut BTreeMap<String, String>,
    typing_names: &[String],
    message: &str,
) {
    logger(format!("ATA:: {message}: {}", go_list(typing_names)));
    for typing_name in typing_names {
        add_inferred_typing(inferred_typings, typing_name);
    }
}

/// tsgo `getTypingNamesFromSourceFileNames`: the typings of well-known
/// script names (`jquery-min.2.3.4.js` is jQuery's), and React's for JSX.
fn get_typing_names_from_source_file_names(
    logger: &mut dyn FnMut(String),
    inferred_typings: &mut BTreeMap<String, String>,
    file_names: &[&String],
) {
    let mut has_jsx_file = false;
    let mut from_file_names = Vec::new();
    for file_name in file_names {
        has_jsx_file = has_jsx_file || file_name.ends_with(".jsx");
        let base_name = &file_name[file_name.rfind('/').map_or(0, |index| index + 1)..];
        let inferred_typing_name = tsc_program::remove_file_extension(JsStr::from_str(
            &tsc_host::to_file_name_lower_case(base_name),
        ))
        .to_string_lossy()
        .into_owned();
        let cleaned_typing_name = remove_min_and_version_numbers(&inferred_typing_name);
        if let Some(type_name) = super::types_map::type_name(cleaned_typing_name) {
            from_file_names.push(type_name.to_owned());
        }
    }
    if !from_file_names.is_empty() {
        add_inferred_typings(
            logger,
            inferred_typings,
            &from_file_names,
            "Inferred typings from file names",
        );
    }
    if has_jsx_file {
        logger("ATA:: Inferred 'react' typings due to presence of '.jsx' extension".to_owned());
        add_inferred_typing(inferred_typings, "react");
    }
}

/// tsgo `addTypingNamesAndGetFilesToWatch`: the typings of a manifest's
/// dependencies (`package.json`, `bower.json`) and of the installed
/// packages without their own types; the manifest and the modules folder
/// are watched.
fn add_typing_names_and_get_files_to_watch(
    fs: &dyn FileSystem,
    logger: &mut dyn FnMut(String),
    inferred_typings: &mut BTreeMap<String, String>,
    files_to_watch: &mut Vec<String>,
    project_root_path: &str,
    manifest_name: &str,
    modules_dir_name: &str,
) {
    let manifest_path = resolve(project_root_path, manifest_name);
    let mut manifest_typing_names = Vec::new();
    if let Some(contents) = read_file(fs, &manifest_path) {
        files_to_watch.push(manifest_path.clone());
        if let Ok(serde_json::Value::Object(manifest)) =
            serde_json::from_str::<serde_json::Value>(&contents)
        {
            for field in [
                "dependencies",
                "devDependencies",
                "optionalDependencies",
                "peerDependencies",
            ] {
                if let Some(serde_json::Value::Object(dependencies)) = manifest.get(field) {
                    manifest_typing_names.extend(dependencies.keys().cloned());
                }
            }
            add_inferred_typings(
                logger,
                inferred_typings,
                &manifest_typing_names,
                &format!("Typing names in '{manifest_path}' dependencies"),
            );
        }
    }
    let packages_folder_path = resolve(project_root_path, modules_dir_name);
    files_to_watch.push(packages_folder_path.clone());
    if !fs.is_dir(&packages_folder_path) {
        return;
    }
    let dependency_manifest_names = if !manifest_typing_names.is_empty() {
        manifest_typing_names
            .iter()
            .map(|typing_name| {
                resolve(
                    &packages_folder_path,
                    &format!("{typing_name}/{manifest_name}"),
                )
            })
            .collect::<Vec<_>>()
    } else {
        // The manifests of the folder's packages (`node_modules/@foo/bar`
        // for a scoped one).
        let host = VfsCompilerHost::new(fs, "/".to_owned());
        let manifests = tsc_program::CompilerConfigHost::new(&host)
            .read_directory(
                JsStr::from_str(&packages_folder_path),
                &[".json"],
                None,
                None,
                Some(3),
            )
            .unwrap_or_default();
        manifests
            .iter()
            .map(JsString::to_string_lossy)
            .filter_map(|manifest_path| {
                let components = manifest_path.split('/').collect::<Vec<_>>();
                let count = components.len();
                if components[count - 1] != manifest_name || count < 4 {
                    return None;
                }
                let is_scoped = components[count - 3].starts_with('@');
                let folder = if is_scoped {
                    components[count - 4]
                } else {
                    components[count - 3]
                };
                (tsc_host::to_file_name_lower_case(folder) == modules_dir_name)
                    .then(|| manifest_path.into_owned())
            })
            .collect()
    };
    logger(format!(
        "ATA:: Searching for typing names in {packages_folder_path}; all files: {}",
        go_list(&dependency_manifest_names)
    ));
    // Each package's own types, else its name to look up.
    let mut package_names = Vec::new();
    for manifest_path in &dependency_manifest_names {
        let Some(contents) = read_file(fs, manifest_path) else {
            continue;
        };
        let Ok(serde_json::Value::Object(manifest)) =
            serde_json::from_str::<serde_json::Value>(&contents)
        else {
            continue;
        };
        let string_field = |name: &str| match manifest.get(name) {
            Some(serde_json::Value::String(value)) => value.as_str(),
            _ => "",
        };
        let name = string_field("name");
        if name.is_empty() {
            continue;
        }
        let own_types = match string_field("types") {
            "" => string_field("typings"),
            types => types,
        };
        if own_types.is_empty() {
            package_names.push(name.to_owned());
            continue;
        }
        let absolute_path = tsc_program::get_normalized_absolute_path(
            JsStr::from_str(own_types),
            JsStr::from_str(&directory_path(manifest_path)),
        )
        .to_string_lossy()
        .into_owned();
        if fs.is_file(&absolute_path) {
            logger(format!(
                "ATA::     Package '{name}' provides its own types."
            ));
            inferred_typings.insert(name.to_owned(), absolute_path);
        } else {
            logger(format!(
                "ATA::     Package '{name}' provides its own types but they are missing."
            ));
        }
    }
    add_inferred_typings(
        logger,
        inferred_typings,
        &package_names,
        "    Found package names",
    );
}

fn read_file(fs: &dyn FileSystem, path: &str) -> Option<String> {
    fs.read(path)
        .ok()
        .and_then(|bytes| tsc_program::decode_host_text(bytes).ok())
}

/// tsgo `removeMinAndVersionNumbers`: `jquery-min.4.2.3` is `jquery` (the
/// `.min`/`-min` and `.N`/`-N` suffixes, scanned from the end).
pub(super) fn remove_min_and_version_numbers(file_name: &str) -> &str {
    // The character before `position` and its length (Go's
    // `DecodeLastRuneInString`; none at the start).
    let last = |position: usize| {
        file_name[..position]
            .chars()
            .next_back()
            .map_or(('\u{fffd}', 0), |character| {
                (character, character.len_utf8())
            })
    };
    let mut end = file_name.len();
    let mut position = end;
    while position > 0 {
        let (mut character, mut size) = last(position);
        if character.is_ascii_digit() {
            // A `\d+` segment.
            loop {
                position -= size;
                (character, size) = last(position);
                if position == 0 || !character.is_ascii_digit() {
                    break;
                }
            }
        } else if position > 4 && matches!(character, 'n' | 'N') {
            // `min`, the `n` matched.
            position -= size;
            (character, size) = last(position);
            if !matches!(character, 'i' | 'I') {
                break;
            }
            position -= size;
            (character, size) = last(position);
            if !matches!(character, 'm' | 'M') {
                break;
            }
            position -= size;
            (character, size) = last(position);
        } else {
            // Not part of either suffix.
            break;
        }
        if !matches!(character, '-' | '.') {
            break;
        }
        position -= size;
        end = position;
    }
    &file_name[..end]
}

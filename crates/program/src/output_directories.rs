//! Pure output-directory facts shared by Program diagnostics and emission.
//! No host I/O or checker state is retained here.

use tsc_diagnostics::{JsStr, JsString};
use tsc_types::CompilerOptions;

use crate::js_path::{directory_name, file_name_key, normalize_slashes, root_parts};
use crate::module_requests::is_declaration_file_name;
use crate::module_resolution::normalize_absolute_js_path_lexical;

fn normalized(path: JsStr<'_>, current_directory: JsStr<'_>) -> JsString {
    let path = if path.is_empty() {
        current_directory
    } else {
        path
    };
    normalize_absolute_js_path_lexical(path, Some(current_directory))
        .expect("output paths use JS options and an absolute Program current directory")
}

fn canonical(text: JsStr<'_>, case_sensitive: bool) -> JsString {
    file_name_key(text, case_sensitive)
}

/// Canonical comparison only; callback paths retain their requested spelling.
pub fn canonical_emit_path(
    path: JsStr<'_>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    canonical(normalized(path, current_directory).as_js(), case_sensitive)
}

fn components(path: JsStr<'_>) -> Vec<JsString> {
    let (root, tail) = root_parts(path).expect("normalized Program path is rooted");
    std::iter::once(root.to_owned())
        .chain(
            tail.split_ascii(b'/')
                .filter(|part| !part.is_empty())
                .map(JsStr::to_owned),
        )
        .collect()
}

/// tspath.reducePathComponents (tsgo): empty and `.` components are dropped
/// and `..` climbs the preceding component unless it is the root or another
/// `..` (GetPathComponentsRelativeTo reduces both sides, so the relative
/// path of `node_modules/@types/./css/package.json` names `@types/css`).
fn reduce_path_components(components: Vec<JsString>) -> Vec<JsString> {
    let mut reduced: Vec<JsString> = Vec::with_capacity(components.len());
    let mut components = components.into_iter();
    let Some(root) = components.next() else {
        return reduced;
    };
    reduced.push(root);
    for component in components {
        if component.is_empty() || component.as_str() == Some(".") {
            continue;
        }
        if component.as_str() == Some("..") {
            if reduced.len() > 1 {
                if reduced[reduced.len() - 1].as_str() != Some("..") {
                    reduced.pop();
                    continue;
                }
            } else if !reduced[0].is_empty() {
                continue;
            }
        }
        reduced.push(component);
    }
    reduced
}

fn from_components(parts: &[JsString]) -> JsString {
    let Some(root) = parts.first() else {
        return JsString::new();
    };
    let mut result = root.clone();
    if !result.is_empty() && !result.ends_with("/") {
        result.push('/');
    }
    result.push_js(join_components(&parts[1..]).as_js());
    result
}

fn join_components(parts: &[JsString]) -> JsString {
    let mut result = JsString::new();
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            result.push('/');
        }
        result.push_js(part.as_js());
    }
    result
}

/// tsc-port: computeCommonSourceDirectoryOfFilenames @6.0.3
/// tsc-hash: 900713690ffd0a6a34f2b76b227d6c7b71a8c2f505b247c142dd946bfb31196b
/// tsc-span: _tsc.js:121909-121939
pub fn inferred_common_source_directory(
    source_files: &[JsStr<'_>],
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let mut common: Option<Vec<JsString>> = None;
    for source in source_files {
        let mut directory = components(normalized(*source, current_directory).as_js());
        directory.pop();
        if let Some(common) = &mut common {
            let shared = common
                .iter()
                .zip(&directory)
                .take_while(|(left, right)| {
                    canonical(left.as_js(), case_sensitive)
                        == canonical(right.as_js(), case_sensitive)
                })
                .count();
            if shared == 0 {
                return JsString::new();
            }
            common.truncate(shared);
        } else {
            common = Some(directory);
        }
    }
    common.map_or_else(
        || current_directory.to_owned(),
        |common| from_components(&common),
    )
}

/// tsc-port: getCommonSourceDirectory @6.0.3
/// tsc-hash: 6213cf3653b969239fea4dd1852031f25b46c5ed630b923fe3a7ca0b55d9c930
/// tsc-span: _tsc.js:116460-116475
pub fn common_source_directory(
    options: &CompilerOptions,
    config_file: Option<JsStr<'_>>,
    source_files: &[JsStr<'_>],
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let directory = if let Some(root) = options
        .root_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|root| !root.is_empty())
    {
        normalized(root, current_directory)
    } else if let Some(config) = config_file {
        directory_name(config)
    } else {
        inferred_common_source_directory(source_files, current_directory, case_sensitive)
    };
    let mut directory = directory;
    if !directory.is_empty() && !directory.ends_with("/") {
        directory.push('/');
    }
    directory
}

/// tsc-port: getSourceFilePathInNewDirWorker @6.0.3
/// tsc-hash: 1c92b6269af48b15f457c7c5e64b620cc19f367cb7e0a38b07b74b05b4940203
/// tsc-span: _tsc.js:16638-16643
pub fn source_file_path_in_new_directory(
    source: JsStr<'_>,
    directory: JsStr<'_>,
    common: JsStr<'_>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let source = normalized(source, current_directory);
    let relative = if canonical(source.as_js(), case_sensitive)
        .as_js()
        .starts_with_js(canonical(common, case_sensitive).as_js())
    {
        // TypeScript slices by the original common directory's UTF-16 length,
        // including boundaries inside a pair after case-fold expansion.
        source
            .as_js()
            .substring(common.len_units(), source.len_units())
    } else {
        source
    };
    if directory.is_empty() || root_parts(relative.as_js()).is_some() {
        relative
    } else {
        let mut directory = normalize_slashes(directory);
        if !directory.ends_with("/") {
            directory.push('/');
        }
        directory.push_js(relative.as_js());
        directory
    }
}

/// tspath.GetRelativePathFromDirectory (tsgo): the path of `to` relative to
/// `from_directory`, both absolute; `..` for each component of `from` past
/// the shared prefix, which is compared case-insensitively when file names
/// are.
/// tsgo tspath.GetRelativePathFromDirectory: `to` relative to
/// `from_directory` (`..` per unshared component of the directory), or `to`
/// itself when the two share no root.
pub fn relative_path_from_directory(
    from_directory: JsStr<'_>,
    to: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let from = reduce_path_components(components(from_directory));
    let to = reduce_path_components(components(to));
    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| {
            canonical(left.as_js(), case_sensitive) == canonical(right.as_js(), case_sensitive)
        })
        .count();
    if shared == 0 {
        return to
            .last()
            .map_or_else(JsString::new, |_| from_components(&to));
    }
    let mut parts: Vec<JsString> = Vec::with_capacity(from.len() + to.len());
    // The root is the first component of both; everything past the shared
    // prefix of `from` is climbed.
    parts.push(JsString::from(""));
    for _ in shared..from.len() {
        parts.push(JsString::from(".."));
    }
    parts.extend(to[shared..].iter().cloned());
    join_components(&parts[1..])
}

/// outputpaths.getOutputPathWithoutChangingExtension (tsgo): the input's
/// path under `output_directory`, by its path relative to the common source
/// directory; the input itself without an output directory.
fn output_path_without_changing_extension(
    input: JsStr<'_>,
    output_directory: Option<JsStr<'_>>,
    common: JsStr<'_>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let input = normalized(input, current_directory);
    let Some(output_directory) = output_directory.filter(|directory| !directory.is_empty()) else {
        return input;
    };
    let common = normalized(common, current_directory);
    let relative = relative_path_from_directory(common.as_js(), input.as_js(), case_sensitive);
    let output_directory = normalized(output_directory, current_directory);
    normalized(
        crate::js_path::combine_paths(output_directory.as_js(), relative.as_js()).as_js(),
        current_directory,
    )
}

fn lower_ascii(path: JsStr<'_>) -> JsString {
    path.code_units()
        .map(|unit| {
            if (0x41..=0x5a).contains(&unit) {
                unit + 0x20
            } else {
                unit
            }
        })
        .collect()
}

/// outputpaths.GetOutputExtension (tsgo): JSON stays JSON, `.jsx`/`.tsx`
/// keep `.jsx` under `jsx: preserve`, the module-flavored extensions keep
/// their flavor and everything else emits `.js`.
pub fn output_extension(file_name: JsStr<'_>, jsx: Option<i32>) -> &'static str {
    const JSX_PRESERVE: i32 = 1;
    let lower = lower_ascii(file_name);
    if lower.ends_with(".json") {
        ".json"
    } else if jsx == Some(JSX_PRESERVE) && (lower.ends_with(".jsx") || lower.ends_with(".tsx")) {
        ".jsx"
    } else if lower.ends_with(".mts") || lower.ends_with(".mjs") {
        ".mjs"
    } else if lower.ends_with(".cts") || lower.ends_with(".cjs") {
        ".cjs"
    } else {
        ".js"
    }
}

/// outputpaths.GetOutputJSFileName (tsgo): the JavaScript file a project
/// emits for `input`; `None` under `emitDeclarationOnly` and for a JSON
/// file whose output would be the file itself.
pub fn output_js_file_name(
    input: JsStr<'_>,
    options: &CompilerOptions,
    common: JsStr<'_>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> Option<JsString> {
    if options.emit_declaration_only == Some(true) {
        return None;
    }
    let out_dir = options
        .out_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty());
    let mut output = remove_file_extension(
        output_path_without_changing_extension(
            input,
            out_dir,
            common,
            current_directory,
            case_sensitive,
        )
        .as_js(),
    );
    output.push_str(output_extension(input, options.jsx));
    if output.ends_with(".json")
        && canonical_emit_path(input, current_directory, case_sensitive)
            == canonical_emit_path(output.as_js(), current_directory, case_sensitive)
    {
        return None;
    }
    Some(output)
}

/// outputpaths.GetSourceMapFilePath (tsgo): the map of a JavaScript file
/// under `sourceMap` without `inlineSourceMap`.
pub fn source_map_file_path(js_file: JsStr<'_>, options: &CompilerOptions) -> Option<JsString> {
    (options.source_map == Some(true) && options.inline_source_map != Some(true)).then(|| {
        let mut map = js_file.to_owned();
        map.push_str(".map");
        map
    })
}

/// tsoptions `ParsedCommandLine.GetOutputFileNames` (tsgo): every output
/// the project's file names produce, in file order: the JavaScript file and
/// its source map, then the declaration file and its map. A declaration
/// file produces nothing; a JSON file produces its copy alone.
pub fn output_file_names(
    options: &CompilerOptions,
    config_file: Option<JsStr<'_>>,
    file_names: &[JsString],
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> Vec<JsString> {
    // tsoptions ParsedCommandLine.CommonSourceDirectory: `rootDir`, else
    // the config's directory, else inferred from the files that are not
    // declaration files (nor JavaScript files under noEmitForJsFiles).
    let composite = options.composite == Some(true);
    let no_emit_for_js = options.no_emit_for_js_files == Some(true);
    let emitted: Vec<JsStr<'_>> = file_names
        .iter()
        .map(JsString::as_js)
        .filter(|name| {
            if is_declaration_file_name(*name) {
                return false;
            }
            let lower = lower_ascii(*name);
            let javascript = [".js", ".jsx", ".mjs", ".cjs"]
                .iter()
                .any(|extension| lower.ends_with(extension));
            !no_emit_for_js || !javascript
        })
        .collect();
    let common = common_source_directory(
        options,
        config_file,
        &emitted,
        current_directory,
        case_sensitive,
    );
    let declarations = options.declaration == Some(true) || composite;
    let declaration_maps = declarations && options.declaration_map == Some(true);
    let mut outputs = Vec::new();
    for file_name in file_names {
        let file_name = file_name.as_js();
        if is_declaration_file_name(file_name) {
            continue;
        }
        let is_json = lower_ascii(file_name).ends_with(".json");
        if let Some(js) = output_js_file_name(
            file_name,
            options,
            common.as_js(),
            current_directory,
            case_sensitive,
        ) {
            let map = (!is_json)
                .then(|| source_map_file_path(js.as_js(), options))
                .flatten();
            outputs.push(js);
            outputs.extend(map);
        }
        if is_json {
            continue;
        }
        if declarations {
            let declaration = output_declaration_file_name(
                file_name,
                options,
                common.as_js(),
                current_directory,
                case_sensitive,
            );
            let map = declaration_maps.then(|| {
                let mut map = declaration.clone();
                map.push_str(".map");
                map
            });
            outputs.push(declaration);
            outputs.extend(map);
        }
    }
    outputs
}

/// outputpaths.ChangeToDeclarationExtension (tsgo): `.d.mts`/`.d.cts` for
/// the module-flavored extensions, `.d.json.ts` for JSON, `.d.ts` otherwise.
fn change_to_declaration_extension(path: JsStr<'_>) -> JsString {
    let lower = lower_ascii(path);
    let extension = if lower.ends_with(".mts") || lower.ends_with(".mjs") {
        ".d.mts"
    } else if lower.ends_with(".cts") || lower.ends_with(".cjs") {
        ".d.cts"
    } else if lower.ends_with(".json") {
        ".d.json.ts"
    } else {
        ".d.ts"
    };
    let mut without_extension = remove_file_extension(path);
    without_extension.push_str(extension);
    without_extension
}

/// tspath.RemoveFileExtension: the path without its last extension (any
/// extension when the last is not a TypeScript-known one).
fn remove_file_extension(path: JsStr<'_>) -> JsString {
    let name = base_name(path);
    let Some(dot) = name
        .as_js()
        .as_bytes()
        .iter()
        .rposition(|byte| *byte == b'.')
    else {
        return path.to_owned();
    };
    if dot == 0 {
        return path.to_owned();
    }
    let cut = path
        .len_units()
        .saturating_sub(name.len_units().saturating_sub(dot));
    path.substring(0, cut).to_owned()
}

fn base_name(path: JsStr<'_>) -> JsString {
    path.rsplit_once("/")
        .map_or_else(|| path.to_owned(), |(_, name)| name.to_owned())
}

/// outputpaths.GetOutputDeclarationFileNameWorker (tsgo): the declaration
/// file a project emits for `input` (under `declarationDir`, else `outDir`,
/// else beside the input), given the project's common source directory.
pub fn output_declaration_file_name(
    input: JsStr<'_>,
    options: &CompilerOptions,
    common: JsStr<'_>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let directory = options
        .declaration_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty())
        .or(options
            .out_dir
            .as_ref()
            .map(JsString::as_js)
            .filter(|directory| !directory.is_empty()));
    change_to_declaration_extension(
        output_path_without_changing_extension(
            input,
            directory,
            common,
            current_directory,
            case_sensitive,
        )
        .as_js(),
    )
}

/// outputpaths.GetBuildInfoFileName (tsgo): the `.tsbuildinfo` an
/// incremental or composite project writes: `tsBuildInfoFile`, else the
/// config's name under `outDir` (relative to `rootDir` when both are set),
/// else beside the config. `None` for a project that writes none.
pub fn build_info_file_name(
    options: &CompilerOptions,
    config_file_path: Option<JsStr<'_>>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> Option<JsString> {
    if options.incremental != Some(true) && options.composite != Some(true) {
        return None;
    }
    build_info_file_name_in_build_mode(options, config_file_path, current_directory, case_sensitive)
}

/// outputpaths.GetBuildInfoFileName under `tsc -b` (tsgo
/// `CompilerOptions.Build`): every project of a build writes a build info,
/// whether or not it is incremental.
pub fn build_info_file_name_in_build_mode(
    options: &CompilerOptions,
    config_file_path: Option<JsStr<'_>>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> Option<JsString> {
    if let Some(file) = options
        .ts_build_info_file
        .as_ref()
        .map(JsString::as_js)
        .filter(|file| !file.is_empty())
    {
        return Some(normalized(file, current_directory));
    }
    let config = normalized(config_file_path?, current_directory);
    let config_extension_less = remove_file_extension(config.as_js());
    let out_dir = options
        .out_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty());
    let root_dir = options
        .root_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty());
    let mut extension_less = match (out_dir, root_dir) {
        (Some(out_dir), Some(root_dir)) => {
            let out_dir = normalized(out_dir, current_directory);
            let root_dir = normalized(root_dir, current_directory);
            let relative = relative_path_from_directory(
                root_dir.as_js(),
                config_extension_less.as_js(),
                case_sensitive,
            );
            normalized(
                crate::js_path::combine_paths(out_dir.as_js(), relative.as_js()).as_js(),
                current_directory,
            )
        }
        (Some(out_dir), None) => {
            let out_dir = normalized(out_dir, current_directory);
            crate::js_path::combine_paths(
                out_dir.as_js(),
                base_name(config_extension_less.as_js()).as_js(),
            )
        }
        (None, _) => config_extension_less,
    };
    extension_less.push_str(".tsbuildinfo");
    Some(extension_less)
}

/// tsc-port: sourceFileMayBeEmitted @6.0.3
/// tsc-hash: 333fcd249758d38eb80146910286d7cabdbbf6f1ea0787f8f1a2c85e9535ecb2
/// tsc-span: _tsc.js:16617-16634
pub fn source_file_may_be_emitted_for_options(
    source: JsStr<'_>,
    eligible: bool,
    options: &CompilerOptions,
    config_file: Option<JsStr<'_>>,
    current_directory: JsStr<'_>,
    case_sensitive: bool,
) -> bool {
    let name = source;
    if !eligible || is_declaration_file_name(name) {
        return false;
    }
    let name: JsString = name
        .code_units()
        .map(|unit| {
            if (0x41..=0x5a).contains(&unit) {
                unit + 0x20
            } else {
                unit
            }
        })
        .collect();
    if options.no_emit_for_js_files == Some(true)
        && [".js", ".jsx", ".mjs", ".cjs", ".json"]
            .iter()
            .any(|extension| name.ends_with(extension))
    {
        return false;
    }
    if !name.ends_with(".json") {
        return true;
    }
    if options
        .out_file
        .as_ref()
        .map(JsString::as_js)
        .is_some_and(|file| !file.is_empty())
    {
        return true;
    }
    let Some(out_dir) = options
        .out_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty())
    else {
        return false;
    };
    if options
        .root_dir
        .as_ref()
        .map(JsString::as_js)
        .is_some_and(|root| !root.is_empty())
        || config_file.is_some()
    {
        let common =
            common_source_directory(options, config_file, &[], current_directory, case_sensitive);
        let common = normalized(common.as_js(), current_directory);
        let output = source_file_path_in_new_directory(
            source,
            out_dir,
            common.as_js(),
            current_directory,
            case_sensitive,
        );
        if canonical_emit_path(source, current_directory, case_sensitive)
            == canonical_emit_path(output.as_js(), current_directory, case_sensitive)
        {
            return false;
        }
    }
    true
}

pub(crate) fn directory_relative_to_config(
    config: JsStr<'_>,
    directory: JsStr<'_>,
    case_sensitive: bool,
) -> JsString {
    let mut from = components(config);
    from.pop();
    let to = components(directory);
    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| {
            canonical(left.as_js(), case_sensitive) == canonical(right.as_js(), case_sensitive)
        })
        .count();
    if shared == 0 {
        return directory.to_owned();
    }
    let mut parts = vec![JsString::from(".."); from.len() - shared];
    parts.extend_from_slice(&to[shared..]);
    let relative = join_components(&parts);
    if relative == ".." || relative.starts_with("../") {
        relative
    } else {
        let mut path = JsString::from("./");
        path.push_js(relative.as_js());
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(set: impl FnOnce(&mut CompilerOptions)) -> CompilerOptions {
        let mut options = CompilerOptions::default();
        set(&mut options);
        options
    }

    fn names(files: &[&str]) -> Vec<JsString> {
        files.iter().map(|file| JsString::from(*file)).collect()
    }

    fn outputs(options: &CompilerOptions, config: &str, files: &[&str]) -> Vec<String> {
        output_file_names(
            options,
            Some(config.into()),
            &names(files),
            "/p".into(),
            true,
        )
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
    }

    /// tsgo `GetOutputFileNames` (tsoptions/parsedcommandline.go): the
    /// JavaScript file and its map, then the declaration file and its map;
    /// a declaration file produces nothing, a JSON file its copy alone.
    #[test]
    fn output_file_names_follow_tsgo() {
        let composite = options(|options| options.composite = Some(true));
        assert_eq!(
            outputs(
                &composite,
                "/p/core/tsconfig.json",
                &[
                    "/p/core/src/a.ts",
                    "/p/core/src/b.ts",
                    "/p/core/lib/lib.d.ts"
                ]
            ),
            [
                "/p/core/src/a.js",
                "/p/core/src/a.d.ts",
                "/p/core/src/b.js",
                "/p/core/src/b.d.ts"
            ]
        );
        let maps = options(|options| {
            options.declaration = Some(true);
            options.declaration_map = Some(true);
            options.source_map = Some(true);
            options.out_dir = Some("/p/app/out".into());
            options.root_dir = Some("/p/app/src".into());
        });
        assert_eq!(
            outputs(&maps, "/p/app/tsconfig.json", &["/p/app/src/main.ts"]),
            [
                "/p/app/out/main.js",
                "/p/app/out/main.js.map",
                "/p/app/out/main.d.ts",
                "/p/app/out/main.d.ts.map"
            ]
        );
        let inline = options(|options| {
            options.source_map = Some(true);
            options.inline_source_map = Some(true);
        });
        assert_eq!(
            outputs(&inline, "/p/tsconfig.json", &["/p/a.ts"]),
            ["/p/a.js"]
        );
        let declarations_only = options(|options| {
            options.declaration = Some(true);
            options.emit_declaration_only = Some(true);
        });
        assert_eq!(
            outputs(&declarations_only, "/p/tsconfig.json", &["/p/a.ts"]),
            ["/p/a.d.ts"]
        );
        // TypeScript 7.1: without `rootDir` the config's directory is the
        // common source directory.
        let json = options(|options| {
            options.declaration = Some(true);
            options.out_dir = Some("/p/out".into());
        });
        assert_eq!(
            outputs(
                &json,
                "/p/tsconfig.json",
                &["/p/src/a.ts", "/p/src/data.json"]
            ),
            [
                "/p/out/src/a.js",
                "/p/out/src/a.d.ts",
                "/p/out/src/data.json"
            ]
        );
        // A JSON file whose output is itself produces nothing.
        assert_eq!(
            outputs(
                &CompilerOptions::default(),
                "/p/tsconfig.json",
                &["/p/data.json"]
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn output_extensions_follow_tsgo() {
        assert_eq!(output_extension("/p/a.ts".into(), None), ".js");
        assert_eq!(output_extension("/p/a.tsx".into(), None), ".js");
        assert_eq!(output_extension("/p/a.tsx".into(), Some(1)), ".jsx");
        assert_eq!(output_extension("/p/a.jsx".into(), Some(1)), ".jsx");
        assert_eq!(output_extension("/p/a.mts".into(), None), ".mjs");
        assert_eq!(output_extension("/p/a.cjs".into(), None), ".cjs");
        assert_eq!(output_extension("/p/a.json".into(), None), ".json");
    }

    /// Every project of a build writes a build info; the command's
    /// (`build_info_file_name`) only an incremental one.
    #[test]
    fn build_info_names_in_build_mode() {
        let plain = CompilerOptions::default();
        assert_eq!(
            build_info_file_name(&plain, Some("/p/tsconfig.json".into()), "/p".into(), true),
            None
        );
        assert_eq!(
            build_info_file_name_in_build_mode(
                &plain,
                Some("/p/tsconfig.json".into()),
                "/p".into(),
                true
            )
            .map(|name| name.to_string_lossy().into_owned()),
            Some("/p/tsconfig.tsbuildinfo".to_owned())
        );
        let out = options(|options| options.out_dir = Some("/p/dist".into()));
        assert_eq!(
            build_info_file_name_in_build_mode(
                &out,
                Some("/p/tsconfig.json".into()),
                "/p".into(),
                true
            )
            .map(|name| name.to_string_lossy().into_owned()),
            Some("/p/dist/tsconfig.tsbuildinfo".to_owned())
        );
    }
}

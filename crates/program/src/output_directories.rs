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

//! Pure output-directory facts shared by Program diagnostics and emission.
//! No host I/O or checker state is retained here.

use std::path::{Path, PathBuf};

use tsc_host::to_file_name_lower_case;
use tsc_types::CompilerOptions;

use crate::module_requests::is_declaration_file_name;
use crate::module_resolution::{
    directory_name, normalize_absolute_path_lexical, normalized_root_parts,
};

fn normalized(path: &Path, current_directory: &Path) -> String {
    let path = if path.as_os_str().is_empty() {
        current_directory
    } else {
        path
    };
    normalize_absolute_path_lexical(path, Some(&current_directory.to_string_lossy()))
        .expect("output paths use Unicode options and an absolute Program current directory")
}

fn canonical(text: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        text.to_owned()
    } else {
        to_file_name_lower_case(text)
    }
}

/// Canonical comparison only; callback paths retain their requested spelling.
pub fn canonical_emit_path(path: &Path, current_directory: &Path, case_sensitive: bool) -> PathBuf {
    PathBuf::from(canonical(
        &normalized(path, current_directory),
        case_sensitive,
    ))
}

fn components(path: &str) -> Vec<String> {
    let (root, tail) = normalized_root_parts(path).expect("normalized Program path is rooted");
    std::iter::once(root.to_owned())
        .chain(
            tail.split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        )
        .collect()
}

fn from_components(parts: &[String]) -> String {
    let Some(root) = parts.first() else {
        return String::new();
    };
    let mut result = root.clone();
    if !result.is_empty() && !result.ends_with('/') {
        result.push('/');
    }
    result.push_str(&parts[1..].join("/"));
    result
}

/// tsc-port: computeCommonSourceDirectoryOfFilenames @6.0.3
/// tsc-hash: 900713690ffd0a6a34f2b76b227d6c7b71a8c2f505b247c142dd946bfb31196b
/// tsc-span: _tsc.js:121909-121939
pub fn inferred_common_source_directory(
    source_files: &[&Path],
    current_directory: &Path,
    case_sensitive: bool,
) -> PathBuf {
    let mut common: Option<Vec<String>> = None;
    for source in source_files {
        let mut directory = components(&normalized(source, current_directory));
        directory.pop();
        if let Some(common) = &mut common {
            let shared = common
                .iter()
                .zip(&directory)
                .take_while(|(left, right)| {
                    canonical(left, case_sensitive) == canonical(right, case_sensitive)
                })
                .count();
            if shared == 0 {
                return PathBuf::new();
            }
            common.truncate(shared);
        } else {
            common = Some(directory);
        }
    }
    common.map_or_else(
        || current_directory.to_path_buf(),
        |common| PathBuf::from(from_components(&common)),
    )
}

/// tsc-port: getCommonSourceDirectory @6.0.3
/// tsc-hash: 6213cf3653b969239fea4dd1852031f25b46c5ed630b923fe3a7ca0b55d9c930
/// tsc-span: _tsc.js:116460-116475
pub fn common_source_directory(
    options: &CompilerOptions,
    config_file: Option<&Path>,
    source_files: &[&Path],
    current_directory: &Path,
    case_sensitive: bool,
) -> PathBuf {
    let directory = if let Some(root) = options.root_dir.as_deref().filter(|root| !root.is_empty())
    {
        normalized(Path::new(root), current_directory)
    } else if let Some(config) = config_file {
        directory_name(&config.to_string_lossy())
    } else {
        inferred_common_source_directory(source_files, current_directory, case_sensitive)
            .to_string_lossy()
            .into_owned()
    };
    let directory = if !directory.is_empty() && !directory.ends_with('/') {
        format!("{directory}/")
    } else {
        directory
    };
    PathBuf::from(directory)
}

/// tsc-port: getSourceFilePathInNewDirWorker @6.0.3
/// tsc-hash: 1c92b6269af48b15f457c7c5e64b620cc19f367cb7e0a38b07b74b05b4940203
/// tsc-span: _tsc.js:16638-16643
pub fn source_file_path_in_new_directory(
    source: &Path,
    directory: &str,
    common: &Path,
    current_directory: &Path,
    case_sensitive: bool,
) -> PathBuf {
    let source = normalized(source, current_directory);
    let common = common.to_string_lossy();
    let relative =
        if canonical(&source, case_sensitive).starts_with(&canonical(&common, case_sensitive)) {
            // tsc slices the original source using the common directory's UTF-16
            // length, independently from canonical spelling/Unicode byte lengths.
            String::from_utf16_lossy(
                &source
                    .encode_utf16()
                    .skip(common.encode_utf16().count())
                    .collect::<Vec<_>>(),
            )
        } else {
            source
        };
    if directory.is_empty() || normalized_root_parts(&relative).is_some() {
        PathBuf::from(relative)
    } else {
        let directory = directory.replace('\\', "/");
        PathBuf::from(format!(
            "{directory}{}{relative}",
            if directory.ends_with('/') { "" } else { "/" }
        ))
    }
}

/// tsc-port: sourceFileMayBeEmitted @6.0.3
/// tsc-hash: 333fcd249758d38eb80146910286d7cabdbbf6f1ea0787f8f1a2c85e9535ecb2
/// tsc-span: _tsc.js:16617-16634
pub fn source_file_may_be_emitted_for_options(
    source: &Path,
    eligible: bool,
    options: &CompilerOptions,
    config_file: Option<&Path>,
    current_directory: &Path,
    case_sensitive: bool,
) -> bool {
    let name = source.to_string_lossy();
    if !eligible || is_declaration_file_name(&name) {
        return false;
    }
    let name = name.to_ascii_lowercase();
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
        .as_deref()
        .is_some_and(|file| !file.is_empty())
    {
        return true;
    }
    let Some(out_dir) = options
        .out_dir
        .as_deref()
        .filter(|directory| !directory.is_empty())
    else {
        return false;
    };
    if options
        .root_dir
        .as_deref()
        .is_some_and(|root| !root.is_empty())
        || config_file.is_some()
    {
        let common =
            common_source_directory(options, config_file, &[], current_directory, case_sensitive);
        let common = PathBuf::from(normalized(&common, current_directory));
        let output = source_file_path_in_new_directory(
            source,
            out_dir,
            &common,
            current_directory,
            case_sensitive,
        );
        if canonical_emit_path(source, current_directory, case_sensitive)
            == canonical_emit_path(&output, current_directory, case_sensitive)
        {
            return false;
        }
    }
    true
}

pub(crate) fn directory_relative_to_config(
    config: &Path,
    directory: &Path,
    case_sensitive: bool,
) -> String {
    let mut from = components(&config.to_string_lossy());
    from.pop();
    let to = components(&directory.to_string_lossy());
    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| {
            canonical(left, case_sensitive) == canonical(right, case_sensitive)
        })
        .count();
    if shared == 0 {
        return directory.to_string_lossy().into_owned();
    }
    let mut parts = vec!["..".to_owned(); from.len() - shared];
    parts.extend_from_slice(&to[shared..]);
    let relative = parts.join("/");
    if relative == ".." || relative.starts_with("../") {
        relative
    } else {
        format!("./{relative}")
    }
}

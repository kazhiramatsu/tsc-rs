//! tsgo's `tspath` helpers on the request file system's file names, over
//! tsc-rs's port of them (`tsc_program`).

use tsc_diagnostics::JsStr;

/// `tspath.GetNormalizedAbsolutePath`.
pub(crate) fn normalized_absolute_path(path: &str, current_directory: &str) -> String {
    tsc_program::get_normalized_absolute_path(
        JsStr::from_str(path),
        JsStr::from_str(current_directory),
    )
    .to_string_lossy()
    .into_owned()
}

/// `tspath.GetDirectoryPath`.
pub(crate) fn directory_path(path: &str) -> String {
    tsc_program::get_directory_path(JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// `tspath.CombinePaths` of two paths.
pub(crate) fn combine_paths(base: &str, path: &str) -> String {
    tsc_program::combine_paths(JsStr::from_str(base), JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// `tspath.GetBaseFileName`.
pub(crate) fn base_file_name(path: &str) -> String {
    tsc_program::base_file_name(JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// `tspath.IsDiskPathRoot`: a root and nothing else.
pub(crate) fn is_disk_path_root(path: &str) -> bool {
    tsc_program::path_root_parts(JsStr::from_str(path)).is_some_and(|(_, rest)| rest.is_empty())
}

/// `tspath.HasTrailingDirectorySeparator`.
pub(crate) fn has_trailing_separator(path: &str) -> bool {
    path.ends_with(['/', '\\'])
}

/// `tspath.RemoveTrailingDirectorySeparator`: one separator.
pub(crate) fn remove_trailing_separator(path: &str) -> &str {
    if has_trailing_separator(path) {
        &path[..path.len() - 1]
    } else {
        path
    }
}

/// `tspath.EnsureTrailingDirectorySeparator`.
pub(crate) fn ensure_trailing_separator(path: &str) -> String {
    if has_trailing_separator(path) {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

/// `tspath.GetCanonicalFileName`.
pub(crate) fn canonical_file_name(name: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        name.to_owned()
    } else {
        tsc_host::to_file_name_lower_case(name)
    }
}

/// `tspath.ToPath`.
pub(crate) fn to_path(path: &str, current_directory: &str, case_sensitive: bool) -> String {
    canonical_file_name(
        &normalized_absolute_path(path, current_directory),
        case_sensitive,
    )
}

/// `tspath.TrimFilePathPrefix`: the rest of `path` after `prefix`, compared
/// by canonical name (the original's characters skipped one for one).
pub(crate) fn trim_file_path_prefix<'p>(
    path: &'p str,
    prefix: &str,
    case_sensitive: bool,
) -> Option<&'p str> {
    if case_sensitive {
        return path.strip_prefix(prefix);
    }
    let canonical_prefix = canonical_file_name(prefix, false);
    if !canonical_file_name(path, false).starts_with(&canonical_prefix) {
        return None;
    }
    let skip = canonical_prefix.chars().count();
    let start = path
        .char_indices()
        .nth(skip)
        .map_or(path.len(), |(index, _)| index);
    Some(&path[start..])
}

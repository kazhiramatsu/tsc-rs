//! Compiler-facing virtual queries retain UTF-16 units. Fixture files enter
//! through scalar corpus names; query names need not have that restriction.
use super::CompilerUnitInput;
use std::collections::HashSet;
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::to_file_name_lower_case_js as fold;
use tsc_program::{ConfigFilePattern, ConfigHostError, ConfigHostOperation};

pub(super) fn library_query(path: JsStr<'_>, root: JsStr<'_>) -> bool {
    if !path.starts_with("/") || path.split_ascii(b'/').any(|part| part == "..") {
        return false;
    }
    let mut parts = path
        .split_ascii(b'/')
        .filter(|part| !part.is_empty() && *part != ".");
    root.split_ascii(b'/')
        .filter(|part| !part.is_empty() && *part != ".")
        .all(|part| parts.next() == Some(part))
}

pub(super) fn join(directory: JsStr<'_>, name: JsStr<'_>) -> JsString {
    let mut result = directory.to_owned();
    if !result.ends_with("/") {
        result.push_str("/");
    }
    result.push_js(name);
    result
}

pub(super) fn trim_end_slashes(mut path: JsStr<'_>) -> JsStr<'_> {
    while let Some(prefix) = path.strip_suffix("/") {
        path = prefix;
    }
    path
}

pub(super) fn directory_prefix(path: JsStr<'_>) -> JsString {
    let mut prefix = trim_end_slashes(path).to_owned();
    prefix.push_str("/");
    prefix
}

fn slashes(path: JsStr<'_>) -> JsString {
    path.code_units()
        .map(|unit| if unit == 92 { 47 } else { unit })
        .collect()
}

pub(super) fn normalize_virtual(base: JsStr<'_>, path: JsStr<'_>) -> Result<JsString, String> {
    let path = slashes(path);
    let combined = if path.starts_with("/") {
        path
    } else {
        join(base, path.as_js())
    };
    // The scalar corpus normalizer flips separators after joining the base,
    // too. Preserve that order for virtual JS queries.
    let combined = slashes(combined.as_js());
    if combined.contains("\0") {
        return Err(format!("virtual path contains NUL: {combined:?}"));
    }
    if !combined.starts_with("/") {
        return Err(format!("virtual path is not absolute: {combined:?}"));
    }
    let mut parts = Vec::new();
    for part in combined.as_js().split_ascii(b'/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }
    let mut result = JsString::from("/");
    for (index, part) in parts.into_iter().enumerate() {
        if index != 0 {
            result.push_str("/");
        }
        result.push_js(part);
    }
    Ok(result)
}

fn root_parts(path: JsStr<'_>) -> Option<(JsStr<'_>, JsStr<'_>)> {
    if let Some(tail) = path.strip_prefix("//") {
        return match tail.as_bytes().iter().position(|byte| *byte == b'/') {
            Some(index) => path.split_at_byte(index + 3),
            None => Some((path, "".into())),
        };
    }
    if path.starts_with("/") {
        return path.split_at_byte(1);
    }
    let bytes = path.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':') {
        if bytes.get(2) == Some(&b'/') {
            return path.split_at_byte(3);
        }
        if bytes.len() == 2 {
            return Some((path, "".into()));
        }
    }
    None
}

fn normalize_rooted(path: JsStr<'_>) -> Result<JsString, String> {
    if path.contains("\0") {
        return Err(format!("compiler virtual path contains NUL: {path:?}"));
    }
    let (root, tail) =
        root_parts(path).ok_or_else(|| format!("compiler virtual path is not rooted: {path:?}"))?;
    let mut parts = Vec::new();
    for part in tail.split_ascii(b'/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }
    let mut result = root.to_owned();
    for (index, part) in parts.into_iter().enumerate() {
        if index != 0 {
            result.push_str("/");
        }
        result.push_js(part);
    }
    Ok(result)
}

fn absolute_pattern(directory: JsStr<'_>, pattern: JsStr<'_>) -> JsString {
    let pattern = slashes(pattern);
    let combined = if root_parts(pattern.as_js()).is_some() {
        pattern
    } else {
        join(directory, pattern.as_js())
    };
    normalize_rooted(combined.as_js()).unwrap_or(combined)
}

fn contains(parent: JsStr<'_>, child: JsStr<'_>) -> bool {
    let parent = fold(trim_end_slashes(parent));
    let child = fold(trim_end_slashes(child));
    child == parent
        || child
            .as_js()
            .starts_with_js(directory_prefix(parent.as_js()).as_js())
}

pub(super) fn base_paths(directory: JsStr<'_>, includes: &[JsString]) -> Vec<JsString> {
    let mut include_bases = includes
        .iter()
        .map(|include| {
            let absolute = absolute_pattern(directory, include.as_js());
            let path = absolute.as_js();
            let wildcard = path
                .as_bytes()
                .iter()
                .position(|byte| matches!(byte, b'*' | b'?'));
            match wildcard {
                Some(index) => path
                    .split_at_byte(index)
                    .expect("ASCII wildcard boundary")
                    .0
                    .rsplit_once("/")
                    .map_or_else(|| directory.to_owned(), |(prefix, _)| prefix.to_owned()),
                None if path
                    .split_ascii(b'/')
                    .next_back()
                    .is_some_and(|base| base.contains(".")) =>
                {
                    path.rsplit_once("/")
                        .map_or_else(|| directory.to_owned(), |(prefix, _)| prefix.to_owned())
                }
                None => absolute,
            }
        })
        .collect::<Vec<_>>();
    include_bases.sort_by(|a, b| fold(a.as_js()).cmp_utf16(fold(b.as_js()).as_js()));
    let mut bases = vec![directory.to_owned()];
    for base in include_bases {
        if bases
            .iter()
            .all(|parent| !contains(parent.as_js(), base.as_js()))
        {
            bases.push(base);
        }
    }
    bases
}

fn exclude_matches(directory: JsStr<'_>, pattern: JsStr<'_>, path: JsStr<'_>) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = absolute_pattern(directory, pattern);
    if pattern.contains("*") || pattern.contains("?") {
        return glob_matches(pattern.as_js(), path);
    }
    contains(pattern.as_js(), path)
}

fn glob_matches(pattern: JsStr<'_>, text: JsStr<'_>) -> bool {
    let pattern = fold(pattern).to_utf16();
    let text = fold(text).to_utf16();
    let mut memo = vec![vec![None; text.len() + 1]; pattern.len() + 1];
    fn matches(
        pattern: &[u16],
        text: &[u16],
        p: usize,
        t: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(value) = memo[p][t] {
            return value;
        }
        let result = if p == pattern.len() {
            t == text.len()
        } else if pattern[p] == 42 {
            let double = pattern.get(p + 1) == Some(&42);
            let after = p + if double { 2 } else { 1 };
            if double && pattern.get(after) == Some(&47) {
                matches(pattern, text, after + 1, t, memo)
                    || (t < text.len() && matches(pattern, text, p, t + 1, memo))
            } else {
                matches(pattern, text, after, t, memo)
                    || (t < text.len()
                        && (double || text[t] != 47)
                        && matches(pattern, text, p, t + 1, memo))
            }
        } else if t < text.len() && ((pattern[p] == 63 && text[t] != 47) || pattern[p] == text[t]) {
            matches(pattern, text, p + 1, t + 1, memo)
        } else {
            false
        };
        memo[p][t] = Some(result);
        result
    }
    matches(&pattern, &text, 0, 0, &mut memo)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn visit_directory(
    units: &[CompilerUnitInput],
    base_directory: JsStr<'_>,
    directory: JsStr<'_>,
    extensions: &[&str],
    excludes: Option<&[JsString]>,
    includes: &[JsString],
    include_patterns: &[Option<ConfigFilePattern>],
    depth: Option<usize>,
    visited: &mut HashSet<JsString>,
    buckets: &mut [Vec<JsString>],
) -> Result<(), ConfigHostError> {
    let directory_key = fold(directory);
    if !visited.insert(directory_key.clone()) {
        return Ok(());
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    for unit in units {
        let normalized =
            super::normalize_compiler_unit_path(unit.name.as_ref()).map_err(|error| {
                ConfigHostError::new(
                    ConfigHostOperation::ReadDirectory,
                    unit.name.as_ref(),
                    error.to_string(),
                )
            })?;
        let normalized = JsStr::from(normalized.as_str());
        // Preserve harnessIO's raw folded prefix and JS slice offset, even
        // where the original spelling and its case fold have different lengths.
        if !fold(normalized)
            .as_js()
            .starts_with_js(directory_key.as_js())
        {
            continue;
        }
        let tail = normalized.substring(directory.len_units(), normalized.len_units());
        let tail = tail.as_js().strip_prefix("/").unwrap_or(tail.as_js());
        if let Some((child, _)) = tail.split_once("/") {
            if !child.is_empty()
                && !directories
                    .iter()
                    .any(|entry: &JsString| entry.as_js() == child)
            {
                directories.push(child.to_owned());
            }
        } else if !tail.is_empty() {
            files.push(tail.to_owned());
        }
    }
    files.sort_by(|a, b| a.cmp_utf16(b.as_js()));
    for file in files {
        let path = join(directory, file.as_js());
        if !extensions
            .iter()
            .any(|extension| path.len_units() > extension.len() && path.ends_with(extension))
            || excludes.is_some_and(|patterns| {
                patterns
                    .iter()
                    .any(|pattern| exclude_matches(base_directory, pattern.as_js(), path.as_js()))
            })
        {
            continue;
        }
        let include_index = if includes.is_empty() {
            Some(0)
        } else {
            include_patterns.iter().position(|pattern| {
                pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern.matches(path.as_js()))
            })
        };
        if let Some(index) = include_index {
            buckets[index].push(path);
        }
    }
    if depth == Some(1) {
        return Ok(());
    }
    let child_depth = depth.map(|depth| depth.saturating_sub(1));
    directories.sort_by(|a, b| a.cmp_utf16(b.as_js()));
    for child in directories {
        let path = join(directory, child.as_js());
        if excludes.is_some_and(|patterns| {
            patterns
                .iter()
                .any(|pattern| exclude_matches(base_directory, pattern.as_js(), path.as_js()))
        }) {
            continue;
        }
        visit_directory(
            units,
            base_directory,
            path.as_js(),
            extensions,
            excludes,
            includes,
            include_patterns,
            child_depth,
            visited,
            buckets,
        )?;
    }
    Ok(())
}

pub(super) fn json_string(value: JsStr<'_>) -> tsc_program::JsonValue {
    tsc_program::JsonValue::String(value.to_owned())
}

pub(super) fn json_strings(values: &[JsString]) -> tsc_program::JsonValue {
    tsc_program::JsonValue::Array(
        values
            .iter()
            .map(|value| json_string(value.as_js()))
            .collect(),
    )
}

pub(super) fn log_entry<const N: usize>(
    values: [(&str, tsc_program::JsonValue); N],
) -> tsc_program::JsonValue {
    tsc_program::JsonValue::Object(values.into_iter().collect())
}

pub(super) fn scalar_json_observation(
    value: &tsc_program::JsonValue,
) -> crate::HarnessResult<serde_json::Value> {
    use serde_json::Value as V;
    use tsc_program::JsonValue as J;
    let scalar = |text: JsStr<'_>| {
        text.as_str().map(str::to_owned).ok_or_else(|| {
            super::error(format!(
                "scalar corpus JSON observer cannot represent JS text {text:?}"
            ))
        })
    };
    Ok(match value {
        J::Null => V::Null,
        J::Bool(value) => V::Bool(*value),
        J::Number(value) => V::Number(value.clone()),
        J::String(value) => V::String(scalar(value.as_js())?),
        J::Array(values) => V::Array(
            values
                .iter()
                .map(scalar_json_observation)
                .collect::<crate::HarnessResult<_>>()?,
        ),
        J::Object(values) => {
            let mut result = serde_json::Map::new();
            for (key, value) in values.iter() {
                result.insert(scalar(key.as_js())?, scalar_json_observation(value)?);
            }
            V::Object(result)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_normalization_keeps_scalar_corpus_semantics_and_utf16_identity() {
        for (base, path) in [
            ("/work", "a.ts"),
            (r"\work\src", r"..\a.ts"),
            ("/work", "/../../a.ts"),
            ("relative", "a.ts"),
            ("/work", "bad\0.ts"),
            ("/work/", "./a//b/../c.ts"),
        ] {
            let scalar = super::super::normalize_virtual_path(base, path);
            let js = normalize_virtual(base.into(), path.into());
            match (scalar, js) {
                (Ok(expected), Ok(actual)) => assert_eq!(actual.as_js(), expected.as_str()),
                (Err(_), Err(_)) => {}
                outcomes => panic!("normalization boundary changed: {outcomes:?}"),
            }
        }
        let mut paths = Vec::new();
        for unit in [0xd800, 0xd801, 0xdc00, 0xfffd] {
            let mut input = JsString::from("x/../");
            input.push_js(JsString::from_code_units(&[unit]).as_js());
            input.push_str(".ts");
            let result = normalize_virtual(r"\work\src".into(), input.as_js()).unwrap();
            let mut expected = "/work/src/".encode_utf16().collect::<Vec<_>>();
            expected.push(unit);
            expected.extend(".ts".encode_utf16());
            assert_eq!(result.to_utf16(), expected);
            assert!(paths.iter().all(|previous| previous != &result));
            paths.push(result);
        }
    }
}

//! Pure TypeScript path operations on canonical JavaScript strings. These
//! operations preserve names; host path encoding is a separate boundary.
use tsc_diagnostics::{JsStr, JsString};

fn prefix(text: JsStr<'_>, end: usize) -> JsStr<'_> {
    text.split_at_byte(end)
        .expect("path grammar cuts whole code points")
        .0
}

fn suffix(text: JsStr<'_>, start: usize) -> JsStr<'_> {
    text.split_at_byte(start)
        .expect("path grammar cuts whole code points")
        .1
}

fn slice(text: JsStr<'_>, start: usize, end: usize) -> JsStr<'_> {
    prefix(suffix(text, start), end - start)
}

/// getEncodedRootLength/getRootLength, _tsc.js:5349–5390. Internally the
/// offset is in bytes: every root boundary is selected by ASCII punctuation
/// or the end of a canonical string. All subsequent slicing uses that unit.
fn root_end_byte(path: JsStr<'_>) -> usize {
    let bytes = path.as_bytes();
    let Some(&first) = bytes.first() else {
        return 0;
    };
    if matches!(first, b'/' | b'\\') {
        if bytes.get(1) != Some(&first) {
            return 1;
        }
        return bytes
            .iter()
            .enumerate()
            .skip(2)
            .find(|(_, byte)| **byte == first)
            .map_or(bytes.len(), |(index, _)| index + 1);
    }
    if first.is_ascii_alphabetic() && bytes.get(1) == Some(&b':') {
        if matches!(bytes.get(2), Some(b'/' | b'\\')) {
            return 3;
        }
        if bytes.len() == 2 {
            return 2;
        }
    }
    let Some((scheme, rest)) = path.split_once("://") else {
        return 0;
    };
    let authority_start = scheme.as_bytes().len() + 3;
    let Some((authority, _)) = rest.split_once("/") else {
        return bytes.len();
    };
    let authority_end = authority_start + authority.as_bytes().len();
    if scheme == "file"
        && (authority == "" || authority == "localhost")
        && bytes
            .get(authority_end + 1)
            .is_some_and(u8::is_ascii_alphabetic)
    {
        let volume_start = authority_end + 2;
        let volume_end = match bytes.get(volume_start..) {
            Some([b':', ..]) => Some(volume_start + 1),
            Some([b'%', b'3', b'a' | b'A', ..]) => Some(volume_start + 3),
            _ => None,
        };
        if let Some(end) = volume_end {
            if bytes.get(end) == Some(&b'/') {
                return end + 1;
            }
            if end == bytes.len() {
                return end;
            }
        }
    }
    authority_end + 1
}

pub(crate) fn root_parts(path: JsStr<'_>) -> Option<(JsStr<'_>, JsStr<'_>)> {
    let end = root_end_byte(path);
    (end != 0).then(|| path.split_at_byte(end).expect("root boundary is canonical"))
}

/// TypeScript getBaseFileName without extension removal (_tsc.js:5398–5405).
/// The returned value is a JS string; a bare disk/UNC/URL root has no basename.
pub fn base_file_name<'p>(path: impl Into<JsStr<'p>>) -> JsString {
    let path = normalize_slashes(path.into());
    let path = path.as_js();
    if root_end_byte(path) == path.as_bytes().len() {
        return JsString::new();
    }
    let path = path.strip_suffix("/").unwrap_or(path);
    let after_separator = path
        .as_bytes()
        .iter()
        .rposition(|byte| *byte == b'/')
        .map_or(0, |index| index + 1);
    suffix(path, root_end_byte(path).max(after_separator)).to_owned()
}

/// `getDirectoryPath`: trim one trailing separator, then retain the prefix
/// through the last component boundary without truncating a disk/URL root.
pub(crate) fn directory_name(path: JsStr<'_>) -> JsString {
    let slashed = normalize_slashes(path);
    let slashed = slashed.as_js();
    let root_length = root_end_byte(slashed);
    if root_length == slashed.as_bytes().len() {
        return slashed.to_owned();
    }
    let trimmed = slashed.strip_suffix("/").unwrap_or(slashed);
    let last_separator = trimmed
        .as_bytes()
        .iter()
        .rposition(|byte| *byte == b'/')
        .unwrap_or(0);
    prefix(trimmed, root_length.max(last_separator)).to_owned()
}

fn replace_scalar(text: JsStr<'_>, separator: &str, replacement: &str) -> JsString {
    debug_assert!(!separator.is_empty());
    let mut rest = text;
    let mut result = JsString::with_capacity(text.as_bytes().len());
    while let Some((before, after)) = rest.split_once(separator) {
        result.push_js(before);
        result.push_str(replacement);
        rest = after;
    }
    result.push_js(rest);
    result
}

pub(crate) fn normalize_slashes(path: JsStr<'_>) -> JsString {
    replace_scalar(path, "\\", "/")
}

/// The existing host's TypeScript filename case profile, applied to a JS
/// value before any filesystem conversion. Unpaired surrogates are unchanged
/// and break Unicode casing context; scalar runs use the shared profile.
pub(crate) fn file_name_lower_case(path: JsStr<'_>) -> JsString {
    tsc_host::to_file_name_lower_case_js(path)
}

pub(crate) fn file_name_key(path: JsStr<'_>, case_sensitive: bool) -> JsString {
    if case_sensitive {
        path.to_owned()
    } else {
        file_name_lower_case(path)
    }
}

/// `equateStringsCaseInsensitive` compares uppercase values. This differs
/// from the filename-key lowercase profile; unpaired units remain intact.
pub(crate) fn eq_ignore_case(left: JsStr<'_>, right: JsStr<'_>) -> bool {
    left == right || uppercase(left) == uppercase(right)
}

pub(crate) fn uppercase(value: JsStr<'_>) -> JsString {
    let mut result = JsString::new();
    for point in char::decode_utf16(value.code_units()) {
        match point {
            Ok(ch) => ch.to_uppercase().for_each(|ch| result.push(ch)),
            Err(unit) => result.push_code_unit(unit.unpaired_surrogate()),
        }
    }
    result
}

/// combinePaths, _tsc.js:5474–5487; root recognition precedes concatenation.
pub(crate) fn combine_paths(parent: JsStr<'_>, child: JsStr<'_>) -> JsString {
    let child = normalize_slashes(child);
    if parent.is_empty() || root_end_byte(child.as_js()) != 0 {
        return child;
    }
    let mut combined = normalize_slashes(parent);
    if !child.is_empty() {
        if !combined.ends_with("/") {
            combined.push('/');
        }
        combined.push_js(child.as_js());
    }
    combined
}

fn has_relative_segment(path: JsStr<'_>) -> bool {
    if path.contains("//") {
        return true;
    }
    let mut rest = path;
    while let Some((segment, after)) = rest.split_once("/") {
        if segment == "." || segment == ".." {
            return true;
        }
        rest = after;
    }
    rest == "." || rest == ".."
}

fn simple_normalize(path: JsStr<'_>) -> Option<JsString> {
    if !has_relative_segment(path) {
        return Some(path.to_owned());
    }
    let simplified = replace_scalar(path, "/./", "/");
    let simplified = simplified
        .as_js()
        .strip_prefix("./")
        .unwrap_or(simplified.as_js());
    (simplified != path && !has_relative_segment(simplified)).then(|| simplified.to_owned())
}

fn remove_trailing_separator_once(mut path: JsString, root_length: usize) -> JsString {
    if path.as_bytes().len() > root_length && path.ends_with("/") {
        assert!(path.truncate_bytes(path.as_bytes().len() - 1));
    }
    path
}

/// getNormalizedAbsolutePath, _tsc.js:5493–5567. Separator/dot searches are
/// byte operations at ASCII boundaries. The normalizedUpTo - 2 operation
/// below remains explicitly in UTF-16 units. Arbitrary segment values are
/// copied without scalar projection.
pub(crate) fn normalized_absolute_path(path: JsStr<'_>, current_directory: JsStr<'_>) -> JsString {
    let mut root_length = root_end_byte(path);
    let path = if root_length == 0 && !current_directory.is_empty() {
        let combined = combine_paths(current_directory, path);
        root_length = root_end_byte(combined.as_js());
        combined
    } else {
        normalize_slashes(path)
    };
    let path = path.as_js();
    let root = prefix(path, root_length);
    if let Some(simple) = simple_normalize(path) {
        return remove_trailing_separator_once(simple, root_length);
    }
    let bytes = path.as_bytes();
    let mut normalized = None::<JsString>;
    let mut index = root_length;
    let mut normalized_up_to = index;
    let mut seen_non_dot_dot_segment = root_length != 0;
    while index < bytes.len() {
        let mut segment_start = index;
        while bytes[index] == b'/' && index + 1 < bytes.len() {
            index += 1;
        }
        if index > segment_start {
            normalized
                .get_or_insert_with(|| prefix(path, segment_start.saturating_sub(1)).to_owned());
            segment_start = index;
        }
        let mut segment_end = index + 1;
        while segment_end < bytes.len() && bytes[segment_end] != b'/' {
            segment_end += 1;
        }
        let segment = slice(path, segment_start, segment_end);
        if segment == "." {
            normalized.get_or_insert_with(|| prefix(path, normalized_up_to).to_owned());
        } else if segment == ".." {
            if !seen_non_dot_dot_segment {
                if let Some(normalized) = &mut normalized {
                    normalized.push_str(if normalized.as_bytes().len() == root_length {
                        ".."
                    } else {
                        "/.."
                    });
                } else {
                    normalized_up_to = index + 2;
                }
            } else if normalized.is_none() {
                // normalizedUpTo - 2 is a UTF-16 position, not a byte
                // position. A one-unit non-ASCII first relative segment must
                // take TypeScript's short-prefix branch just as ASCII does.
                let up_to_units = prefix(path, normalized_up_to).len_units();
                normalized = Some(if up_to_units >= 2 {
                    let end_units = path
                        .code_units()
                        .take(up_to_units - 1)
                        .enumerate()
                        .filter_map(|(index, unit)| (unit == u16::from(b'/')).then_some(index))
                        .last()
                        .unwrap_or(0)
                        .max(root.len_units());
                    path.substring(0, end_units)
                } else {
                    prefix(path, normalized_up_to).to_owned()
                });
            } else if let Some(normalized) = &mut normalized {
                if let Some(last_slash) =
                    normalized.as_bytes().iter().rposition(|byte| *byte == b'/')
                {
                    assert!(normalized.truncate_bytes(last_slash.max(root_length)));
                } else {
                    *normalized = root.to_owned();
                }
                if normalized.as_bytes().len() == root_length {
                    seen_non_dot_dot_segment = root_length != 0;
                }
            }
        } else if let Some(normalized) = &mut normalized {
            if normalized.as_bytes().len() != root_length {
                normalized.push('/');
            }
            seen_non_dot_dot_segment = true;
            normalized.push_js(segment);
        } else {
            seen_non_dot_dot_segment = true;
            normalized_up_to = segment_end;
        }
        index = segment_end + 1;
    }
    normalized.unwrap_or_else(|| remove_trailing_separator_once(path.to_owned(), root_length))
}

pub(crate) fn normalized_config_dir_value_path<'b>(
    value: JsStr<'_>,
    config_base_path: impl Into<JsStr<'b>>,
) -> Option<JsString> {
    if !starts_with_ignore_case(value, CONFIG_DIR_TEMPLATE) {
        return None;
    }
    // TypeScript's admission is case insensitive, but replace is case
    // sensitive. Preserve that distinction (including the dotless-i case).
    let substituted = if let Some((before, after)) = value.split_once(CONFIG_DIR_TEMPLATE) {
        let mut result = before.to_owned();
        result.push_str("./");
        result.push_js(after);
        result
    } else {
        value.to_owned()
    };
    Some(normalized_absolute_path(
        substituted.as_js(),
        config_base_path.into(),
    ))
}

pub(crate) fn normalized_config_value_path<'b>(
    value: JsStr<'_>,
    base_path: impl Into<JsStr<'b>>,
) -> JsString {
    normalized_absolute_path(
        if value.is_empty() { ".".into() } else { value },
        base_path.into(),
    )
}

const CONFIG_DIR_TEMPLATE: &str = "${configDir}";

pub(crate) fn starts_with_config_dir_template<'s>(value: impl Into<JsStr<'s>>) -> bool {
    starts_with_ignore_case(value, CONFIG_DIR_TEMPLATE)
}

/// TypeScript's ignore-case startsWith uppercases a UTF-16 slice instead of
/// applying ASCII-only folding. Keep the allocation-free common ASCII path,
/// then reproduce that Unicode behavior for spellings such as dotless-i.
///
/// tsc-port: equateStringsCaseInsensitive @6.0.3
/// tsc-hash: ab81c5a8cd044f72148e7e8ecb60f7003c0c3afb2b7ecde10d6bc4f48132975a
/// tsc-span: _tsc.js:905-906
/// tsc-port: startsWith @6.0.3
/// tsc-hash: b0a4b4a17f81742d08ed6267db9860c810ceb118696b1c83bd7655f9fa1b10b4
/// tsc-span: _tsc.js:1078-1079
fn starts_with_ignore_case<'s>(value: impl Into<JsStr<'s>>, prefix: &str) -> bool {
    let value = value.into();
    if let Some(candidate) = value.as_bytes().get(..prefix.len()) {
        if candidate.eq_ignore_ascii_case(prefix.as_bytes()) {
            return true;
        }
        if candidate.is_ascii() {
            return false;
        }
    }
    let prefix_length = prefix.encode_utf16().count();
    let candidate = value.substring(0, prefix_length);
    // A surrogate cannot case-fold to the scalar config-dir marker. Other
    // Unicode scalars retain the existing JavaScript uppercase comparison.
    candidate
        .as_str()
        .is_some_and(|candidate| candidate.to_uppercase() == prefix.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_basenames_match_typescript_values_and_root_boundaries() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/utf16-generated-module-names.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            assert_eq!(
                base_file_name(value(&case["value_utf16"]).as_js()),
                value(&case["expected"]["base_name_utf16"]),
                "{}",
                case["case_id"]
            );
        }
    }

    #[test]
    fn file_option_value_phases_match_repeated_typescript_config_observations() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/utf16-config-path-values.json"
        ))
        .unwrap();
        assert_eq!(fixture["repetitions"], 2);
        for case in fixture["cases"].as_array().unwrap() {
            let input = value(&case["value_utf16"]);
            let base = case["base"].as_str().unwrap();
            let written = normalize_slashes(input.as_js());
            let converted = if starts_with_config_dir_template(&written) {
                written
            } else {
                normalized_config_value_path(written.as_js(), base)
            };
            let finalized =
                normalized_config_dir_value_path(converted.as_js(), base).unwrap_or(converted);
            assert_eq!(
                finalized,
                value(&case["out_dir_utf16"]),
                "{}",
                case["case_id"]
            );
        }
    }
    fn value(units: &serde_json::Value) -> JsString {
        JsString::from_code_units(
            &units
                .as_array()
                .unwrap()
                .iter()
                .map(|unit| unit.as_u64().unwrap() as u16)
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn lexical_paths_match_repeated_typescript_values() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/utf16-lexical-paths.json"))
                .unwrap();
        assert_eq!(fixture["repetitions"], 2);
        for case in fixture["cases"].as_array().unwrap() {
            let path = value(&case["path_utf16"]);
            let base = value(&case["base_utf16"]);
            let root_length = prefix(path.as_js(), root_end_byte(path.as_js())).len_units();
            assert_eq!(
                root_length,
                case["root_length"].as_u64().unwrap() as usize,
                "root {}",
                case["case_id"]
            );
            assert_eq!(
                normalized_absolute_path(path.as_js(), base.as_js()),
                value(&case["normalized_utf16"]),
                "normalize {}",
                case["case_id"]
            );
            assert_eq!(
                combine_paths(base.as_js(), path.as_js()),
                value(&case["combined_utf16"]),
                "combine {}",
                case["case_id"]
            );
        }
    }
}

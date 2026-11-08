// Unchanged base algorithms from 1a05993; independent differential oracle.
use super::*;
use crate::ResolutionError;

fn old_prefix(text: JsStr<'_>, end: usize) -> JsStr<'_> {
    text.split_at_byte(end)
        .expect("path grammar cuts whole code points")
        .0
}

fn old_suffix(text: JsStr<'_>, start: usize) -> JsStr<'_> {
    text.split_at_byte(start)
        .expect("path grammar cuts whole code points")
        .1
}

fn old_slice(text: JsStr<'_>, start: usize, end: usize) -> JsStr<'_> {
    old_prefix(old_suffix(text, start), end - start)
}

fn old_root_end_byte(path: JsStr<'_>) -> usize {
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
    // tsgo: a dynamic file name (an untitled document's) roots at `^/`, an
    // encoded URI name past its scheme and authority.
    if first == b'^' && bytes.get(1) == Some(&b'/') {
        let old_prefix = tsc_host::DYNAMIC_URI_FILE_NAME_PREFIX.as_bytes();
        if let Some(rest) = bytes.strip_prefix(old_prefix) {
            if let Some(scheme_end) = rest.iter().position(|&byte| byte == b'/') {
                let authority_start = old_prefix.len() + scheme_end + 1;
                return match bytes[authority_start..]
                    .iter()
                    .position(|&byte| byte == b'/')
                {
                    Some(authority_end) => authority_start + authority_end + 1,
                    None => bytes.len(),
                };
            }
        }
        return 2;
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
        && (authority.is_empty() || authority == "localhost")
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

fn old_replace_scalar(text: JsStr<'_>, separator: &str, replacement: &str) -> JsString {
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

pub fn old_normalize_slashes(path: JsStr<'_>) -> JsString {
    old_replace_scalar(path, "\\", "/")
}

pub fn old_combine_paths(parent: JsStr<'_>, child: JsStr<'_>) -> JsString {
    let child = old_normalize_slashes(child);
    if parent.is_empty() || old_root_end_byte(child.as_js()) != 0 {
        return child;
    }
    let mut combined = old_normalize_slashes(parent);
    if !child.is_empty() {
        if !combined.ends_with("/") {
            combined.push('/');
        }
        combined.push_js(child.as_js());
    }
    combined
}

fn old_has_relative_segment(path: JsStr<'_>) -> bool {
    // `/` is ASCII, so the WTF-8 bytes split where the path's separators are.
    let bytes = path.as_bytes();
    bytes.windows(2).any(|pair| pair == b"//")
        || bytes
            .split(|&byte| byte == b'/')
            .any(|segment| segment == b"." || segment == b"..")
}

fn old_simple_normalize(path: JsStr<'_>) -> Option<JsString> {
    if !old_has_relative_segment(path) {
        return Some(path.to_owned());
    }
    let simplified = old_replace_scalar(path, "/./", "/");
    let simplified = simplified
        .as_js()
        .strip_prefix("./")
        .unwrap_or(simplified.as_js());
    (simplified != path && !old_has_relative_segment(simplified)).then(|| simplified.to_owned())
}

fn old_remove_trailing_separator_once(mut path: JsString, root_length: usize) -> JsString {
    if path.as_bytes().len() > root_length && path.ends_with("/") {
        assert!(path.truncate_bytes(path.as_bytes().len() - 1));
    }
    path
}

pub fn old_normalized_absolute_path(path: JsStr<'_>, current_directory: JsStr<'_>) -> JsString {
    let mut root_length = old_root_end_byte(path);
    let path = if root_length == 0 && !current_directory.is_empty() {
        let combined = old_combine_paths(current_directory, path);
        root_length = old_root_end_byte(combined.as_js());
        combined
    } else {
        old_normalize_slashes(path)
    };
    let path = path.as_js();
    let root = old_prefix(path, root_length);
    if let Some(simple) = old_simple_normalize(path) {
        return old_remove_trailing_separator_once(simple, root_length);
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
            normalized.get_or_insert_with(|| {
                old_prefix(path, segment_start.saturating_sub(1)).to_owned()
            });
            segment_start = index;
        }
        let mut segment_end = index + 1;
        while segment_end < bytes.len() && bytes[segment_end] != b'/' {
            segment_end += 1;
        }
        let segment = old_slice(path, segment_start, segment_end);
        if segment == "." {
            normalized.get_or_insert_with(|| old_prefix(path, normalized_up_to).to_owned());
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
                // take TypeScript's short-old_prefix branch just as ASCII does.
                let up_to_units = old_prefix(path, normalized_up_to).len_units();
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
                    old_prefix(path, normalized_up_to).to_owned()
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
    normalized.unwrap_or_else(|| old_remove_trailing_separator_once(path.to_owned(), root_length))
}

pub fn old_normalize_path(path: JsStr<'_>) -> JsString {
    let path = old_normalize_slashes(path);
    if let Some(simple) = old_simple_normalize(path.as_js()) {
        return simple;
    }
    let mut normalized = old_normalized_absolute_path(path.as_js(), JsStr::from_str(""));
    if !normalized.is_empty() && path.ends_with("/") && !normalized.ends_with("/") {
        normalized.push('/');
    }
    normalized
}

fn old_absolute(
    text: JsStr<'_>,
    base: Option<JsStr<'_>>,
    reject_nul: bool,
) -> Result<JsString, ResolutionError> {
    if text.is_empty() || (reject_nul && text.contains("\0")) {
        return Err(ResolutionError::canonicalization_js(
            Some(text),
            "path is empty or contains a NUL byte",
        ));
    }
    let slashed = old_normalize_slashes(text);
    let absolute = if root_parts(slashed.as_js()).is_some() {
        slashed
    } else {
        let base = base.ok_or_else(|| {
            ResolutionError::canonicalization_js(Some(text), "an absolute path is required")
        })?;
        if reject_nul && base.contains("\0") {
            return Err(ResolutionError::canonicalization_js(
                Some(text),
                "path base contains a NUL byte",
            ));
        }
        old_join(old_normalize_slashes(base).as_js(), slashed.as_js())
    };
    if root_parts(absolute.as_js()).is_none() {
        return Err(ResolutionError::canonicalization_js(
            Some(text),
            "path has no supported absolute root",
        ));
    }
    Ok(old_normalized_absolute_path(absolute.as_js(), "".into()))
}

fn old_join(parent: JsStr<'_>, mut child: JsStr<'_>) -> JsString {
    while let Some(tail) = child.strip_prefix("/") {
        child = tail;
    }
    let mut result = parent.to_owned();
    if !result.ends_with("/") {
        result.push('/');
    }
    result.push_js(child);
    result
}

fn compare<T: std::fmt::Debug + PartialEq>(
    label: &str,
    old: impl FnOnce() -> T + std::panic::UnwindSafe,
    new: impl FnOnce() -> T + std::panic::UnwindSafe,
) {
    let old = std::panic::catch_unwind(old);
    let new = std::panic::catch_unwind(new);
    match (old, new) {
        (Ok(old), Ok(new)) => assert_eq!(new, old, "{label}"),
        (Err(old), Err(new)) => {
            fn message(p: &(dyn std::any::Any + Send)) -> Option<&str> {
                p.downcast_ref::<&str>()
                    .copied()
                    .or_else(|| p.downcast_ref::<String>().map(String::as_str))
            }
            assert_eq!(message(&*new), message(&*old), "panic payload: {label}");
        }
        (old, new) => panic!(
            "different success/panic outcomes: {label}; base={}, new={}",
            old.is_ok(),
            new.is_ok()
        ),
    }
}

#[test]
fn generated_path_corpus_is_byte_exact_to_the_base_algorithms() {
    let roots = [
        "",
        "/",
        "//server/share/",
        "C:",
        "C:/",
        "c:\\",
        "http://host/",
        "file:///C:/",
        "file://localhost/c%3a/",
        "^/",
        "/\\",
    ];
    let segments = [
        "", ".", "..", "a", "b", "...", "a..", "é", "𐐀", "\0", "a\\b", "/", "//",
    ];
    let separators = ["/", "\\", "//", "/./", ""];
    let bases = [
        None,
        Some(""),
        Some("/base/dir"),
        Some("C:/base"),
        Some("//server/share/base"),
        Some("file:///base/"),
        Some("relative"),
        Some("/base\0"),
    ];
    let mut state = 0x123456789abcdef_u64;
    for index in 0..8192 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let mut path = JsString::from(roots[index % roots.len()]);
        for shift in [0, 8, 16] {
            path.push_str(segments[((state >> shift) as usize) % segments.len()]);
            path.push_str(separators[((state >> (shift + 4)) as usize) % separators.len()]);
        }
        match index % 5 {
            0 => path.push_code_unit(0xd800),
            1 => path.push_code_unit(0xdc00),
            2 => path.push('/'),
            _ => {}
        }
        let label = format!("path {path:?}");
        compare(
            &label,
            || old_normalize_slashes(path.as_js()),
            || normalize_slashes(path.as_js()),
        );
        compare(
            &label,
            || old_normalize_path(path.as_js()),
            || normalize_path(path.as_js()),
        );
        for base in bases {
            let base = base.map(JsStr::from);
            let label = format!("path {path:?}, base {base:?}");
            compare(
                &label,
                || old_normalized_absolute_path(path.as_js(), base.unwrap_or("".into())),
                || normalized_absolute_path(path.as_js(), base.unwrap_or("".into())),
            );
            for reject_nul in [false, true] {
                compare(
                    &format!("{label}, reject_nul {reject_nul}"),
                    || old_absolute(path.as_js(), base, reject_nul),
                    || {
                        crate::module_resolution::normalize_absolute_js_path(
                            path.as_js(),
                            base,
                            reject_nul,
                        )
                    },
                );
            }
        }
    }
}

#[test]
fn first_dotdot_prefixes_keep_utf16_short_prefix_and_root_rules() {
    for root in [
        "",
        "/",
        "//server/share/",
        "C:",
        "C:/",
        "http://host/",
        "file:///C:/",
        "^/",
    ] {
        for segment in ["", "a", "ab", "abc", "é", "日本", "😀"] {
            for suffix in [
                "/..",
                "/../",
                "/../a",
                "/../../b/",
                "/./../x",
                "//../x",
                "/../é/..",
                "/../😀/..",
            ] {
                let path = JsString::from(format!("{root}{segment}{suffix}"));
                for base in ["", "/base/dir", "relative", "C:/base", "file:///base/"] {
                    compare(
                        &format!("{path:?}, base={base}"),
                        || old_normalized_absolute_path(path.as_js(), base.into()),
                        || normalized_absolute_path(path.as_js(), base.into()),
                    );
                }
            }
            for surrogate in [0xd800, 0xdc00] {
                let mut path = JsString::from(root);
                path.push_code_unit(surrogate);
                path.push_str(segment);
                path.push_str("/../x/../");
                compare(
                    &format!("surrogate {path:?}"),
                    || old_normalized_absolute_path(path.as_js(), "".into()),
                    || normalized_absolute_path(path.as_js(), "".into()),
                );
            }
        }
    }
}

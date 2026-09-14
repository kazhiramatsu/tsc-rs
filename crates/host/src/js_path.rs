//! JavaScript values at a host boundary. No lexical normalization or I/O.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tsc_diagnostics::{JsStr, JsString};

use crate::{HostError, HostErrorKind, HostOperation};

/// The TypeScript filename case profile on arbitrary JS strings. Lone
/// surrogates remain unchanged and delimit scalar Unicode casing runs.
pub fn to_file_name_lower_case_js(path: JsStr<'_>) -> JsString {
    if let Some(path) = path.as_str() {
        return crate::to_file_name_lower_case(path).into();
    }
    let mut result = JsString::with_capacity(path.as_bytes().len());
    let mut scalar = String::new();
    for point in char::decode_utf16(path.code_units()) {
        match point {
            Ok(ch) => scalar.push(ch),
            Err(unit) => {
                result.push_str(&crate::to_file_name_lower_case(&scalar));
                scalar.clear();
                result.push_code_unit(unit.unpaired_surrogate());
            }
        }
    }
    result.push_str(&crate::to_file_name_lower_case(&scalar));
    result
}

pub(crate) fn validate(path: JsStr<'_>, operation: HostOperation) -> Result<(), HostError> {
    let detail = if path.is_empty() {
        Some("path is empty")
    } else if path.contains("\0") {
        Some("path contains a null character")
    } else {
        None
    };
    if let Some(detail) = detail {
        return Err(HostError::new_js(
            HostErrorKind::InvalidInput,
            operation,
            Some(path),
            detail,
        ));
    }
    Ok(())
}

/// This is an actual filesystem query boundary, never a compiler lookup key.
pub(crate) fn filesystem_path(
    path: JsStr<'_>,
    operation: HostOperation,
) -> Result<Cow<'_, Path>, HostError> {
    validate(path, operation)?;
    Ok(match path.as_str() {
        Some(path) => Cow::Borrowed(Path::new(path)),
        None => Cow::Owned(PathBuf::from(path.to_string_lossy().into_owned())),
    })
}

pub(crate) fn from_native(
    path: &Path,
    operation: HostOperation,
    kind: HostErrorKind,
) -> Result<JsString, HostError> {
    path.to_str().map(JsString::from).ok_or_else(|| {
        HostError::new(
            kind,
            operation,
            Some(path.to_owned()),
            "path is not representable as Unicode text",
        )
    })
}

/// A temporary, equally long UTF-8 spelling used only to ask std::path about
/// platform separators/components. A surrogate's three WTF-8 bytes become a
/// three-byte ordinary scalar; all punctuation and byte offsets are retained.
/// This value must never enter a lookup table, diagnostic or host query.
fn component_spelling(path: JsStr<'_>) -> Cow<'_, str> {
    match path.as_str() {
        Some(path) => Cow::Borrowed(path),
        None => Cow::Owned(
            char::decode_utf16(path.code_units())
                .map(|point| point.unwrap_or('\u{e000}'))
                .collect(),
        ),
    }
}

fn original_slice<'p>(original: JsStr<'p>, spelling: &str, part: &str) -> JsStr<'p> {
    let start = (part.as_ptr() as usize)
        .checked_sub(spelling.as_ptr() as usize)
        .expect("std::path returns a borrowed component slice");
    let (_, rest) = original
        .split_at_byte(start)
        .expect("path component boundary is canonical");
    rest.split_at_byte(part.len())
        .expect("path component boundary is canonical")
        .0
}

pub(crate) fn parent(path: JsStr<'_>) -> Option<JsStr<'_>> {
    let spelling = component_spelling(path);
    let parent = Path::new(spelling.as_ref())
        .parent()?
        .to_str()
        .expect("component spelling is Unicode");
    Some(original_slice(path, &spelling, parent))
}

pub(crate) fn child_name(path: JsStr<'_>) -> Option<JsStr<'_>> {
    let spelling = component_spelling(path);
    let native = Path::new(spelling.as_ref());
    let parent = native.parent()?;
    let name = native
        .strip_prefix(parent)
        .ok()?
        .to_str()
        .expect("component spelling is Unicode");
    Some(original_slice(path, &spelling, name))
}

pub(crate) fn join_observed_name(parent: JsStr<'_>, name: &str) -> JsString {
    let spelling = component_spelling(parent);
    let joined = Path::new(spelling.as_ref()).join(name);
    let joined = joined.to_str().expect("observed entry name is Unicode");
    let suffix = joined
        .strip_prefix(spelling.as_ref())
        .expect("relative entry name appends to the requested directory");
    let mut result = parent.to_owned();
    result.push_str(suffix);
    result
}

//! JavaScript value operations used before names or paths reach the host.

use crate::resolution_error::ResolutionError;
use tsc_diagnostics::{JsStr, JsString, JsStringByteLength};

const MIN_JS_REPLACEMENT_OUTPUT_BUDGET: usize = 1 << 20;
const MAX_JS_REPLACEMENT_OUTPUT_BUDGET: usize = 64 << 20;
const JS_REPLACEMENT_INPUT_MULTIPLIER: usize = 16;

/// getTypesPackageName / mangleScopedPackageName, _tsc.js:42070-42080.
/// Only the first slash is replaced; an @ name without a slash is unchanged.
pub(crate) fn types_package_name<'n>(package_name: impl Into<JsStr<'n>>) -> JsString {
    let package_name = package_name.into();
    let mut result = JsString::from("@types/");
    if let Some(scoped) = package_name.strip_prefix("@") {
        if let Some((scope, rest)) = scoped.split_once("/") {
            result.push_js(scope);
            result.push_str("__");
            result.push_js(rest);
            return result;
        }
    }
    result.push_js(package_name);
    result
}

/// JavaScript replacement-string semantics for `replaceFirstStar` and the
/// package-map `/\*/g` replacement. There are no capture groups, so only the
/// four context-independent/context tokens and `$$` are active; `$1` and
/// `$<name>` remain literal.
pub(crate) fn js_replace_first_star<'t, 'r>(
    target: impl Into<JsStr<'t>>,
    replacement: impl Into<JsStr<'r>>,
) -> Result<JsString, ResolutionError> {
    js_replace_stars(target.into(), replacement.into(), false)
}

pub(crate) fn js_replace_all_stars<'t, 'r>(
    target: impl Into<JsStr<'t>>,
    replacement: impl Into<JsStr<'r>>,
) -> Result<JsString, ResolutionError> {
    js_replace_stars(target.into(), replacement.into(), true)
}

/// Value-level String.replace for checker name construction. Like its other
/// string builders this uses ordinary allocation. Host resolver entry points
/// above retain their separate checked output budget and allocation failures;
/// both policies share the exact replacement-piece interpretation below.
pub fn replace_first_star_value<'t, 'r>(
    target: impl Into<JsStr<'t>>,
    replacement: impl Into<JsStr<'r>>,
) -> JsString {
    replace_stars_value(target.into(), replacement.into(), false)
}

/// The global-star variant used by package exports/imports name construction.
pub fn replace_all_stars_value<'t, 'r>(
    target: impl Into<JsStr<'t>>,
    replacement: impl Into<JsStr<'r>>,
) -> JsString {
    replace_stars_value(target.into(), replacement.into(), true)
}

fn replace_stars_value(target: JsStr<'_>, replacement: JsStr<'_>, all: bool) -> JsString {
    let mut result = JsString::new();
    let walked = for_each_js_star_piece(target, replacement, all, &mut |piece| {
        result.push_js(piece);
        Ok::<(), std::convert::Infallible>(())
    });
    match walked {
        Ok(()) => result,
        Err(never) => match never {},
    }
}

fn js_replace_stars(
    target: JsStr<'_>,
    replacement: JsStr<'_>,
    replace_all: bool,
) -> Result<JsString, ResolutionError> {
    let input_length = target
        .as_bytes()
        .len()
        .checked_add(replacement.as_bytes().len())
        .ok_or_else(|| {
            ResolutionError::resource_limit(
                "JavaScript star replacement input length overflowed usize",
            )
        })?;
    let output_budget = input_length
        .saturating_mul(JS_REPLACEMENT_INPUT_MULTIPLIER)
        .clamp(
            MIN_JS_REPLACEMENT_OUTPUT_BUDGET,
            MAX_JS_REPLACEMENT_OUTPUT_BUDGET,
        )
        .max(target.as_bytes().len());
    let output_length = if let (Some(target), Some(replacement)) =
        (target.as_str(), replacement.as_str())
    {
        // Preserve the existing linear preflight and exact budget diagnostics
        // on scalar inputs. Such pieces cannot form a new surrogate pair.
        js_star_replacement_output_length(target, replacement, replace_all).ok_or_else(|| {
            ResolutionError::resource_limit(
                "JavaScript star replacement output length overflowed usize",
            )
        })?
    } else {
        let mut length = JsStringByteLength::default();
        for_each_js_star_piece(target, replacement, replace_all, &mut |piece| {
            length.append(piece).ok_or_else(|| {
                ResolutionError::resource_limit(
                    "JavaScript star replacement output length overflowed usize",
                )
            })?;
            if length.bytes() > output_budget {
                // Canonical append never reduces total length. Stop before
                // allocating or traversing a superlinear expansion further.
                return Err(ResolutionError::resource_limit(format!(
                    "JavaScript star replacement exceeds the {output_budget}-byte output budget"
                )));
            }
            Ok(())
        })?;
        length.bytes()
    };
    if output_length > output_budget {
        return Err(ResolutionError::resource_limit(format!(
            "JavaScript star replacement would expand {input_length} input bytes to {output_length} bytes (budget {output_budget})"
        )));
    }
    let mut result = JsString::new();
    result.try_reserve_exact(output_length).map_err(|error| {
        ResolutionError::resource_limit(format!(
            "could not reserve {output_length} bytes for JavaScript star replacement: {error}"
        ))
    })?;
    for_each_js_star_piece(target, replacement, replace_all, &mut |piece| {
        result.push_js(piece);
        Ok::<(), ResolutionError>(())
    })?;
    debug_assert_eq!(result.as_bytes().len(), output_length);
    Ok(result)
}

/// Walk the replacement pieces in output order. Canonical byte accounting and
/// construction share this interpretation of dollar tokens.
fn for_each_js_star_piece<'a, E>(
    target: JsStr<'a>,
    replacement: JsStr<'a>,
    replace_all: bool,
    emit: &mut impl FnMut(JsStr<'a>) -> Result<(), E>,
) -> Result<(), E> {
    let mut rest = target;
    while let Some((before, after)) = rest.split_once("*") {
        emit(before)?;
        let star_byte = target.as_bytes().len() - after.as_bytes().len() - 1;
        let prefix = target
            .split_at_byte(star_byte)
            .expect("ASCII star is a whole code point")
            .0;
        for_each_js_replacement_piece(replacement, prefix, after, emit)?;
        rest = after;
        if !replace_all {
            break;
        }
    }
    emit(rest)
}

fn for_each_js_replacement_piece<'a, E>(
    replacement: JsStr<'a>,
    prefix: JsStr<'a>,
    suffix: JsStr<'a>,
    emit: &mut impl FnMut(JsStr<'a>) -> Result<(), E>,
) -> Result<(), E> {
    let mut rest = replacement;
    while let Some((before, after)) = rest.split_once("$") {
        emit(before)?;
        let (piece, token) = match after.as_bytes().first() {
            Some(b'$') => (JsStr::from_str("$"), "$"),
            Some(b'&') => (JsStr::from_str("*"), "&"),
            Some(b'`') => (prefix, "`"),
            Some(b'\'') => (suffix, "'"),
            _ => {
                emit(JsStr::from_str("$"))?;
                rest = after;
                continue;
            }
        };
        emit(piece)?;
        rest = after
            .strip_prefix(token)
            .expect("matched ASCII replacement token");
    }
    emit(rest)
}

fn js_star_replacement_output_length(
    target: &str,
    replacement: &str,
    replace_all: bool,
) -> Option<usize> {
    let replacement = js_star_replacement_length_summary(replacement)?;
    let mut output_length = 0_usize;
    let mut search_start = 0;
    let mut replaced = false;
    while let Some(relative_star) = target[search_start..].find('*') {
        let star = search_start + relative_star;
        output_length = output_length.checked_add(star - search_start)?;
        output_length = output_length
            .checked_add(replacement.expanded_length(star, target.len() - star - 1)?)?;
        search_start = star + 1;
        replaced = true;
        if !replace_all {
            break;
        }
    }
    if !replaced {
        return Some(target.len());
    }
    output_length.checked_add(target.len() - search_start)
}

#[derive(Clone, Copy)]
struct JsStarReplacementLengthSummary {
    fixed_length: usize,
    prefix_tokens: usize,
    suffix_tokens: usize,
}

impl JsStarReplacementLengthSummary {
    fn expanded_length(self, prefix_length: usize, suffix_length: usize) -> Option<usize> {
        self.fixed_length
            .checked_add(self.prefix_tokens.checked_mul(prefix_length)?)?
            .checked_add(self.suffix_tokens.checked_mul(suffix_length)?)
    }
}

fn js_star_replacement_length_summary(replacement: &str) -> Option<JsStarReplacementLengthSummary> {
    let mut summary = JsStarReplacementLengthSummary {
        fixed_length: 0,
        prefix_tokens: 0,
        suffix_tokens: 0,
    };
    let mut cursor = 0;
    while let Some(relative_dollar) = replacement[cursor..].find('$') {
        let dollar = cursor + relative_dollar;
        summary.fixed_length = summary.fixed_length.checked_add(dollar - cursor)?;
        let Some(token) = replacement.as_bytes().get(dollar + 1).copied() else {
            summary.fixed_length = summary.fixed_length.checked_add(1)?;
            cursor = dollar + 1;
            break;
        };
        match token {
            b'$' | b'&' => {
                summary.fixed_length = summary.fixed_length.checked_add(1)?;
            }
            b'`' => summary.prefix_tokens = summary.prefix_tokens.checked_add(1)?,
            b'\'' => summary.suffix_tokens = summary.suffix_tokens.checked_add(1)?,
            _ => {
                summary.fixed_length = summary.fixed_length.checked_add(1)?;
                cursor = dollar + 1;
                continue;
            }
        }
        cursor = dollar + 2;
    }
    summary.fixed_length = summary
        .fixed_length
        .checked_add(replacement.len() - cursor)?;
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_package_names_match_repeated_typescript_observations() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/utf16-package-names.json"))
                .unwrap();
        assert_eq!(fixture["repetitions"], 2);
        for case in fixture["cases"].as_array().unwrap() {
            assert_eq!(
                types_package_name(&value(&case["name_utf16"])),
                value(&case["types_package_utf16"]),
                "{}",
                case["case_id"],
            );
        }
    }

    fn value(units: &serde_json::Value) -> JsString {
        units
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn replacements_and_canonical_allocation_lengths_match_javascript() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/utf16-string-replacement.json"
        ))
        .unwrap();
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 14);
        for case in fixture["cases"].as_array().unwrap() {
            let target = value(&case["target"]);
            let replacement = value(&case["replacement"]);
            assert_eq!(
                replace_first_star_value(&target, &replacement),
                value(&case["first"]),
                "{}: value first",
                case["id"]
            );
            assert_eq!(
                replace_all_stars_value(&target, &replacement),
                value(&case["all"]),
                "{}: value all",
                case["id"]
            );
            for (name, actual) in [
                (
                    "first",
                    js_replace_first_star(&target, &replacement).unwrap(),
                ),
                ("all", js_replace_all_stars(&target, &replacement).unwrap()),
            ] {
                assert_eq!(actual, value(&case[name]), "{}: {name}", case["id"]);
                assert_eq!(
                    actual.as_bytes().len() as u64,
                    case[format!("{name}_bytes")].as_u64().unwrap()
                );
            }
        }
    }

    #[test]
    fn expansion_stays_within_the_existing_native_allocation_budget() {
        use crate::resolution_error::ResolutionErrorKind;
        let scalar = "*".repeat(4096);
        assert_eq!(
            js_replace_all_stars(&scalar, "$`").unwrap_err().kind(),
            ResolutionErrorKind::ResourceLimit
        );
        let mut non_scalar = JsString::from_code_units(&[0xd800]);
        non_scalar.push_str(&scalar);
        assert_eq!(
            js_replace_all_stars(&non_scalar, "$`").unwrap_err().kind(),
            ResolutionErrorKind::ResourceLimit
        );
    }
}

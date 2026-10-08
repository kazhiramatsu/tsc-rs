//! JavaScript value operations used before names or paths reach the host.

use tsc_diagnostics::{JsStr, JsString};

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

/// Value-level String.replace for checker name construction, with
/// JavaScript's replacement-string tokens (`$&`, `` $` ``, `$'`, `$$`). The
/// resolver replaces stars literally like tsgo (`strings.Replace`); this
/// token interpretation remains only for the checker's module specifier
/// names until that code follows tsgo too.
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

#[cfg(test)]
#[path = "../tests/unit/js_string_ops/tests.rs"]
mod tests;

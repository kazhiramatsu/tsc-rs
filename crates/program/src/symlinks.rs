//! Symlink facts discovered from a prepared program's resolutions — the
//! program-level half of upstream's `SymlinkCache` (`getSymlinkCache` in
//! program.ts builds it from every resolved module / type-reference directive
//! whose `originalPath` differs from its real `resolvedFileName`). Module
//! specifier generation consumes the facts through the emit host so a file
//! reached through a symlinked package directory is named by the link
//! (`package-a`) rather than by a relative path to its real location.

use std::collections::BTreeSet;
use tsc_diagnostics::{JsStr, JsString};

use crate::path::ProgramPath;
use crate::prepared::PreparedProgram;
use crate::resolution::ResolutionOutcome;

/// `(real_path, symlink_path)` pairs: every aliased file and every guessed
/// directory link, in the resolution table's key order (deterministic).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SymlinkFacts {
    pub files: Vec<(JsString, JsString)>,
    pub directories: Vec<(JsString, JsString)>,
}

/// tsc-port: createSymlinkCache @6.0.3
/// tsc-hash: afa07d0aa3b7d5178eb3e0994a08de727967a63fd718f127e975fab78a8e1833
/// tsc-span: _tsc.js:18331-18382
///
/// `setSymlinksFromResolutions` + `processResolution`: a resolution with an
/// `originalPath` records the file alias (keyed by the symlink spelling) and,
/// when `guessDirectorySymlink` finds a common tail, the directory link
/// (keyed by the symlink directory; ignored paths are skipped).
pub fn discover_symlink_facts(prepared: &PreparedProgram) -> SymlinkFacts {
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    let mut facts = SymlinkFacts::default();
    let mut seen_files: BTreeSet<JsString> = BTreeSet::new();
    let mut seen_directories: BTreeSet<JsString> = BTreeSet::new();
    let mut process = |resolved_file: &ProgramPath, original_path: Option<&ProgramPath>| {
        let Some(original_path) = original_path else {
            return;
        };
        let real = path_text(resolved_file);
        let symlink = path_text(original_path);
        if seen_files.insert(canonical(&symlink, case_sensitive)) {
            facts.files.push((real.clone(), symlink.clone()));
        }
        if let Some((common_real, common_symlink)) =
            guess_directory_symlink(&real, &symlink, case_sensitive)
        {
            if !contains_ignored_path(&common_symlink)
                && seen_directories.insert(canonical(&common_symlink, case_sensitive))
            {
                facts.directories.push((common_real, common_symlink));
            }
        }
    };
    for (_, resolution) in prepared.resolutions().modules() {
        if let ResolutionOutcome::Resolved(module) = resolution.outcome() {
            process(module.target().resolved_file(), module.original_path());
        }
    }
    for (_, resolution) in prepared.resolutions().type_references() {
        if let ResolutionOutcome::Resolved(directive) = resolution.outcome() {
            process(directive.target(), directive.original_path());
        }
    }
    // `getAllModulePathsWorker`'s prelude: the package scope's runtime
    // dependencies, resolved from `<package>/package.json`, feed the same
    // cache (`links.setSymlinksFromResolution`).
    for (resolved_file, original_path) in prepared.dependency_symlink_resolutions() {
        process(resolved_file, Some(original_path));
    }
    facts
}

fn path_text(path: &ProgramPath) -> JsString {
    let mut result = JsString::with_capacity(path.display().as_bytes().len());
    for unit in path.display().code_units() {
        result.push_code_unit(if unit == b'\\' as u16 {
            b'/' as u16
        } else {
            unit
        });
    }
    result
}

fn canonical<'p>(path: impl Into<JsStr<'p>>, case_sensitive: bool) -> JsString {
    let path = path.into();
    if case_sensitive {
        path.to_owned()
    } else {
        tsc_host::to_file_name_lower_case_js(path)
    }
}

/// tsc-port: guessDirectorySymlink @6.0.3
/// tsc-hash: b041a1ac530696abd871ba166b8bd58d5474a07e183151c8579cfeb87d04160c
/// tsc-span: _tsc.js:18383-18393
fn guess_directory_symlink<'p>(
    a: impl Into<JsStr<'p>>,
    b: impl Into<JsStr<'p>>,
    case_sensitive: bool,
) -> Option<(JsString, JsString)> {
    let a = a.into();
    let b = b.into();
    let mut a_parts = path_components(a);
    let mut b_parts = path_components(b);
    let mut is_directory = false;
    while a_parts.len() >= 2
        && b_parts.len() >= 2
        && !is_node_modules_or_scoped_package_directory(a_parts[a_parts.len() - 2], case_sensitive)
        && !is_node_modules_or_scoped_package_directory(b_parts[b_parts.len() - 2], case_sensitive)
        && canonical(a_parts[a_parts.len() - 1], case_sensitive)
            == canonical(b_parts[b_parts.len() - 1], case_sensitive)
    {
        a_parts.pop();
        b_parts.pop();
        is_directory = true;
    }
    is_directory.then(|| {
        (
            path_from_components(&a_parts),
            path_from_components(&b_parts),
        )
    })
}

/// tsc-port: isNodeModulesOrScopedPackageDirectory @6.0.3
/// tsc-hash: 209f33f48ff4e9442403b837b01f0281dbc6814b0019cdc767520b2576d21613
/// tsc-span: _tsc.js:18394-18396
fn is_node_modules_or_scoped_package_directory(segment: JsStr<'_>, case_sensitive: bool) -> bool {
    canonical(segment, case_sensitive) == "node_modules" || segment.starts_with("@")
}

/// tsc-port: containsIgnoredPath @6.0.3
/// tsc-hash: d87e642f05e79aae2abc944929c73d8dc32169211819b192e5dd6ad8428dff72
/// tsc-span: _tsc.js:19115-19117
fn contains_ignored_path<'p>(path: impl Into<JsStr<'p>>) -> bool {
    let path = path.into();
    ["/node_modules/.", "/.git", "/.#"]
        .iter()
        .any(|ignored| path.contains(ignored))
}

/// `getPathComponents` for a normalized absolute POSIX path: the root `"/"`
/// followed by the non-empty segments.
fn path_components(path: JsStr<'_>) -> Vec<JsStr<'_>> {
    let mut parts = vec![JsStr::from("/")];
    parts.extend(path.split_ascii(b'/').filter(|part| !part.is_empty()));
    parts
}

/// `getPathFromPathComponents`: the root plus the remaining segments.
fn path_from_components(parts: &[JsStr<'_>]) -> JsString {
    let mut result = JsString::from("/");
    for (index, part) in parts.iter().skip(1).enumerate() {
        if index != 0 {
            result.push_str("/");
        }
        result.push_js(*part);
    }
    result
}

#[cfg(test)]
#[path = "../tests/unit/symlinks/tests.rs"]
mod tests;

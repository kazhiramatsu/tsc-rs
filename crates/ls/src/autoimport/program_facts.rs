//! tsgo `compiler.Program`'s package facts the auto-import registry reads:
//! the packages a project's files import (`collectPackageNames` with
//! `getResolvedPackageNames`), the packages imported by a deep path that
//! has no `exports` map, and the outputs of the project's references.

use std::collections::BTreeSet;

use tsc_checker::program::ProgramFileId;
use tsc_checker::state::CheckerState;
use tsc_program::{CanonicalPath, PreparedProgram, ResolutionOutcome, ResolvedModuleTarget};
use tsc_types::JsStr;

use super::extract::source_file_of_module;
use super::registry::ProgramPackageFacts;
use crate::references::{collect_external_module_references, is_external_module_name_relative};
use crate::utilities::module_resolution_of_specifier;

/// tsgo `packageNamesInfo`.
#[derive(Default)]
struct PackageNames {
    resolved: BTreeSet<String>,
    unresolved: BTreeSet<String>,
    deep_import_packages: BTreeSet<String>,
}

/// What the registry reads of a project's program (tsgo
/// `getResolvedPackageNames`, `DeepImportPackageNames` and
/// `addProjectReferenceOutputMappings`).
pub fn program_package_facts(
    state: &mut CheckerState<'_>,
    prepared: &PreparedProgram,
) -> ProgramPackageFacts {
    let package_names = collect_package_names(state, prepared);
    // Normalize @types/ package names to their actual package names
    // (e.g., "@types/react" → "react"). Resolved package names can name the
    // @types package an import like "react" resolved to.
    let mut resolved_package_names: BTreeSet<String> = package_names
        .resolved
        .iter()
        .map(|name| tsc_program::get_package_name_from_types_package_name(name))
        .collect();
    for name in prepared.compiler_options().types.iter().flatten() {
        let name = name.to_string_lossy();
        if name != "*" {
            resolved_package_names
                .insert(tsc_program::get_package_name_from_types_package_name(&name));
        }
    }
    for name in &package_names.unresolved {
        let Some(symbol) = state.try_find_ambient_module(JsStr::from_str(name), true) else {
            continue;
        };
        let Some(file) = source_file_of_module(state, symbol) else {
            continue;
        };
        let declaring_file = state.binder.source(file).file_name.to_string_lossy();
        let package_name = tsc_program::get_package_name_from_directory(&declaring_file);
        if !package_name.is_empty() {
            resolved_package_names.insert(tsc_program::get_package_name_from_types_package_name(
                package_name,
            ));
        }
    }
    let project_reference_outputs = prepared
        .program_options()
        .project_references()
        .map(|references| {
            references
                .outputs_to_sources()
                .map(|(output, source)| {
                    (
                        output.as_js().to_string_lossy().into_owned(),
                        source.source().to_string_lossy().into_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    ProgramPackageFacts {
        resolved_package_names,
        deep_import_package_names: package_names.deep_import_packages,
        project_reference_outputs,
    }
}

/// tsgo `collectPackageNames`: the packages the program's own files import
/// (by package id name, else the name of the package.json above the
/// imported file, else its node_modules directory), the non-relative names
/// it imports that resolve to no file, and the packages without an
/// `exports` map imported by a deep path.
fn collect_package_names(state: &CheckerState<'_>, prepared: &PreparedProgram) -> PackageNames {
    let mut names = PackageNames::default();
    for file in 0..state.binder.file_count() {
        if state
            .binder
            .file_facts(ProgramFileId::from_raw(file as u32))
            .is_default_library()
        {
            continue;
        }
        let source = state.binder.source(file);
        let file_name = source.file_name.to_string_lossy();
        // Checking for /node_modules/ is a little imprecise, but ATA treats locally
        // installed typings as root files, which would not pass
        // IsSourceFileFromExternalLibrary.
        if file_name.to_lowercase().contains("/node_modules/")
            || is_source_file_from_external_library(prepared, &file_name)
        {
            continue;
        }
        for import in collect_external_module_references(source).imports {
            let tsc_syntax::NodeData::StringLiteral(literal) = &source.arena.node(import).data
            else {
                continue;
            };
            let text = literal.text.to_string_lossy().into_owned();
            let parent = source.arena.node(import).parent;
            if parent.is_some_and(|parent| {
                tsc_syntax::utilities::is_source_phase_import(&source.arena, parent)
            }) || is_external_module_name_relative(&literal.text)
            {
                continue;
            }
            let resolution = module_resolution_of_specifier(state, prepared, source, import, &text);
            if let Some(ResolutionOutcome::Resolved(module)) =
                resolution.map(|resolution| resolution.outcome())
            {
                if !module.is_external_library_import() {
                    continue;
                }
                let resolved_file_name = module
                    .target()
                    .resolved_file()
                    .display()
                    .to_string_lossy()
                    .into_owned();
                // Priority order for getting package name:
                // 1. PackageId.Name (requires both name and version in package.json)
                let mut name = module
                    .package_id()
                    .map(|package_id| package_id.name().to_string_lossy().into_owned())
                    .unwrap_or_default();
                let scope = package_scope(prepared, module.target());
                if name.is_empty() {
                    // 2. the name of the package.json in the package directory
                    if let Some(scope_name) = scope.as_ref().and_then(|scope| scope.0.clone()) {
                        name = scope_name;
                    }
                }
                if name.is_empty() {
                    // 3. the package of its node_modules path
                    name = tsc_program::get_package_name_from_directory(&resolved_file_name)
                        .to_owned();
                }
                // 4. If all fail, don't add empty string
                if !name.is_empty() {
                    // Detect deep imports: subpath imports in packages without exports.
                    // These are imports like "lodash/fp" where the package has no exports
                    // map, so auto-import can only find them via recursive directory search.
                    let (_, rest) = tsc_program::parse_package_name(&text);
                    if !rest.is_empty() && scope.is_some_and(|scope| !scope.1) {
                        names
                            .deep_import_packages
                            .insert(tsc_program::get_package_name_from_types_package_name(&name));
                    }
                    names.resolved.insert(name);
                }
                continue;
            }
            names.unresolved.insert(text);
        }
    }
    names
}

/// tsgo `IsSourceFileFromExternalLibrary`: a file found searching
/// node_modules.
fn is_source_file_from_external_library(prepared: &PreparedProgram, file_name: &str) -> bool {
    CanonicalPath::from_js_normalized(JsStr::from_str(&crate::utilities::path_key(
        prepared, file_name,
    )))
    .ok()
    .and_then(|path| prepared.source_id(&path))
    .and_then(|id| prepared.source_file(id))
    .is_some_and(|source| source.found_searching_node_modules())
}

/// tsgo `GetPackageScopeForPath` of a resolved module's directory: the
/// package.json above the module's file the program read, its name and
/// whether it has an `exports` field.
fn package_scope(
    prepared: &PreparedProgram,
    target: &ResolvedModuleTarget,
) -> Option<(Option<String>, bool)> {
    let package_json = match target {
        ResolvedModuleTarget::Source { source, .. } => prepared
            .source_file(*source)
            .and_then(|source| source.package_scope())
            .cloned(),
        ResolvedModuleTarget::Unloaded { resolved_file, .. } => {
            let mut directory = tsc_program::get_directory_path(resolved_file.display());
            loop {
                let candidate =
                    tsc_program::combine_paths(directory.as_js(), JsStr::from_str("package.json"));
                let found = CanonicalPath::from_js_normalized(JsStr::from_str(
                    &crate::utilities::path_key(prepared, &candidate.to_string_lossy()),
                ))
                .ok()
                .filter(|path| prepared.package(path).is_some());
                if found.is_some() {
                    break found;
                }
                let parent = tsc_program::get_directory_path(directory.as_js());
                if parent == directory {
                    break None;
                }
                directory = parent;
            }
        }
    }?;
    let metadata = prepared.package(&package_json)?;
    let has_exports = serde_json::from_str::<serde_json::Value>(metadata.text())
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .map(|object| object.contains_key("exports"))
        })
        .unwrap_or(false);
    Some((
        metadata
            .name()
            .map(|name| name.to_string_lossy().into_owned()),
        has_exports,
    ))
}

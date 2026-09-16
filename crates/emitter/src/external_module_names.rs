use tsc_diagnostics::{JsStr, JsString};
use tsc_syntax::SourceFile;

use crate::source_map::paths;
use crate::{
    EmitHost, EmitResolver, EmitResolverError, EmitResolverMethod, TransformArena, TransformError,
    TransformNode,
};

/// tsc-port: getResolvedExternalModuleName @6.0.3
/// tsc-hash: d563d5806a3c929b098ad52a2d31b607624d9b26e9bb6ed5271e84c25903fafa
/// tsc-span: _tsc.js:16535-16537
pub(crate) fn get_resolved_external_module_name(
    host: &dyn EmitHost,
    file: &SourceFile,
    reference_file: Option<&SourceFile>,
) -> JsString {
    file.module_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || {
                external_module_name_from_path(
                    host,
                    file.file_name.as_js(),
                    reference_file.map(|file| file.file_name.as_js()),
                )
            },
            JsString::from,
        )
}

/// tsc-port: getExternalModuleNameFromPath @6.0.3
/// tsc-hash: 6ba63b23b3a5d9605a52edf790695293b2935f2922082d0d816ab9b193bcca35
/// tsc-span: _tsc.js:16552-16566
fn external_module_name_from_path(
    host: &dyn EmitHost,
    file_name: JsStr<'_>,
    reference_path: Option<JsStr<'_>>,
) -> JsString {
    let cwd = paths::normalize_slashes(host.current_directory());
    let directory = reference_path.map_or_else(
        || paths::normalize_slashes(host.common_source_directory()),
        paths::directory_path,
    );
    let directory = paths::get_normalized_absolute_path(&directory, &cwd);
    let directory = if host.use_case_sensitive_file_names() {
        directory
    } else {
        paths::to_file_name_lower_case(&directory)
    };
    let file_path = paths::get_normalized_absolute_path(file_name, &cwd);
    let relative = paths::get_relative_path_to_directory_or_url(
        &directory,
        &file_path,
        &directory,
        host.use_case_sensitive_file_names(),
        false,
    );
    let extensionless = paths::remove_file_extension(&relative);
    let relative_name = extensionless == "."
        || extensionless == ".."
        || extensionless.starts_with("./")
        || extensionless.starts_with("../");
    if reference_path.is_some() && paths::get_root_length(&extensionless) == 0 && !relative_name {
        let mut name = JsString::from("./");
        name.push_js(extensionless.as_js());
        name
    } else {
        extensionless
    }
}

/// tsc-port: tryRenameExternalModule @6.0.3
/// tsc-hash: 1b0c9a3e5d7f2b4c6a8e0d2f4b6a8c0e2d4f6a8b0c2e4d6f8a0b2c4d6e8f0a2c
/// tsc-span: _tsc.js:27720-27723
///
/// API-supplied `renamedDependencies` rewrite a module specifier only when no
/// resolved-file name (outFile / explicit moduleName) claimed it first.
pub(crate) fn try_rename_external_module(
    source: &SourceFile,
    module_name: JsStr<'_>,
) -> Option<JsString> {
    source
        .renamed_dependencies
        .iter()
        .find(|(from, _)| JsStr::from_str(from) == module_name)
        .map(|(_, to)| JsString::from(to.as_str()))
}

/// tsc-port: tryGetModuleNameFromFile @6.0.3
/// tsc-hash: 303add929c042f86af9ebd622cf3c6374a0dedeb0a3b3a9e24d636e90f1b84fa
/// tsc-span: _tsc.js:27724-27735
pub(crate) fn try_get_module_name_from_file(
    host: Option<&dyn EmitHost>,
    file: &SourceFile,
) -> Option<JsString> {
    if let Some(name) = file.module_name.as_deref().filter(|name| !name.is_empty()) {
        return Some(name.into());
    }
    let host = host?;
    (!file.is_declaration_file
        && host
            .compiler_options()
            .out_file
            .as_ref()
            .is_some_and(|name| !name.is_empty()))
    .then(|| get_resolved_external_module_name(host, file, None))
}

/// Resolved-file branch of getExternalModuleNameLiteral. Host API
/// renamedDependencies is a separate observation-only reference boundary.
pub(crate) fn resolved_external_module_name_literal(
    host: Option<&dyn EmitHost>,
    resolver: &dyn EmitResolver,
    arena: &TransformArena,
    declaration: TransformNode,
) -> Result<Option<JsString>, TransformError> {
    let Some(host) = host else { return Ok(None) };
    let out_file = host
        .compiler_options()
        .out_file
        .as_ref()
        .is_some_and(|name| !name.is_empty());
    let named_source = host.source_file_ids().iter().copied().any(|source| {
        host.source_file(source)
            .and_then(|source| source.syntax())
            .is_some_and(|source| {
                source
                    .module_name
                    .as_deref()
                    .is_some_and(|name| !name.is_empty())
            })
    });
    if !out_file && !named_source {
        return Ok(None);
    }
    let declaration = arena.get_original_node(declaration);
    let Some(declaration) = arena.parse_tree_resolver_node(declaration)? else {
        return Ok(None);
    };
    let target = match resolver.get_external_module_file_from_declaration(declaration) {
        Ok(target) => target,
        // Retain the pre-bundle optional explicit-name protocol. Bundle
        // dependency identities require the real checker answer.
        Err(EmitResolverError::Unavailable {
            method: EmitResolverMethod::GetExternalModuleFileFromDeclaration,
            ..
        }) if !out_file => None,
        Err(error) => return Err(error.into()),
    };
    let Some(target) = target else {
        return Ok(None);
    };
    let file = host
        .source_file(target.source())
        .and_then(|file| file.syntax())
        .ok_or(EmitResolverError::UnknownSource {
            method: EmitResolverMethod::GetExternalModuleFileFromDeclaration,
            node: target,
        })?;
    Ok(try_get_module_name_from_file(Some(host), file))
}

#[cfg(test)]
#[path = "../tests/unit/external_module_names/tests.rs"]
mod tests;

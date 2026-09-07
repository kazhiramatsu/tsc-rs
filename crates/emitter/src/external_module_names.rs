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
) -> String {
    file.module_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || {
                external_module_name_from_path(
                    host,
                    &file.file_name,
                    reference_file.map(|file| file.file_name.as_str()),
                )
            },
            str::to_owned,
        )
}

/// tsc-port: getExternalModuleNameFromPath @6.0.3
/// tsc-hash: 6ba63b23b3a5d9605a52edf790695293b2935f2922082d0d816ab9b193bcca35
/// tsc-span: _tsc.js:16552-16566
fn external_module_name_from_path(
    host: &dyn EmitHost,
    file_name: &str,
    reference_path: Option<&str>,
) -> String {
    let cwd = paths::normalize_slashes(&host.current_directory().to_string_lossy());
    let directory = reference_path.map_or_else(
        || paths::normalize_slashes(&host.common_source_directory().to_string_lossy()),
        |path| {
            let path = paths::normalize_slashes(path);
            let root_length = paths::get_root_length(&path);
            if root_length == path.len() {
                return path;
            }
            let path = path.strip_suffix('/').unwrap_or(&path);
            path[..root_length.max(path.rfind('/').unwrap_or(0))].to_owned()
        },
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
    // removeFileExtension, including its strict length check and ordered
    // declaration extensions. Unknown and uppercase extensions are retained.
    let extensionless = [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ]
    .into_iter()
    .find_map(|extension| {
        (relative.len() > extension.len())
            .then(|| relative.strip_suffix(extension))
            .flatten()
    })
    .unwrap_or(&relative);
    let relative_name = extensionless == "."
        || extensionless == ".."
        || extensionless.starts_with("./")
        || extensionless.starts_with("../");
    if reference_path.is_some() && paths::get_root_length(extensionless) == 0 && !relative_name {
        format!("./{extensionless}")
    } else {
        extensionless.to_owned()
    }
}

/// tsc-port: tryGetModuleNameFromFile @6.0.3
/// tsc-hash: 303add929c042f86af9ebd622cf3c6374a0dedeb0a3b3a9e24d636e90f1b84fa
/// tsc-span: _tsc.js:27724-27735
pub(crate) fn try_get_module_name_from_file(
    host: Option<&dyn EmitHost>,
    file: &SourceFile,
) -> Option<String> {
    if let Some(name) = file.module_name.as_deref().filter(|name| !name.is_empty()) {
        return Some(name.to_owned());
    }
    let host = host?;
    (!file.is_declaration_file
        && host
            .compiler_options()
            .out_file
            .as_deref()
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
) -> Result<Option<String>, TransformError> {
    let Some(host) = host else { return Ok(None) };
    let out_file = host
        .compiler_options()
        .out_file
        .as_deref()
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

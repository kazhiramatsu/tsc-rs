use tsc_diagnostics::{JsStr, JsString};
use tsc_syntax::SourceFile;

use crate::source_map::paths;
use crate::EmitHost;

/// tsc-port: getResolvedExternalModuleName @6.0.3
/// tsc-hash: d563d5806a3c929b098ad52a2d31b607624d9b26e9bb6ed5271e84c25903fafa
/// tsc-span: _tsc.js:16535-16537
pub(crate) fn get_resolved_external_module_name(
    host: &dyn EmitHost,
    file: &SourceFile,
    reference_file: Option<&SourceFile>,
) -> JsString {
    file.module_name
        .as_ref()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || {
                external_module_name_from_path(
                    host,
                    file.file_name.as_js(),
                    reference_file.map(|file| file.file_name.as_js()),
                )
            },
            Clone::clone,
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
/// tsc-hash: 86fd395bde1284aad3de45da47e7de1f6bdafb89d9173fbbd246cc1891395e52
/// tsc-span: _tsc.js:27720-27723
///
/// API-supplied `renamedDependencies` rewrite a module specifier (tsgo's
/// tryGetModuleNameFromFile names no file first).
pub(crate) fn try_rename_external_module(
    source: &SourceFile,
    module_name: JsStr<'_>,
) -> Option<JsString> {
    source
        .renamed_dependencies
        .iter()
        .rev()
        .find(|(from, _)| from.as_js() == module_name)
        .filter(|(_, to)| !to.is_empty())
        .map(|(_, to)| to.clone())
}

#[cfg(test)]
#[path = "../tests/unit/external_module_names/tests.rs"]
mod tests;

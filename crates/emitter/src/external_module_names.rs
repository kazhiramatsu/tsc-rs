use tsc_diagnostics::{JsStr, JsString};
use tsc_syntax::SourceFile;

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

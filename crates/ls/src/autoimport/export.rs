//! tsgo `ls/autoimport/export.go`: what the registry knows of a module's
//! export — its module and name, the syntax that exports it, its flags, the
//! export an alias resolves to, and how a completion shows it.

use tsc_types::{InternalSymbolName, SymbolFlags};

use crate::lsutil::{ScriptElementKind, ScriptElementKindModifier};

/// tsgo `ModuleID`: a file module (by path) or an ambient module (by name).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ModuleId {
    File(String),
    Ambient(String),
}

impl ModuleId {
    /// tsgo `ModuleID.AsString`.
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::File(path) | Self::Ambient(path) => path,
        }
    }

    /// tsgo `ModuleID.AsPathKey`.
    pub(crate) fn as_path(&self) -> Option<&str> {
        match self {
            Self::File(path) => Some(path),
            Self::Ambient(_) => None,
        }
    }

    /// tsgo `ModuleID.AsModuleSpecifier`.
    pub(crate) fn as_module_specifier(&self) -> Option<&str> {
        match self {
            Self::Ambient(specifier) => Some(specifier),
            Self::File(_) => None,
        }
    }
}

/// tsgo `ExportID`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ExportId {
    pub(crate) module_id: ModuleId,
    pub(crate) export_name: String,
}

/// tsgo `ExportSyntax`, in tsgo's order (the grouping keeps the least).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ExportSyntax {
    None,
    /// `export const x = {}`
    Modifier,
    /// `export { x }`
    Named,
    /// `export default function f() {}`
    DefaultModifier,
    /// `export default f`
    DefaultDeclaration,
    /// `export = x`
    Equals,
    /// `export as namespace x`
    Umd,
    /// `export * from "module"`
    Star,
    /// `module.exports = {}`
    CommonJsModuleExports,
    /// `exports.x = {}`
    CommonJsExportsProperty,
}

/// tsgo `Export`.
#[derive(Clone, Debug)]
pub(crate) struct Export {
    pub(crate) id: ExportId,
    pub(crate) module_file_name: String,
    pub(crate) unresolved_module_specifier: String,
    pub(crate) syntax: ExportSyntax,
    pub(crate) flags: SymbolFlags,
    pub(crate) local_name: String,
    /// The module symbol's export this export was found on: `export=`,
    /// `__export`, or none.
    pub(crate) through: String,
    /// The export an alias resolves to (tsgo's zero `ExportID` when none).
    pub(crate) target: Option<ExportId>,
    pub(crate) is_type_only: bool,
    pub(crate) script_element_kind: ScriptElementKind,
    pub(crate) script_element_kind_modifiers: ScriptElementKindModifier,
    /// The file where the export was found.
    pub(crate) path: String,
    pub(crate) package_name: String,
}

impl Export {
    /// tsgo `Export.Name`.
    pub(crate) fn name(&self) -> &str {
        if !self.local_name.is_empty() {
            return &self.local_name;
        }
        if self.id.export_name == InternalSymbolName::EXPORT_EQUALS {
            return self
                .target
                .as_ref()
                .map_or("", |target| target.export_name.as_str());
        }
        &self.id.export_name
    }

    /// tsgo `Export.IsRenameable`.
    pub(crate) fn is_renameable(&self) -> bool {
        self.id.export_name == InternalSymbolName::EXPORT_EQUALS
            || self.id.export_name == InternalSymbolName::DEFAULT
    }

    /// tsgo `Export.AmbientModuleName`.
    pub(crate) fn ambient_module_name(&self) -> &str {
        self.id.module_id.as_module_specifier().unwrap_or("")
    }

    /// tsgo `Export.IsUnresolvedAlias`.
    pub(crate) fn is_unresolved_alias(&self) -> bool {
        self.flags == SymbolFlags::ALIAS
    }

    /// The export's target, or itself (tsgo's grouping key).
    pub(crate) fn target_or_self(&self) -> &ExportId {
        self.target.as_ref().unwrap_or(&self.id)
    }
}

//! M4 5.8d: the module/alias/import-export band (m4-58 §8-§9) — the
//! resolveAlias protocol + per-kind alias targets, external-module
//! resolution (the 2307 band), module symbol resolution (export=
//! chase), and the module exports worker (export-star + the 2308
//! ambiguity row).
//!
//! Mode machinery (impliedNodeFormat / Node16..NodeNext arms /
//! resolution-mode overrides) preserves CommonJS / ESNext / unknown
//! through the resolver. The program-layer resolver
//! (`resolve_program_module`) is the host.getResolvedModule seam:
//! tsrs-native over the in-memory file set per program-and-modules.md
//! §2 (no node_modules; package.json `type` is interpreted for implied
//! Node format, while exports/imports resolution remains suppressed).

use tsrs2_binder::{node_util, SymbolId, SymbolTable};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{
    escape_leading_underscores, unescape_leading_underscores, NodeData, NodeId, SyntaxKind,
};
use tsrs2_types::{CheckMode, InternalSymbolName, ObjectFlags, SymbolFlags, TypeFlags};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, Unsupported};
use tsrs2_types::TypeId;

/// The export-star collision tracker (getExportsOfModuleWorker's
/// lookupTable): name -> (first specifier text, exportsWithDuplicate).
type ExportLookupTable = indexmap::IndexMap<String, (String, Vec<NodeId>)>;

/// The program resolver's verdict (the host.getResolvedModule seam).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedProgramModule {
    pub file_index: usize,
    /// tsc resolvedUsingTsExtension: the specifier itself carried the
    /// TS-family extension and resolved as written.
    pub resolved_using_ts_extension: bool,
    /// The resolved file ends `.tsx` (getResolutionDiagnostic's jsx
    /// row is the only extension read the modeled subset needs).
    pub is_tsx: bool,
}

/// The resolver's three-way verdict: `Suppressed` marks a miss that
/// unmodeled machinery (node_modules, baseUrl/paths, allowJs targets,
/// mode-dependent extensions) might turn into a hit for tsc — the
/// error tail stays silent (FN-side) instead of fabricating 2307.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ProgramModuleResolution {
    Resolved(ResolvedProgramModule),
    Suppressed,
    Missed,
}

/// tsc ResolutionMode at the host-resolution seam. `Unknown` remains
/// observable outside contexts whose emit syntax can be determined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModuleResolutionMode {
    CommonJs,
    EsNext,
    Unknown,
}

pub(crate) const EMIT_HELPER_DECORATE: u32 = 1 << 3;
pub(crate) const EMIT_HELPER_READ: u32 = 1 << 9;
pub(crate) const EMIT_HELPER_SPREAD_ARRAY: u32 = 1 << 10;
pub(crate) const EMIT_HELPER_EXPORT_STAR: u32 = 1 << 15;
pub(crate) const EMIT_HELPER_IMPORT_STAR: u32 = 1 << 16;
pub(crate) const EMIT_HELPER_IMPORT_DEFAULT: u32 = 1 << 17;
pub(crate) const EMIT_HELPER_SET_FUNCTION_NAME: u32 = 1 << 22;
pub(crate) const EMIT_HELPER_PROP_KEY: u32 = 1 << 23;
pub(crate) const EMIT_HELPER_ADD_DISPOSABLE_RESOURCE_AND_DISPOSE_RESOURCES: u32 = 1 << 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolutionModeOverrideParse {
    Missing,
    WrongCardinality { token: SyntaxKind },
    InvalidName { token: SyntaxKind, name: NodeId },
    InvalidValue { value: NodeId },
    Valid(ModuleResolutionMode),
}

impl<'a> CheckerState<'a> {
    // ================================================================
    // §9 alias protocol (resolveAlias family)
    // ================================================================

    /// tsc-port: isNonLocalAlias @6.0.3
    /// tsc-hash: 7e19d14548da4892d5858091bba01bccd5bb2ee1bc81b347f96613c586307b10
    /// tsc-span: _tsc.js:49109-49112
    pub(crate) fn is_non_local_alias(
        &self,
        symbol: Option<SymbolId>,
        excludes: SymbolFlags,
    ) -> bool {
        let Some(symbol) = symbol else {
            return false;
        };
        let flags = self.binder.symbol(symbol).flags;
        (flags & (SymbolFlags::ALIAS | excludes)) == SymbolFlags::ALIAS
            || (flags.intersects(SymbolFlags::ALIAS) && flags.intersects(SymbolFlags::ASSIGNMENT))
    }

    fn default_alias_excludes() -> SymbolFlags {
        SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE
    }

    /// tsc-port: resolveSymbol @6.0.3
    /// tsc-hash: c27aacb412ed9829936f7dd51cc280685d895afc381326591d762898a133a48c
    /// tsc-span: _tsc.js:49113-49115
    pub(crate) fn resolve_symbol_ex(
        &mut self,
        symbol: Option<SymbolId>,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        if !dont_resolve_alias && self.is_non_local_alias(symbol, Self::default_alias_excludes()) {
            Ok(Some(
                self.resolve_alias(symbol.expect("non-local alias is Some"))?,
            ))
        } else {
            Ok(symbol)
        }
    }

    /// tsc-port: resolveAlias @6.0.3
    /// tsc-hash: 7216b589d571727923ff9e9abac7f5ea468fc3b1a53cbb1d83257d47b21632b9
    /// tsc-span: _tsc.js:49116-49133
    ///
    /// SymbolLinks.alias_target protocol (the resolvedSignature twin):
    /// Vacant→Resolving on entry, Resolving→Resolved for the tail
    /// write AND the sentinel-on-entry cycle collapse; a re-entrant
    /// Resolved observed by the outer frame reports 5303 without
    /// writing. Err-unwind reverts the sentinel this frame wrote.
    pub(crate) fn resolve_alias(&mut self, symbol: SymbolId) -> CheckResult2<SymbolId> {
        debug_assert!(
            self.binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::ALIAS),
            "Should only get Alias here."
        );
        match self.links.symbol(symbol).alias_target {
            LinkSlot::Resolved(target) => return Ok(target),
            LinkSlot::Resolving => {
                // Sentinel found ON ENTRY: cycle collapse to unknown.
                let unknown = self.unknown_symbol;
                self.links.set_symbol_alias_target(
                    self.speculation_depth,
                    symbol,
                    LinkSlot::Resolved(unknown),
                );
                return Ok(unknown);
            }
            LinkSlot::Vacant => {}
        }
        self.links
            .set_symbol_alias_target(self.speculation_depth, symbol, LinkSlot::Resolving);
        let Some(node) = self.get_declaration_of_alias_symbol(symbol) else {
            // tsc Debug.fail() — an Alias symbol always has an alias
            // declaration; a recovery shape without one contains.
            self.links.revert_symbol_alias_target(symbol);
            return Err(Unsupported::new(
                "resolveAlias alias symbol without alias declaration (parse-recovery shape, tsc Debug.fail)",
            ));
        };
        let target = match self.get_target_of_alias_declaration(node, false) {
            Ok(target) => target,
            Err(unsupported) => {
                self.links.revert_symbol_alias_target(symbol);
                return Err(unsupported);
            }
        };
        if self.links.symbol(symbol).alias_target.is_resolving() {
            let resolved = target.unwrap_or(self.unknown_symbol);
            self.links.set_symbol_alias_target(
                self.speculation_depth,
                symbol,
                LinkSlot::Resolved(resolved),
            );
        } else {
            // A re-entrant frame resolved (or collapsed) the slot
            // while our getTargetOfAliasDeclaration ran.
            let name = self.symbol_display_name(symbol);
            self.error_at(
                Some(node),
                &diagnostics::Circular_definition_of_import_alias_0,
                &[&name],
            );
        }
        match self.links.symbol(symbol).alias_target {
            LinkSlot::Resolved(resolved) => Ok(resolved),
            _ => unreachable!("resolveAlias tail leaves the slot Resolved"),
        }
    }

    /// tsc-port: tryResolveAlias @6.0.3
    /// tsc-hash: bab6b09fe2dcce72699b3e3f0194e26d2f371b115c231c9a18e46b2bd6d81c8f
    /// tsc-span: _tsc.js:49134-49140
    pub(crate) fn try_resolve_alias(&mut self, symbol: SymbolId) -> CheckResult2<Option<SymbolId>> {
        if self.links.symbol(symbol).alias_target.is_resolving() {
            return Ok(None);
        }
        Ok(Some(self.resolve_alias(symbol)?))
    }

    /// tsc-port: getTargetOfAliasDeclaration @6.0.3
    /// tsc-hash: 162af5ad124b130bc6f80f131212585721d1d34d502fc735b9eaea94780b01d0
    /// tsc-span: _tsc.js:49071-49108
    ///
    /// TS core kinds; the JS arms (VariableDeclaration/BindingElement
    /// require shapes, Property/ShorthandPropertyAssignment, access
    /// expressions, BinaryExpression module.exports) are unreachable
    /// through isAliasSymbolDeclaration's TS arms — constant None here
    /// (tsc Debug.fail on unknown kinds).
    fn get_target_of_alias_declaration(
        &mut self,
        node: NodeId,
        dont_recursively_resolve: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        match self.kind_of(node) {
            SyntaxKind::ImportEqualsDeclaration => {
                self.get_target_of_import_equals_declaration(node, dont_recursively_resolve)
            }
            SyntaxKind::ImportClause => {
                self.get_target_of_import_clause(node, dont_recursively_resolve)
            }
            SyntaxKind::NamespaceImport => {
                self.get_target_of_namespace_import(node, dont_recursively_resolve)
            }
            SyntaxKind::NamespaceExport => {
                self.get_target_of_namespace_export(node, dont_recursively_resolve)
            }
            SyntaxKind::ImportSpecifier => {
                self.get_target_of_import_specifier(node, dont_recursively_resolve)
            }
            SyntaxKind::ExportSpecifier => self.get_target_of_export_specifier(
                node,
                SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE,
                dont_recursively_resolve,
            ),
            SyntaxKind::ExportAssignment => {
                self.get_target_of_export_assignment(node, dont_recursively_resolve)
            }
            SyntaxKind::NamespaceExportDeclaration => {
                self.get_target_of_namespace_export_declaration(node, dont_recursively_resolve)
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: getTargetOfImportEqualsDeclaration @6.0.3
    /// tsc-hash: 0ef78eb1ab67c6129a32b5f56cb806263aec483a5ffbd38ecf3096c5a8b0b81a
    /// tsc-span: _tsc.js:48504-48534
    ///
    /// The commonJSPropertyAccess/VariableDeclaration require arms are
    /// JS-only (constant-dead in TS files); the Node20+ module.exports
    /// arm is mode machinery (dead at the modeled defaults).
    fn get_target_of_import_equals_declaration(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let module_reference = match self.data_of(node) {
            NodeData::ImportEqualsDeclaration(data) => data.module_reference,
            _ => None,
        };
        let Some(module_reference) = module_reference else {
            return Ok(None);
        };
        if self.kind_of(module_reference) == SyntaxKind::ExternalModuleReference {
            let expression = match self.data_of(module_reference) {
                NodeData::ExternalModuleReference(data) => data.expression,
                _ => None,
            };
            let Some(expression) = expression else {
                return Ok(None);
            };
            let immediate = self.resolve_external_module_name(node, expression, false)?;
            let resolved = self.resolve_external_module_symbol(immediate, false)?;
            // 48516-48521: under node20..nodenext, `import x =
            // require(esm)` targets the module's `"module.exports"`
            // named export when one exists.
            if let Some(resolved_symbol) = resolved {
                let module_kind = self.options.emit_module_kind();
                if (102..=199).contains(&module_kind) {
                    let module_exports = self.get_export_of_module(
                        resolved_symbol,
                        "module.exports",
                        node,
                        dont_resolve_alias,
                    )?;
                    if module_exports.is_some() {
                        return Ok(module_exports);
                    }
                }
            }
            self.mark_symbol_of_alias_declaration_if_type_only(
                Some(node),
                immediate,
                resolved,
                /*overwrite_empty*/ false,
                None,
                None,
            )?;
            return Ok(resolved);
        }
        let resolved = self.get_symbol_of_part_of_right_hand_side_of_import_equals(
            module_reference,
            dont_resolve_alias,
        )?;
        self.check_and_report_error_for_resolving_import_alias_to_type_only_symbol(node, resolved)?;
        Ok(resolved)
    }

    /// tsc-port: checkAndReportErrorForResolvingImportAliasToTypeOnlySymbol @6.0.3
    /// tsc-hash: e1b0ad5d2be45668be05d26fbea30e9092a36523a0d41ce295beedef29b28517
    /// tsc-span: _tsc.js:48535-48551
    fn check_and_report_error_for_resolving_import_alias_to_type_only_symbol(
        &mut self,
        node: NodeId,
        resolved: Option<SymbolId>,
    ) -> CheckResult2<()> {
        let marked = self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            /*immediate_target*/ None,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        let is_type_only = match self.data_of(node) {
            NodeData::ImportEqualsDeclaration(data) => data.is_type_only,
            _ => false,
        };
        if !marked || is_type_only {
            return Ok(());
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        let Some(type_only_declaration) = self.get_type_only_alias_declaration(symbol)? else {
            return Ok(());
        };
        let is_export = matches!(
            self.kind_of(type_only_declaration),
            SyntaxKind::ExportSpecifier | SyntaxKind::ExportDeclaration
        );
        let message = if is_export {
            &diagnostics::An_import_alias_cannot_reference_a_declaration_that_was_exported_using_export_type
        } else {
            &diagnostics::An_import_alias_cannot_reference_a_declaration_that_was_imported_using_import_type
        };
        let related_message = if is_export {
            &diagnostics::_0_was_exported_here
        } else {
            &diagnostics::_0_was_imported_here
        };
        let name = if self.kind_of(type_only_declaration) == SyntaxKind::ExportDeclaration {
            "*".to_owned()
        } else {
            let decl_name = match self.data_of(type_only_declaration) {
                NodeData::ImportSpecifier(data) => data.name,
                NodeData::ExportSpecifier(data) => data.name,
                NodeData::ImportClause(data) => data.name,
                NodeData::NamespaceImport(data) => data.name,
                NodeData::NamespaceExport(data) => data.name,
                NodeData::ImportEqualsDeclaration(data) => data.name,
                _ => None,
            };
            match decl_name {
                Some(decl_name) => self.module_export_name_text_unescaped(decl_name),
                None => String::new(),
            }
        };
        let module_reference = match self.data_of(node) {
            NodeData::ImportEqualsDeclaration(data) => data.module_reference,
            _ => None,
        };
        let related = self.related_info_for_node(type_only_declaration, related_message, &[&name]);
        self.error_at_with_related(module_reference.or(Some(node)), message, &[], vec![related]);
        Ok(())
    }

    /// tsc-port: resolveExportByName @6.0.3
    /// tsc-hash: 44fb57abfeda2ba65f7348458183aabe71673fd3ca7beeb3d1d71284d703d68d
    /// tsc-span: _tsc.js:48552-48569
    fn resolve_export_by_name(
        &mut self,
        module_symbol: SymbolId,
        name: &str,
        source_node: Option<NodeId>,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let export_value = self
            .binder
            .symbol(module_symbol)
            .exports
            .get(InternalSymbolName::EXPORT_EQUALS)
            .copied();
        let export_symbol = match export_value {
            Some(export_value) => {
                let ty = self.get_type_of_symbol(export_value)?;
                self.get_property_of_type_ex(
                    ty, name, /*skip_object_function_property_augment*/ true,
                )?
            }
            None => self.binder.symbol(module_symbol).exports.get(name).copied(),
        };
        let resolved = self.resolve_symbol_ex(export_symbol, dont_resolve_alias)?;
        self.mark_symbol_of_alias_declaration_if_type_only(
            source_node,
            export_symbol,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: isSyntacticDefault @6.0.3
    /// tsc-hash: 4727d1486b8e30a5c2a9fd2539e06695f1fb8af52d9d7a5576a37fc4c42cca2f
    /// tsc-span: _tsc.js:48570-48572
    fn is_syntactic_default(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        match self.kind_of(node) {
            SyntaxKind::ExportAssignment => match self.data_of(node) {
                NodeData::ExportAssignment(data) => data.is_export_equals != Some(true),
                _ => false,
            },
            SyntaxKind::ExportSpecifier | SyntaxKind::NamespaceExport => true,
            _ => {
                node_util::has_syntactic_modifier(source, node, tsrs2_types::ModifierFlags::DEFAULT)
            }
        }
    }

    /// tsc-port: canHaveSyntheticDefault @6.0.3
    /// tsc-hash: ab3950cb39060feb59cf8e5222f00e3d321ffb96e627967ec11c760cc7a06c65
    /// tsc-span: _tsc.js:48595-48651
    ///
    pub(crate) fn can_have_synthetic_default(
        &mut self,
        file_index: Option<usize>,
        module_symbol: SymbolId,
        dont_resolve_alias: bool,
        usage: Option<NodeId>,
    ) -> CheckResult2<bool> {
        if let (Some(file_index), Some(usage)) = (file_index, usage) {
            let usage_mode = self.resolution_mode_for_usage(usage);
            let target_mode = self.implied_node_format_for_file_index(file_index);
            let module_kind = self.options.emit_module_kind();
            if usage_mode == ModuleResolutionMode::EsNext
                && target_mode == ModuleResolutionMode::CommonJs
                && (100..=199).contains(&module_kind)
            {
                return Ok(true);
            }
            if usage_mode == ModuleResolutionMode::EsNext
                && target_mode == ModuleResolutionMode::EsNext
            {
                return Ok(false);
            }
        }
        if !self.options.allow_synthetic_default_imports_effective() {
            return Ok(false);
        }
        let is_declaration_file =
            file_index.map(|index| self.binder.source(index).is_declaration_file);
        if file_index.is_none() || is_declaration_file == Some(true) {
            let default_export_symbol = self.resolve_export_by_name(
                module_symbol,
                InternalSymbolName::DEFAULT,
                /*source_node*/ None,
                /*dont_resolve_alias*/ true,
            )?;
            if let Some(default_export_symbol) = default_export_symbol {
                let declarations = self
                    .binder
                    .symbol(default_export_symbol)
                    .declarations
                    .clone();
                if declarations
                    .iter()
                    .any(|&declaration| self.is_syntactic_default(declaration))
                {
                    return Ok(false);
                }
            }
            if self
                .resolve_export_by_name(
                    module_symbol,
                    &escape_leading_underscores("__esModule"),
                    /*source_node*/ None,
                    dont_resolve_alias,
                )?
                .is_some()
            {
                return Ok(false);
            }
            return Ok(true);
        }
        // TS source files (isSourceFileJS is always false: JS checking
        // remains unmodeled).
        Ok(self.has_export_assignment_symbol(module_symbol))
    }

    /// tsc-port: isOnlyImportableAsDefault @6.0.3
    /// tsc-hash: 29199e8c07f2bfda84ea1b03a7c257266beaf088be86674e43d95e927a0a5392
    /// tsc-span: _tsc.js:48577-48594
    fn is_only_importable_as_default(
        &mut self,
        usage: NodeId,
        resolved_module: Option<SymbolId>,
    ) -> CheckResult2<bool> {
        let module_kind = self.options.emit_module_kind();
        if !(100..=199).contains(&module_kind)
            || self.resolution_mode_for_usage(usage) != ModuleResolutionMode::EsNext
        {
            return Ok(false);
        }
        let resolved_module = match resolved_module {
            Some(symbol) => Some(symbol),
            None => self.resolve_external_module_name(usage, usage, true)?,
        };
        let Some(file_index) =
            resolved_module.and_then(|symbol| self.source_file_index_of_symbol(symbol))
        else {
            return Ok(false);
        };
        let file_name = &self.binder.source(file_index).file_name;
        Ok(file_name.ends_with(".json") || file_name.ends_with(".d.json.ts"))
    }

    /// tsc-port: getTargetOfImportClause @6.0.3
    /// tsc-hash: fe2fcc5056477219de5bbcfc1f88965196930b948dab81858963d126d509ff5e
    /// tsc-span: _tsc.js:48652-48657
    fn get_target_of_import_clause(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let module_specifier = self
            .parent_of(node)
            .and_then(|parent| match self.data_of(parent) {
                NodeData::ImportDeclaration(data) => data.module_specifier,
                _ => None,
            });
        let Some(module_specifier) = module_specifier else {
            return Ok(None);
        };
        let module_symbol = self.resolve_external_module_name(node, module_specifier, false)?;
        match module_symbol {
            Some(module_symbol) => {
                self.get_target_of_module_default(module_symbol, node, dont_resolve_alias)
            }
            None => Ok(None),
        }
    }

    /// tsc-port: getTargetofModuleDefault @6.0.3
    /// tsc-hash: 236973afe1c81ad598a9c40070f30503b3aaa463813b9fbae8fce69c6b4e0f4a
    /// tsc-span: _tsc.js:48658-48729
    fn get_target_of_module_default(
        &mut self,
        module_symbol: SymbolId,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let file_index = self.source_file_index_of_symbol(module_symbol);
        let specifier = self.get_module_specifier_for_import_or_export(node);
        let export_default_symbol = if self.is_shorthand_ambient_module_symbol(module_symbol) {
            Some(module_symbol)
        } else {
            if let (Some(file_index), Some(specifier)) = (file_index, specifier) {
                if (102..=199).contains(&self.options.emit_module_kind())
                    && self.resolution_mode_for_usage(specifier) == ModuleResolutionMode::CommonJs
                    && self.implied_node_format_for_file_index(file_index)
                        == ModuleResolutionMode::EsNext
                {
                    if let Some(module_exports) = self.resolve_export_by_name(
                        module_symbol,
                        "module.exports",
                        Some(node),
                        dont_resolve_alias,
                    )? {
                        if !self.options.es_module_interop_effective() {
                            let module_name = self.symbol_display_name(module_symbol);
                            self.error_at(
                                self.name_of_import_binding(node).or(Some(node)),
                                &diagnostics::Module_0_can_only_be_default_imported_using_the_1_flag,
                                &[&module_name, "esModuleInterop"],
                            );
                            return Ok(None);
                        }
                        self.mark_symbol_of_alias_declaration_if_type_only(
                            Some(node),
                            Some(module_exports),
                            /*final_target*/ None,
                            /*overwrite_empty*/ false,
                            None,
                            None,
                        )?;
                        return Ok(Some(module_exports));
                    }
                }
            }
            self.resolve_export_by_name(
                module_symbol,
                InternalSymbolName::DEFAULT,
                Some(node),
                dont_resolve_alias,
            )?
        };
        let Some(specifier) = specifier else {
            return Ok(export_default_symbol);
        };
        let has_default_only =
            self.is_only_importable_as_default(specifier, Some(module_symbol))?;
        let has_synthetic_default = self.can_have_synthetic_default(
            file_index,
            module_symbol,
            dont_resolve_alias,
            Some(specifier),
        )?;
        if export_default_symbol.is_none() && !has_synthetic_default && !has_default_only {
            if self.has_export_assignment_symbol(module_symbol)
                && !self.options.allow_synthetic_default_imports_effective()
            {
                let compiler_option_name = if self.options.emit_module_kind() >= 5 {
                    "allowSyntheticDefaultImports"
                } else {
                    "esModuleInterop"
                };
                let export_equals_symbol = self
                    .binder
                    .symbol(module_symbol)
                    .exports
                    .get(InternalSymbolName::EXPORT_EQUALS)
                    .copied();
                let export_assignment = export_equals_symbol
                    .and_then(|symbol| self.binder.symbol(symbol).value_declaration);
                let module_name = self.symbol_display_name(module_symbol);
                let error_node = self.name_of_import_binding(node).or(Some(node));
                let related = export_assignment.map(|assignment| {
                    self.related_info_for_node(
                        assignment,
                        &diagnostics::This_module_is_declared_with_export_and_can_only_be_used_with_a_default_import_when_using_the_0_flag,
                        &[compiler_option_name],
                    )
                });
                self.error_at_with_related(
                    error_node,
                    &diagnostics::Module_0_can_only_be_default_imported_using_the_1_flag,
                    &[&module_name, compiler_option_name],
                    related.into_iter().collect(),
                );
            } else if self.kind_of(node) == SyntaxKind::ImportClause {
                self.report_non_default_export(module_symbol, node)?;
            } else {
                let name = match self.data_of(node) {
                    NodeData::ImportSpecifier(data) => data.property_name.or(data.name),
                    NodeData::ExportSpecifier(data) => data.property_name.or(data.name),
                    _ => None,
                }
                .unwrap_or(node);
                self.error_no_module_member_symbol(module_symbol, module_symbol, node, name)?;
            }
        } else if has_synthetic_default || has_default_only {
            let resolved = match self
                .resolve_external_module_symbol(Some(module_symbol), dont_resolve_alias)?
            {
                Some(resolved) => Some(resolved),
                None => self.resolve_symbol_ex(Some(module_symbol), dont_resolve_alias)?,
            };
            self.mark_symbol_of_alias_declaration_if_type_only(
                Some(node),
                Some(module_symbol),
                resolved,
                /*overwrite_empty*/ false,
                None,
                None,
            )?;
            return Ok(resolved);
        }
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            export_default_symbol,
            /*final_target*/ None,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(export_default_symbol)
    }

    /// tsc-port: getModuleSpecifierForImportOrExport @6.0.3
    /// tsc-hash: 2eeb2351bb1c87bd21f103f5a8d83c8d349e8643388e49c4f409090be6656e76
    /// tsc-span: _tsc.js:48730-48745
    fn get_module_specifier_for_import_or_export(&self, node: NodeId) -> Option<NodeId> {
        match self.kind_of(node) {
            SyntaxKind::ImportClause => {
                let parent = self.parent_of(node)?;
                match self.data_of(parent) {
                    NodeData::ImportDeclaration(data) => data.module_specifier,
                    _ => None,
                }
            }
            SyntaxKind::ImportEqualsDeclaration => match self.data_of(node) {
                NodeData::ImportEqualsDeclaration(data) => {
                    let reference = data.module_reference?;
                    match self.data_of(reference) {
                        NodeData::ExternalModuleReference(data) => data.expression,
                        _ => None,
                    }
                }
                _ => None,
            },
            SyntaxKind::NamespaceImport => {
                let clause = self.parent_of(node)?;
                let declaration = self.parent_of(clause)?;
                match self.data_of(declaration) {
                    NodeData::ImportDeclaration(data) => data.module_specifier,
                    _ => None,
                }
            }
            SyntaxKind::ImportSpecifier => {
                let named = self.parent_of(node)?;
                let clause = self.parent_of(named)?;
                let declaration = self.parent_of(clause)?;
                match self.data_of(declaration) {
                    NodeData::ImportDeclaration(data) => data.module_specifier,
                    _ => None,
                }
            }
            SyntaxKind::ExportSpecifier => {
                let named = self.parent_of(node)?;
                let declaration = self.parent_of(named)?;
                match self.data_of(declaration) {
                    NodeData::ExportDeclaration(data) => data.module_specifier,
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// tsc-port: reportNonDefaultExport @6.0.3
    /// tsc-hash: 1e78e6ce1d5ae1a3596cd870ce3c64c261d2f702978fe07fbc301726fe85268f
    /// tsc-span: _tsc.js:48746-48770
    fn report_non_default_export(
        &mut self,
        module_symbol: SymbolId,
        node: NodeId,
    ) -> CheckResult2<()> {
        let name = match self.data_of(node) {
            NodeData::ImportClause(data) => data.name,
            _ => None,
        };
        let local_symbol = self.binder.node_symbol(node);
        let module_name = self.symbol_display_name(module_symbol);
        let exports_has_local = local_symbol.is_some_and(|local| {
            let local_name = self.binder.symbol(local).escaped_name.clone();
            self.binder
                .symbol(module_symbol)
                .exports
                .contains_key(&local_name)
        });
        if exports_has_local {
            let local_name = local_symbol
                .map(|local| self.symbol_display_name(local))
                .unwrap_or_default();
            self.error_at(
                name.or(Some(node)),
                &diagnostics::Module_0_has_no_default_export_Did_you_mean_to_use_import_1_from_0_instead,
                &[&module_name, &local_name],
            );
            return Ok(());
        }
        let export_star = self
            .binder
            .symbol(module_symbol)
            .exports
            .get(InternalSymbolName::EXPORT_STAR)
            .copied();
        let mut related = Vec::new();
        if let Some(export_star) = export_star {
            let declarations = self.binder.symbol(export_star).declarations.clone();
            for declaration in declarations {
                let specifier = match self.data_of(declaration) {
                    NodeData::ExportDeclaration(data) => data.module_specifier,
                    _ => None,
                };
                let Some(specifier) = specifier else {
                    continue;
                };
                let resolved = self.resolve_external_module_name(declaration, specifier, false)?;
                if let Some(resolved) = resolved {
                    if self
                        .binder
                        .symbol(resolved)
                        .exports
                        .contains_key(InternalSymbolName::DEFAULT)
                    {
                        related.push(self.related_info_for_node(
                            declaration,
                            &diagnostics::export_does_not_re_export_a_default,
                            &[],
                        ));
                        break;
                    }
                }
            }
        }
        self.error_at_with_related(
            name.or(Some(node)),
            &diagnostics::Module_0_has_no_default_export,
            &[&module_name],
            related,
        );
        Ok(())
    }

    /// tsc-port: getTargetOfNamespaceImport @6.0.3
    /// tsc-hash: f36cfa1a95a81d070ccc9ccdb48f44d67974e2ebdb9a8e78e389a7232bdd78ef
    /// tsc-span: _tsc.js:48771-48789
    fn get_target_of_namespace_import(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let Some(module_specifier) = self.get_module_specifier_for_import_or_export(node) else {
            return Ok(None);
        };
        let immediate = self.resolve_external_module_name(node, module_specifier, false)?;
        let resolved = self.resolve_es_module_symbol(
            immediate,
            module_specifier,
            dont_resolve_alias,
            /*suppress_interop_error*/ false,
        )?;
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            immediate,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: getTargetOfNamespaceExport @6.0.3
    /// tsc-hash: 616c397fc557af9c11586f04ed99fce4be8a328c8b1754c768b2e842dc8cb6fa
    /// tsc-span: _tsc.js:48790-48808
    fn get_target_of_namespace_export(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let module_specifier = self
            .parent_of(node)
            .and_then(|parent| match self.data_of(parent) {
                NodeData::ExportDeclaration(data) => data.module_specifier,
                _ => None,
            });
        let Some(module_specifier) = module_specifier else {
            self.mark_symbol_of_alias_declaration_if_type_only(
                Some(node),
                None,
                None,
                /*overwrite_empty*/ false,
                None,
                None,
            )?;
            return Ok(None);
        };
        let immediate = self.resolve_external_module_name(node, module_specifier, false)?;
        let resolved = self.resolve_es_module_symbol(
            immediate,
            module_specifier,
            dont_resolve_alias,
            /*suppress_interop_error*/ false,
        )?;
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            immediate,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: combineValueAndTypeSymbols @6.0.3
    /// tsc-hash: 3219c9f04214a9d0a4d88bf094ada69308a9effe349efa4bada0c118bf10ed22
    /// tsc-span: _tsc.js:48809-48824
    fn combine_value_and_type_symbols(
        &mut self,
        value_symbol: SymbolId,
        type_symbol: SymbolId,
    ) -> SymbolId {
        if value_symbol == self.unknown_symbol && type_symbol == self.unknown_symbol {
            return self.unknown_symbol;
        }
        let value = self.binder.symbol(value_symbol);
        if value
            .flags
            .intersects(SymbolFlags::TYPE | SymbolFlags::NAMESPACE)
        {
            return value_symbol;
        }
        let flags = value.flags | self.binder.symbol(type_symbol).flags;
        let escaped_name = value.escaped_name.clone();
        let mut declarations = value.declarations.clone();
        for declaration in self.binder.symbol(type_symbol).declarations.clone() {
            if !declarations.contains(&declaration) {
                declarations.push(declaration);
            }
        }
        let parent = value.parent.or(self.binder.symbol(type_symbol).parent);
        let value_declaration = value.value_declaration;
        let members = self.binder.symbol(type_symbol).members.clone();
        let exports = self.binder.symbol(value_symbol).exports.clone();
        let result = self.binder.create_symbol(flags, escaped_name);
        let symbol = self.binder.symbol_mut(result);
        symbol.declarations = declarations;
        symbol.parent = parent;
        symbol.value_declaration = value_declaration;
        symbol.members = members;
        symbol.exports = exports;
        result
    }

    /// tsc-port: getExportOfModule @6.0.3
    /// tsc-hash: 950617f5bdc9aeda1fea2768cf70eb55ea0d2209306b7c3b9d510fd63f7cc57f
    /// tsc-span: _tsc.js:48825-48842
    fn get_export_of_module(
        &mut self,
        symbol: SymbolId,
        name_text: &str,
        specifier: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::MODULE)
        {
            return Ok(None);
        }
        let exports = self.get_exports_of_symbol(symbol)?;
        let export_symbol = exports.get(name_text).copied();
        let resolved = self.resolve_symbol_ex(export_symbol, dont_resolve_alias)?;
        let export_star_declaration = self
            .links
            .symbol(symbol)
            .type_only_export_star_map
            .as_ref()
            .and_then(|map| map.get(name_text))
            .copied();
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(specifier),
            export_symbol,
            resolved,
            /*overwrite_empty*/ false,
            export_star_declaration,
            Some(name_text),
        )?;
        Ok(resolved)
    }

    /// tsc-port: getPropertyOfVariable @6.0.3
    /// tsc-hash: fdd893d0bd7bcf0aa247b2765c029c1d4698c4d9438f49f0f07ce31ba360d386
    /// tsc-span: _tsc.js:48843-48850
    fn get_property_of_variable(
        &mut self,
        symbol: SymbolId,
        name: &str,
    ) -> CheckResult2<Option<SymbolId>> {
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::VARIABLE)
        {
            return Ok(None);
        }
        let Some(value_declaration) = self.binder.symbol(symbol).value_declaration else {
            return Ok(None);
        };
        let type_annotation = match self.data_of(value_declaration) {
            NodeData::VariableDeclaration(data) => data.r#type,
            _ => None,
        };
        let Some(type_annotation) = type_annotation else {
            return Ok(None);
        };
        let ty = self.get_type_from_type_node(type_annotation)?;
        let property = self.get_property_of_type_full(ty, name)?;
        self.resolve_symbol_ex(property, false)
    }

    /// tsc-port: getExternalModuleMember @6.0.3
    /// tsc-hash: 339bc0c293e029fb5e37d6cd2ef3973ac0de57ccea32b553357cf31853178e69
    /// tsc-span: _tsc.js:48851-48901
    ///
    /// The require-argument arm is JS-only.
    fn get_external_module_member(
        &mut self,
        node: NodeId,
        specifier: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let module_specifier = match self.data_of(node) {
            NodeData::ImportDeclaration(data) => data.module_specifier,
            NodeData::ExportDeclaration(data) => data.module_specifier,
            _ => None,
        };
        let Some(module_specifier) = module_specifier else {
            return Ok(None);
        };
        let module_symbol = self.resolve_external_module_name(node, module_specifier, false)?;
        let name = match self.data_of(specifier) {
            NodeData::ImportSpecifier(data) => data.property_name.or(data.name),
            NodeData::ExportSpecifier(data) => data.property_name.or(data.name),
            _ => None,
        };
        let Some(name) = name else {
            return Ok(None);
        };
        if !matches!(
            self.kind_of(name),
            SyntaxKind::Identifier | SyntaxKind::StringLiteral
        ) {
            return Ok(None);
        }
        let name_text = self.module_export_name_text_escaped(name);
        let suppress_interop_error = name_text == InternalSymbolName::DEFAULT
            && self.options.allow_synthetic_default_imports_effective();
        let target_symbol = self.resolve_es_module_symbol(
            module_symbol,
            module_specifier,
            /*dont_resolve_alias*/ false,
            suppress_interop_error,
        )?;
        let Some(target_symbol) = target_symbol else {
            return Ok(None);
        };
        if name_text.is_empty() && self.kind_of(name) != SyntaxKind::StringLiteral {
            return Ok(None);
        }
        let module_symbol = module_symbol.expect("targetSymbol implies moduleSymbol");
        if self.is_shorthand_ambient_module_symbol(module_symbol) {
            return Ok(Some(module_symbol));
        }
        let mut symbol_from_variable = if self
            .binder
            .symbol(module_symbol)
            .exports
            .contains_key(InternalSymbolName::EXPORT_EQUALS)
        {
            let ty = self.get_type_of_symbol(target_symbol)?;
            self.get_property_of_type_ex(
                ty, &name_text, /*skip_object_function_property_augment*/ true,
            )?
        } else {
            self.get_property_of_variable(target_symbol, &name_text)?
        };
        symbol_from_variable = self.resolve_symbol_ex(symbol_from_variable, dont_resolve_alias)?;
        let mut symbol_from_module =
            self.get_export_of_module(target_symbol, &name_text, specifier, dont_resolve_alias)?;
        if symbol_from_module.is_none() && name_text == InternalSymbolName::DEFAULT {
            let file_index = self.source_file_index_of_symbol(module_symbol);
            if self.is_only_importable_as_default(module_specifier, Some(module_symbol))?
                || self.can_have_synthetic_default(
                    file_index,
                    module_symbol,
                    dont_resolve_alias,
                    Some(module_specifier),
                )?
            {
                symbol_from_module = match self
                    .resolve_external_module_symbol(Some(module_symbol), dont_resolve_alias)?
                {
                    Some(resolved) => Some(resolved),
                    None => self.resolve_symbol_ex(Some(module_symbol), dont_resolve_alias)?,
                };
            }
        }
        let symbol = match (symbol_from_module, symbol_from_variable) {
            (Some(from_module), Some(from_variable)) if from_module != from_variable => {
                Some(self.combine_value_and_type_symbols(from_variable, from_module))
            }
            (from_module, from_variable) => from_module.or(from_variable),
        };
        if self.is_only_importable_as_default(module_specifier, Some(module_symbol))?
            && name_text != InternalSymbolName::DEFAULT
        {
            let module_kind = match self.options.emit_module_kind() {
                100 => "Node16",
                101 => "Node18",
                102 => "Node20",
                199 => "NodeNext",
                _ => "NodeNext",
            };
            self.error_at(
                Some(name),
                &diagnostics::Named_imports_from_a_JSON_file_into_an_ECMAScript_module_are_not_allowed_when_module_is_set_to_0,
                &[module_kind],
            );
        } else if symbol.is_none() {
            self.error_no_module_member_symbol(module_symbol, target_symbol, node, name)?;
        }
        Ok(symbol)
    }

    /// tsc-port: errorNoModuleMemberSymbol @6.0.3
    /// tsc-hash: 3e55da20fe5b9c8b41d4fe359abeb4b4361f8ba2d20a5d28ed7445f3b6347d58
    /// tsc-span: _tsc.js:48902-48925
    fn error_no_module_member_symbol(
        &mut self,
        module_symbol: SymbolId,
        target_symbol: SymbolId,
        _node: NodeId,
        name: NodeId,
    ) -> CheckResult2<()> {
        let module_name = self.fully_qualified_name(module_symbol);
        let declaration_name = self.module_export_name_text_unescaped(name);
        let suggestion = if self.kind_of(name) == SyntaxKind::Identifier {
            self.get_suggested_symbol_for_nonexistent_module(name, target_symbol)?
        } else {
            None
        };
        if let Some(suggestion) = suggestion {
            let suggestion_name = self.symbol_display_name(suggestion);
            let related = self
                .binder
                .symbol(suggestion)
                .value_declaration
                .map(|declaration| {
                    self.related_info_for_node(
                        declaration,
                        &diagnostics::_0_is_declared_here,
                        &[&suggestion_name],
                    )
                });
            self.error_at_with_related(
                Some(name),
                &diagnostics::_0_has_no_exported_member_named_1_Did_you_mean_2,
                &[&module_name, &declaration_name, &suggestion_name],
                related.into_iter().collect(),
            );
            return Ok(());
        }
        if self
            .binder
            .symbol(module_symbol)
            .exports
            .contains_key(InternalSymbolName::DEFAULT)
        {
            self.error_at(
                Some(name),
                &diagnostics::Module_0_has_no_exported_member_1_Did_you_mean_to_use_import_1_from_0_instead,
                &[&module_name, &declaration_name],
            );
        } else {
            self.report_non_exported_member(name, &declaration_name, module_symbol, &module_name)?;
        }
        Ok(())
    }

    /// tsc-port: reportNonExportedMember @6.0.3
    /// tsc-hash: 59111fe9aaa904e06888c3c292c7b372d6398d0072ebd3d17c58778855786161
    /// tsc-span: _tsc.js:48926-48944
    fn report_non_exported_member(
        &mut self,
        name: NodeId,
        declaration_name: &str,
        module_symbol: SymbolId,
        module_name: &str,
    ) -> CheckResult2<()> {
        let name_text = self.module_export_name_text_escaped(name);
        let local_symbol = self
            .binder
            .symbol(module_symbol)
            .value_declaration
            .and_then(|declaration| self.binder.locals_of(declaration))
            .and_then(|locals| locals.get(&name_text))
            .copied();
        let Some(local_symbol) = local_symbol else {
            self.error_at(
                Some(name),
                &diagnostics::Module_0_has_no_exported_member_1,
                &[module_name, declaration_name],
            );
            return Ok(());
        };
        let exported_equals_symbol = self
            .binder
            .symbol(module_symbol)
            .exports
            .get(InternalSymbolName::EXPORT_EQUALS)
            .copied();
        if let Some(exported_equals_symbol) = exported_equals_symbol {
            if self
                .get_symbol_if_same_reference(exported_equals_symbol, local_symbol)?
                .is_some()
            {
                self.report_invalid_import_equals_export_member(
                    name,
                    declaration_name,
                    module_name,
                )?;
            } else {
                self.error_at(
                    Some(name),
                    &diagnostics::Module_0_has_no_exported_member_1,
                    &[module_name, declaration_name],
                );
            }
            return Ok(());
        }
        let exports: Vec<SymbolId> = self
            .binder
            .symbol(module_symbol)
            .exports
            .values()
            .copied()
            .collect();
        let mut exported_symbol = None;
        for export in exports {
            if self
                .get_symbol_if_same_reference(export, local_symbol)?
                .is_some()
            {
                exported_symbol = Some(export);
                break;
            }
        }
        let mut related = Vec::new();
        for (index, declaration) in self
            .binder
            .symbol(local_symbol)
            .declarations
            .clone()
            .into_iter()
            .enumerate()
        {
            let message = if index == 0 {
                &diagnostics::_0_is_declared_here
            } else {
                &diagnostics::and_here
            };
            related.push(self.related_info_for_node(declaration, message, &[declaration_name]));
        }
        match exported_symbol {
            Some(exported_symbol) => {
                let exported_name = self.symbol_display_name(exported_symbol);
                self.error_at_with_related(
                    Some(name),
                    &diagnostics::Module_0_declares_1_locally_but_it_is_exported_as_2,
                    &[module_name, declaration_name, &exported_name],
                    related,
                );
            }
            None => {
                self.error_at_with_related(
                    Some(name),
                    &diagnostics::Module_0_declares_1_locally_but_it_is_not_exported,
                    &[module_name, declaration_name],
                    related,
                );
            }
        }
        Ok(())
    }

    /// tsc-port: reportInvalidImportEqualsExportMember @6.0.3
    /// tsc-hash: 8c4469049e4a46f4f4745eede49d15e95046313726a19d94760a76b79f324fb3
    /// tsc-span: _tsc.js:48945-48958
    ///
    /// The isInJSFile middle arm is JS-only (constant-dead).
    fn report_invalid_import_equals_export_member(
        &mut self,
        name: NodeId,
        declaration_name: &str,
        module_name: &str,
    ) -> CheckResult2<()> {
        let es_module_interop = self.options.es_module_interop_effective();
        if self.options.emit_module_kind() >= 5 {
            let message = if es_module_interop {
                &diagnostics::_0_can_only_be_imported_by_using_a_default_import
            } else {
                &diagnostics::_0_can_only_be_imported_by_turning_on_the_esModuleInterop_flag_and_using_a_default_import
            };
            self.error_at(Some(name), message, &[declaration_name]);
        } else if es_module_interop {
            self.error_at(
                Some(name),
                &diagnostics::_0_can_only_be_imported_by_using_import_1_require_2_or_a_default_import,
                &[declaration_name, declaration_name, module_name],
            );
        } else {
            self.error_at(
                Some(name),
                &diagnostics::_0_can_only_be_imported_by_using_import_1_require_2_or_by_turning_on_the_esModuleInterop_flag_and_using_a_default_import,
                &[declaration_name, declaration_name, module_name],
            );
        }
        Ok(())
    }

    /// tsc-port: getTargetOfImportSpecifier @6.0.3
    /// tsc-hash: b70d0ba3fc6fdabec84fe6e33db05f4876baf99eea95b1091ec57298e4508601
    /// tsc-span: _tsc.js:48959-48983
    ///
    /// The BindingElement/commonJSPropertyAccess arms are JS-only.
    fn get_target_of_import_specifier(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let (property_name, name) = match self.data_of(node) {
            NodeData::ImportSpecifier(data) => (data.property_name, data.name),
            _ => return Ok(None),
        };
        let Some(effective) = property_name.or(name) else {
            return Ok(None);
        };
        if self.module_export_name_is_default(effective) {
            let specifier = self.get_module_specifier_for_import_or_export(node);
            if let Some(specifier) = specifier {
                let module_symbol = self.resolve_external_module_name(node, specifier, false)?;
                if let Some(module_symbol) = module_symbol {
                    return self.get_target_of_module_default(
                        module_symbol,
                        node,
                        dont_resolve_alias,
                    );
                }
            }
        }
        let named = self.parent_of(node);
        let clause = named.and_then(|named| self.parent_of(named));
        let root = clause.and_then(|clause| self.parent_of(clause));
        let Some(root) = root else {
            return Ok(None);
        };
        let resolved = self.get_external_module_member(root, node, dont_resolve_alias)?;
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            /*immediate_target*/ None,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: getTargetOfNamespaceExportDeclaration @6.0.3
    /// tsc-hash: 33a498c52c8069fadc5d21828e57801eb7a2adfa655e3aebb5460b95f74c0718
    /// tsc-span: _tsc.js:48989-49002
    fn get_target_of_namespace_export_declaration(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(None);
        };
        let Some(parent_symbol) = self.binder.node_symbol(parent) else {
            return Ok(None);
        };
        let resolved =
            self.resolve_external_module_symbol(Some(parent_symbol), dont_resolve_alias)?;
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            /*immediate_target*/ None,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: getTargetOfExportSpecifier @6.0.3
    /// tsc-hash: 7d710915d1c15e2eff24a3f23fffdcac5bd92f5b92d1ce16c411a297346265d6
    /// tsc-span: _tsc.js:49003-49031
    fn get_target_of_export_specifier(
        &mut self,
        node: NodeId,
        meaning: SymbolFlags,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let (property_name, name) = match self.data_of(node) {
            NodeData::ExportSpecifier(data) => (data.property_name, data.name),
            _ => return Ok(None),
        };
        let Some(effective) = property_name.or(name) else {
            return Ok(None);
        };
        if self.module_export_name_is_default(effective) {
            let specifier = self.get_module_specifier_for_import_or_export(node);
            if let Some(specifier) = specifier {
                let module_symbol = self.resolve_external_module_name(node, specifier, false)?;
                if let Some(module_symbol) = module_symbol {
                    return self.get_target_of_module_default(
                        module_symbol,
                        node,
                        dont_resolve_alias,
                    );
                }
            }
        }
        let named = self.parent_of(node);
        let declaration = named.and_then(|named| self.parent_of(named));
        let module_specifier =
            declaration.and_then(|declaration| match self.data_of(declaration) {
                NodeData::ExportDeclaration(data) => data.module_specifier,
                _ => None,
            });
        let resolved = if module_specifier.is_some() {
            self.get_external_module_member(
                declaration.expect("module specifier implies declaration"),
                node,
                dont_resolve_alias,
            )?
        } else if self.kind_of(effective) == SyntaxKind::StringLiteral {
            // Invalid syntax like `export { "x" }` — skip.
            None
        } else {
            self.resolve_entity_name_ex(
                effective,
                meaning,
                /*ignore_errors*/ false,
                None,
                dont_resolve_alias,
            )?
        };
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            /*immediate_target*/ None,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: getTargetOfExportAssignment @6.0.3
    /// tsc-hash: 574af7e108e18500ba0aff0d7474a3b5568683bf9767ed66bc17355924140e5f
    /// tsc-span: _tsc.js:49032-49044
    fn get_target_of_export_assignment(
        &mut self,
        node: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let expression = match self.data_of(node) {
            NodeData::ExportAssignment(data) => data.expression,
            _ => None,
        };
        let Some(expression) = expression else {
            return Ok(None);
        };
        let resolved = self.get_target_of_alias_like_expression(expression, dont_resolve_alias)?;
        self.mark_symbol_of_alias_declaration_if_type_only(
            Some(node),
            /*immediate_target*/ None,
            resolved,
            /*overwrite_empty*/ false,
            None,
            None,
        )?;
        Ok(resolved)
    }

    /// tsc-port: getTargetOfAliasLikeExpression @6.0.3
    /// tsc-hash: 6935da2d1759d7c4ae48909f28a7de4a2985b58a1a5dd9ba53fbdf98c9b57ebc
    /// tsc-span: _tsc.js:49045-49064
    fn get_target_of_alias_like_expression(
        &mut self,
        expression: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        if self.kind_of(expression) == SyntaxKind::ClassExpression {
            let ty = self.check_expression_cached(expression, CheckMode::NORMAL)?;
            return Ok(self.tables.type_of(ty).symbol);
        }
        if !self.is_entity_name_kind(expression) && !self.is_entity_name_expression(expression) {
            return Ok(None);
        }
        let alias_like = self.resolve_entity_name_ex(
            expression,
            SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE,
            /*ignore_errors*/ true,
            None,
            dont_resolve_alias,
        )?;
        if alias_like.is_some() {
            return Ok(alias_like);
        }
        self.check_expression_cached(expression, CheckMode::NORMAL)?;
        Ok(self.links.node(expression).resolved_symbol.resolved())
    }

    /// tsc-port: getSymbolOfPartOfRightHandSideOfImportEquals @6.0.3
    /// tsc-hash: a112e7860808cb430fdf9a64fe7c8ad5cd3e3d7b19bc5e819d2d8670e2aeff35
    /// tsc-span: _tsc.js:49230-49252
    pub(crate) fn get_symbol_of_part_of_right_hand_side_of_import_equals(
        &mut self,
        entity_name: NodeId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let mut entity_name = entity_name;
        if self.kind_of(entity_name) == SyntaxKind::Identifier
            && self.is_right_side_of_qualified_name_or_property_access(entity_name)
        {
            entity_name = self.parent_of(entity_name).unwrap_or(entity_name);
        }
        let parent_kind = self
            .parent_of(entity_name)
            .map(|parent| self.kind_of(parent));
        if self.kind_of(entity_name) == SyntaxKind::Identifier
            || parent_kind == Some(SyntaxKind::QualifiedName)
        {
            self.resolve_entity_name_ex(
                entity_name,
                SymbolFlags::NAMESPACE,
                /*ignore_errors*/ false,
                None,
                dont_resolve_alias,
            )
        } else {
            debug_assert_eq!(parent_kind, Some(SyntaxKind::ImportEqualsDeclaration));
            self.resolve_entity_name_ex(
                entity_name,
                SymbolFlags::VALUE | SymbolFlags::TYPE | SymbolFlags::NAMESPACE,
                /*ignore_errors*/ false,
                None,
                dont_resolve_alias,
            )
        }
    }

    /// tsc-port: getSymbolFlags @6.0.3
    /// tsc-hash: e3b7a170601483e5984d25a007dca056d30c0eac774e5ff10117b01088ae6eb3
    /// tsc-span: _tsc.js:49141-49175
    pub(crate) fn get_symbol_flags_full(
        &mut self,
        symbol: SymbolId,
        exclude_type_only_meanings: bool,
        exclude_local_meanings: bool,
    ) -> CheckResult2<SymbolFlags> {
        let type_only_declaration = if exclude_type_only_meanings {
            self.get_type_only_alias_declaration(symbol)?
        } else {
            None
        };
        let type_only_declaration_is_export_star = type_only_declaration
            .is_some_and(|declaration| self.kind_of(declaration) == SyntaxKind::ExportDeclaration);
        let type_only_resolution = match type_only_declaration {
            Some(declaration) if type_only_declaration_is_export_star => {
                let specifier = match self.data_of(declaration) {
                    NodeData::ExportDeclaration(data) => data.module_specifier,
                    _ => None,
                };
                match specifier {
                    Some(specifier) => {
                        self.resolve_external_module_name(specifier, specifier, true)?
                    }
                    None => None,
                }
            }
            Some(declaration) => match self.binder.node_symbol(declaration) {
                Some(declaration_symbol) => Some(self.resolve_alias(declaration_symbol)?),
                None => None,
            },
            None => None,
        };
        let type_only_export_star_targets = match type_only_resolution {
            Some(resolution) if type_only_declaration_is_export_star => {
                Some(self.get_exports_of_module(resolution)?)
            }
            _ => None,
        };
        let mut flags = if exclude_local_meanings {
            SymbolFlags::NONE
        } else {
            self.binder.symbol(symbol).flags
        };
        let mut symbol = symbol;
        let mut seen_symbols: Option<std::collections::HashSet<SymbolId>> = None;
        while self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            let resolved = self.resolve_alias(symbol)?;
            let target = self.get_export_symbol_of_value_symbol_if_exported(resolved);
            if (!type_only_declaration_is_export_star && Some(target) == type_only_resolution)
                || type_only_export_star_targets
                    .as_ref()
                    .is_some_and(|targets| {
                        targets
                            .get(&self.binder.symbol(target).escaped_name)
                            .copied()
                            == Some(target)
                    })
            {
                break;
            }
            if target == self.unknown_symbol {
                return Ok(SymbolFlags::ALL);
            }
            if target == symbol
                || seen_symbols
                    .as_ref()
                    .is_some_and(|seen| seen.contains(&target))
            {
                break;
            }
            if self
                .binder
                .symbol(target)
                .flags
                .intersects(SymbolFlags::ALIAS)
            {
                match &mut seen_symbols {
                    Some(seen) => {
                        seen.insert(target);
                    }
                    None => {
                        seen_symbols = Some(std::collections::HashSet::from_iter([symbol, target]));
                    }
                }
            }
            flags |= self.binder.symbol(target).flags;
            symbol = target;
        }
        Ok(flags)
    }

    /// tsrs-native: the no-excludes flavor of get_symbol_flags_full
    /// (tsc's optional-parameter defaults).
    pub(crate) fn get_symbol_flags_of(&mut self, symbol: SymbolId) -> CheckResult2<SymbolFlags> {
        self.get_symbol_flags_full(symbol, false, false)
    }

    /// tsc-port: markSymbolOfAliasDeclarationIfTypeOnly @6.0.3
    /// tsc-hash: 740c0d7d48311aab54d9da24bb2a8378641670123b8d94249e5087a736a590b5
    /// tsc-span: _tsc.js:49176-49194
    pub(crate) fn mark_symbol_of_alias_declaration_if_type_only(
        &mut self,
        alias_declaration: Option<NodeId>,
        immediate_target: Option<SymbolId>,
        final_target: Option<SymbolId>,
        overwrite_empty: bool,
        export_star_declaration: Option<NodeId>,
        export_star_name: Option<&str>,
    ) -> CheckResult2<bool> {
        let Some(alias_declaration) = alias_declaration else {
            return Ok(false);
        };
        if self.kind_of(alias_declaration) == SyntaxKind::PropertyAccessExpression {
            return Ok(false);
        }
        let source_symbol = self.get_symbol_of_declaration(alias_declaration)?;
        if self.is_type_only_import_or_export_declaration(alias_declaration) {
            self.links.set_symbol_type_only_declaration(
                self.speculation_depth,
                source_symbol,
                Some(alias_declaration),
            );
            return Ok(true);
        }
        if let Some(export_star_declaration) = export_star_declaration {
            self.links.set_symbol_type_only_declaration(
                self.speculation_depth,
                source_symbol,
                Some(export_star_declaration),
            );
            if let Some(export_star_name) = export_star_name {
                if self.binder.symbol(source_symbol).escaped_name != export_star_name {
                    self.links.set_symbol_type_only_export_star_name(
                        self.speculation_depth,
                        source_symbol,
                        export_star_name.to_owned(),
                    );
                }
            }
            return Ok(true);
        }
        let marked =
            self.mark_type_only_worker(source_symbol, immediate_target, overwrite_empty)?;
        if marked {
            return Ok(true);
        }
        self.mark_type_only_worker(source_symbol, final_target, overwrite_empty)
    }

    /// tsc-port: markSymbolOfAliasDeclarationIfTypeOnlyWorker @6.0.3
    /// tsc-hash: 7a4e15ebd6ffa75f0bf3f56a5667824f9a3438b62762d0a6f1d7582ef13a40ea
    /// tsc-span: _tsc.js:49195-49203
    fn mark_type_only_worker(
        &mut self,
        source_symbol: SymbolId,
        target: Option<SymbolId>,
        overwrite_empty: bool,
    ) -> CheckResult2<bool> {
        let existing = self.links.symbol(source_symbol).type_only_declaration;
        if let Some(target) = target {
            if existing.is_none() || (overwrite_empty && existing == Some(None)) {
                let export_symbol = self
                    .binder
                    .symbol(target)
                    .exports
                    .get(InternalSymbolName::EXPORT_EQUALS)
                    .copied()
                    .unwrap_or(target);
                let type_only = self
                    .binder
                    .symbol(export_symbol)
                    .declarations
                    .clone()
                    .into_iter()
                    .find(|&declaration| {
                        self.is_type_only_import_or_export_declaration(declaration)
                    });
                let value = match type_only {
                    Some(declaration) => Some(declaration),
                    None => self
                        .links
                        .symbol(export_symbol)
                        .type_only_declaration
                        .flatten(),
                };
                self.links.set_symbol_type_only_declaration(
                    self.speculation_depth,
                    source_symbol,
                    value,
                );
            }
        }
        Ok(self
            .links
            .symbol(source_symbol)
            .type_only_declaration
            .flatten()
            .is_some())
    }

    /// tsc-port: getTypeOnlyAliasDeclaration @6.0.3
    /// tsc-hash: 7362b6f3df10e09dfd3429ebbda90bc5bcaaf37024b029e74c85bbc1f0c64de6
    /// tsc-span: _tsc.js:49204-49229
    ///
    /// The include-filtered flavor (export-star resolution through
    /// getExportsOfModule) rides the same body; pass None for tsc's
    /// undefined `include`.
    pub(crate) fn get_type_only_alias_declaration(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<Option<NodeId>> {
        self.get_type_only_alias_declaration_ex(symbol, None)
    }

    /// tsrs-native: the include-carrying body behind
    /// get_type_only_alias_declaration (tsc's optional `include`
    /// parameter, 49204-49229 — hash/span live on the wrapper).
    pub(crate) fn get_type_only_alias_declaration_ex(
        &mut self,
        symbol: SymbolId,
        include: Option<SymbolFlags>,
    ) -> CheckResult2<Option<NodeId>> {
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            return Ok(None);
        }
        if self.links.symbol(symbol).type_only_declaration.is_none() {
            self.links
                .set_symbol_type_only_declaration(self.speculation_depth, symbol, None);
            let resolved = self.resolve_symbol_ex(Some(symbol), false)?;
            let immediate = match self.get_declaration_of_alias_symbol(symbol) {
                Some(_) => self.get_immediate_aliased_symbol(symbol)?,
                None => None,
            };
            let first_declaration = self.binder.symbol(symbol).declarations.first().copied();
            self.mark_symbol_of_alias_declaration_if_type_only(
                first_declaration,
                immediate,
                resolved,
                /*overwrite_empty*/ true,
                None,
                None,
            )?;
        }
        let type_only_declaration = self.links.symbol(symbol).type_only_declaration.flatten();
        let Some(include) = include else {
            return Ok(type_only_declaration);
        };
        let Some(declaration) = type_only_declaration else {
            return Ok(None);
        };
        let resolved = if self.kind_of(declaration) == SyntaxKind::ExportDeclaration {
            let declaration_symbol = self.binder.node_symbol(declaration);
            let parent = declaration_symbol
                .and_then(|declaration_symbol| self.binder.symbol(declaration_symbol).parent);
            let Some(parent) = parent else {
                return Ok(None);
            };
            let exports = self.get_exports_of_module(parent)?;
            let lookup_name = self
                .links
                .symbol(symbol)
                .type_only_export_star_name
                .unwrap_or_else(|| self.binder.symbol(symbol).escaped_name.clone());
            let export_symbol = exports.get(&lookup_name).copied();
            self.resolve_symbol_ex(export_symbol, false)?
        } else {
            let declaration_symbol = self.binder.node_symbol(declaration);
            match declaration_symbol {
                Some(declaration_symbol) => Some(self.resolve_alias(declaration_symbol)?),
                None => None,
            }
        };
        let Some(resolved) = resolved else {
            return Ok(None);
        };
        if self.get_symbol_flags_of(resolved)?.intersects(include) {
            Ok(Some(declaration))
        } else {
            Ok(None)
        }
    }

    /// tsc-port: getImmediateAliasedSymbol @6.0.3
    /// tsc-hash: 3535835c9331851f2d0022c35d6fdec94e0d67348c32546e188d2e11d8445757
    /// tsc-span: _tsc.js:50092-50101
    pub(crate) fn get_immediate_aliased_symbol(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<Option<SymbolId>> {
        debug_assert!(
            self.binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::ALIAS),
            "Should only get Alias here."
        );
        let links_immediate = self.links.symbol(symbol).immediate_target;
        if let Some(immediate) = links_immediate {
            return Ok(immediate);
        }
        let Some(node) = self.get_declaration_of_alias_symbol(symbol) else {
            return Err(Unsupported::new(
                "getImmediateAliasedSymbol without alias declaration (parse-recovery shape, tsc Debug.fail)",
            ));
        };
        let target =
            self.get_target_of_alias_declaration(node, /*dont_recursively_resolve*/ true)?;
        self.links
            .set_symbol_immediate_target(self.speculation_depth, symbol, target);
        Ok(target)
    }

    /// tsc-port: getTypeOfAlias @6.0.3
    /// tsc-hash: 5c80fbb8a3f77b883164000d53b9c225931e458723582d7eeb69e47b95ca997d
    /// tsc-span: _tsc.js:56864-56884
    ///
    /// The commonJS duplicated-export and export=-type-annotation arms
    /// are JS-only (dead in TS files): the live body is
    /// resolveAlias → Value-flagged target's type, else errorType.
    pub(crate) fn get_type_of_alias(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(cached);
        }
        if !self.push_type_resolution(
            crate::state::ResolutionTarget::Symbol(symbol),
            tsrs2_types::TypeSystemPropertyName::TYPE,
        ) {
            return Ok(self.tables.intrinsics.error);
        }
        let computed = (|state: &mut Self| -> CheckResult2<TypeId> {
            let target_symbol = state.resolve_alias(symbol)?;
            let target_flags = state.get_symbol_flags_of(target_symbol)?;
            if target_flags.intersects(SymbolFlags::VALUE) {
                state.get_type_of_symbol(target_symbol)
            } else {
                Ok(state.tables.intrinsics.error)
            }
        })(self);
        let computed = match computed {
            Ok(computed) => computed,
            Err(unsupported) => {
                self.pop_type_resolution();
                return Err(unsupported);
            }
        };
        let computed = if self.pop_type_resolution() {
            computed
        } else {
            self.report_circularity_error(symbol)
        };
        if let Some(already) = self.links.symbol(symbol).type_of_symbol.resolved() {
            return Ok(already);
        }
        self.links
            .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(computed));
        Ok(computed)
    }

    /// tsc-port: getSymbolIfSameReference @6.0.3
    /// tsc-hash: 908084bf7d1f72b02a8256627f01987eb8cd0a6897b9c7027f0cac3f156f5d3d
    /// tsc-span: _tsc.js:50084-50088
    fn get_symbol_if_same_reference(
        &mut self,
        s1: SymbolId,
        s2: SymbolId,
    ) -> CheckResult2<Option<SymbolId>> {
        let merged1 = self.get_merged_symbol(s1);
        let resolved1 = self.resolve_symbol_ex(Some(merged1), false)?;
        let merged2 = self.get_merged_symbol(s2);
        let resolved2 = self.resolve_symbol_ex(Some(merged2), false)?;
        if resolved1.map(|symbol| self.get_merged_symbol(symbol))
            == resolved2.map(|symbol| self.get_merged_symbol(symbol))
        {
            Ok(Some(s1))
        } else {
            Ok(None)
        }
    }

    // ================================================================
    // Module resolution (the 2307 band)
    // ================================================================

    /// tsc-port: resolveExternalModuleName @6.0.3
    /// tsc-hash: 9d35733bd4ecb68f5802ad505327c208fc59e122e00cbc2633f50e1f3c3d4f9a
    /// tsc-span: _tsc.js:49465-49469
    pub(crate) fn resolve_external_module_name(
        &mut self,
        location: NodeId,
        module_reference_expression: NodeId,
        ignore_errors: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let is_classic = self.options.emit_module_resolution_kind() == 1;
        let error_message = match self
            .get_cannot_resolve_module_name_error_for_specific_module(module_reference_expression)
        {
            Some(message) => message,
            None if is_classic => {
                &diagnostics::Cannot_find_module_0_Did_you_mean_to_set_the_moduleResolution_option_to_nodenext_or_to_add_aliases_to_the_paths_option
            }
            None => &diagnostics::Cannot_find_module_0_or_its_corresponding_type_declarations,
        };
        self.resolve_external_module_name_worker(
            location,
            module_reference_expression,
            if ignore_errors {
                None
            } else {
                Some(error_message)
            },
            ignore_errors,
            /*is_for_augmentation*/ false,
        )
    }

    /// tsc-port: resolveExternalModuleNameWorker @6.0.3
    /// tsc-hash: 06af42ad3f426366f3ce8859410e119e7d429da0ed86b7094aac49115437af9a
    /// tsc-span: _tsc.js:49470-49472
    pub(crate) fn resolve_external_module_name_worker(
        &mut self,
        location: NodeId,
        module_reference_expression: NodeId,
        module_not_found_error: Option<&'static DiagnosticMessage>,
        ignore_errors: bool,
        is_for_augmentation: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let text = match self.data_of(module_reference_expression) {
            NodeData::StringLiteral(data) => data.text.clone(),
            NodeData::NoSubstitutionTemplateLiteral(data) => data.text.clone(),
            _ => return Ok(None),
        };
        let error_node = if !ignore_errors {
            Some(module_reference_expression)
        } else {
            None
        };
        self.resolve_external_module(
            location,
            &text,
            module_not_found_error,
            error_node,
            is_for_augmentation,
        )
    }

    /// tsc-port: resolveExternalModule @6.0.3
    /// tsc-hash: c60386d4343175ebc820e0dbc23c0c2fde8196c47265af66b3023e05432cc820
    /// tsc-span: _tsc.js:49473-49663
    ///
    /// Reduced at the modeled defaults: project-reference redirects,
    /// resolveJsonModule, rewriteRelativeImportExtensions, external
    /// library (node_modules) resolution and its implicit-any rows,
    /// and the alternateResult chain all reduce to nothing — the
    /// program resolver never produces those shapes
    /// (program-and-modules.md §2).
    /// KNOWN-GAP since M4 (m4-review B16): the mode machinery
    /// (impliedNodeFormat, the Node16..Node18 sync-import 1471/1479
    /// rows, resolution-mode overrides) does NOT reduce — the old
    /// "resolver never makes that shape" claim was false
    /// (probe_module_candidates resolves .mts/.cts; tsc probed).
    /// M7-owned, sequenced AFTER the impliedNodeFormat tri-state
    /// (review A10: today's CommonJs fallback becomes
    /// Option<mode>) — m7-tail-steps.md 8.1 module-band note.
    /// LIVE rows: @types/ redirect,
    /// ambient modules, in-program resolution + getResolutionDiagnostic
    /// jsx row, ts-extension rows (5097/2846 family), File_0_is_not_a_
    /// module, pattern ambient modules, and the 2307/2792 tail.
    pub(crate) fn resolve_external_module(
        &mut self,
        location: NodeId,
        module_reference: &str,
        module_not_found_error: Option<&'static DiagnosticMessage>,
        error_node: Option<NodeId>,
        is_for_augmentation: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        if let Some(error_node) = error_node {
            if let Some(without_prefix) = module_reference.strip_prefix("@types/") {
                self.error_at(
                    Some(error_node),
                    &diagnostics::Cannot_import_type_declaration_files_Consider_importing_0_instead_of_1,
                    &[without_prefix, module_reference],
                );
            }
        }
        if let Some(ambient_module) =
            self.try_find_ambient_module(module_reference, /*with_augmentations*/ true)
        {
            return Ok(Some(ambient_module));
        }
        let resolution = self.resolve_program_module(location, module_reference);
        if let ProgramModuleResolution::Resolved(resolved) = resolution {
            let source = self.binder.source(resolved.file_index);
            let resolution_diagnostic = if error_node.is_some() && resolved.is_tsx {
                // getResolutionDiagnostic (12708-region) reduced: the
                // program set is .ts/.tsx/.d.ts only, so the jsx row is
                // the single live arm.
                if self.options.jsx.unwrap_or(0) == 0 {
                    Some(&diagnostics::Module_0_was_resolved_to_1_but_jsx_is_not_set)
                } else {
                    None
                }
            } else {
                None
            };
            let resolved_file_name = source.file_name.clone();
            if let Some(resolution_diagnostic) = resolution_diagnostic {
                self.error_at(
                    error_node,
                    resolution_diagnostic,
                    &[module_reference, &resolved_file_name],
                );
            }
            if resolved.resolved_using_ts_extension
                && Self::is_declaration_file_name(module_reference)
            {
                if let Some(error_node) = error_node {
                    if !self.import_location_is_type_only(location) {
                        let ts_extension =
                            Self::try_extract_ts_extension(module_reference).unwrap_or(".d.ts");
                        let suggestion =
                            self.suggested_import_source(location, module_reference, ts_extension);
                        self.error_at(
                            Some(error_node),
                            &diagnostics::A_declaration_file_cannot_be_imported_without_import_type_Did_you_mean_to_import_an_implementation_file_0_instead,
                            &[&suggestion],
                        );
                    }
                }
            } else if resolved.resolved_using_ts_extension
                && self.options.allow_importing_ts_extensions != Some(true)
                && !self.binder.source_of_node(location).is_declaration_file
            {
                // shouldAllowImportingTsExtension: the option OR a
                // declaration-file importer legalizes the extension
                // (bundlerImportTsExtensions pins the .d.ts face).
                if let Some(error_node) = error_node {
                    if !self.import_location_is_type_only(location) {
                        let ts_extension =
                            Self::try_extract_ts_extension(module_reference).unwrap_or(".ts");
                        self.error_at(
                            Some(error_node),
                            &diagnostics::An_import_path_can_only_end_with_a_0_extension_when_allowImportingTsExtensions_is_enabled,
                            &[ts_extension],
                        );
                    }
                }
            }
            let root = source.root;
            if let Some(file_symbol) = self.binder.node_symbol(root) {
                return Ok(Some(self.get_merged_symbol(file_symbol)));
            }
            if let (Some(error_node), Some(_)) = (error_node, module_not_found_error) {
                if !self.is_side_effect_import(error_node) {
                    self.error_at(
                        Some(error_node),
                        &diagnostics::File_0_is_not_a_module,
                        &[&resolved_file_name],
                    );
                }
            }
            return Ok(None);
        }
        if !self.pattern_ambient_modules.is_empty() {
            if let Some(symbol) = self.find_best_pattern_match(module_reference) {
                if let Some(&augmentation) = self
                    .pattern_ambient_module_augmentations
                    .get(module_reference)
                {
                    return Ok(Some(self.get_merged_symbol(augmentation)));
                }
                return Ok(Some(self.get_merged_symbol(symbol)));
            }
        }
        // Ambient external-module declarations in .d.ts files are
        // permitted to introduce an otherwise unresolved module. A
        // .ts module augmentation still reports 2664.
        if is_for_augmentation
            && matches!(resolution, ProgramModuleResolution::Missed)
            && self.binder.source_of_node(location).is_declaration_file
        {
            return Ok(None);
        }
        let Some(error_node) = error_node else {
            return Ok(None);
        };
        // The Node16/NodeNext ESM extensionless-relative rows
        // (49630-49640) come BEFORE the suppression check. tsc's
        // hasExtension predicate accepts any dot in the basename, not
        // only extensions understood by the TypeScript resolver.
        let module_resolution_kind = self.options.emit_module_resolution_kind();
        if matches!(module_resolution_kind, 3 | 99)
            && Self::is_external_module_name_relative(module_reference)
            && !Self::has_extension(module_reference)
            && self.resolution_mode_for_usage(location) == ModuleResolutionMode::EsNext
        {
            let diagnostics_before = self.diagnostics.len();
            if let Some(suggested) = self.suggested_extension_for(location, module_reference) {
                let suggestion = format!("{module_reference}{suggested}");
                self.error_at(
                    Some(error_node),
                    &diagnostics::Relative_import_paths_need_explicit_file_extensions_in_ECMAScript_imports_when_moduleResolution_is_node16_or_nodenext_Did_you_mean_0,
                    &[&suggestion],
                );
            } else {
                self.error_at(
                    Some(error_node),
                    &diagnostics::Relative_import_paths_need_explicit_file_extensions_in_ECMAScript_imports_when_moduleResolution_is_node16_or_nodenext_Consider_adding_an_extension_to_the_import_path,
                    &[],
                );
            }
            if self.is_in_js_file(error_node) {
                self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
            }
            return Ok(None);
        }
        if matches!(resolution, ProgramModuleResolution::Suppressed) {
            // tsrs-native FP=0 rule: the miss sits behind unmodeled
            // resolution machinery (node_modules/baseUrl-paths/allowJs
            // targets) — tsc may resolve, so the 2307 tail stays
            // silent (FN-side; ledger).
            if is_for_augmentation {
                // The skipped merge leaves member tables thinner than
                // tsc's. Record only members the augmentation could
                // have supplied; downstream property misses must not
                // suppress unrelated names program-wide.
                if let Some(augmentation) = self
                    .parent_of(location)
                    .filter(|&parent| self.kind_of(parent) == SyntaxKind::ModuleDeclaration)
                {
                    self.record_unresolved_module_augmentation(augmentation, module_reference);
                }
            }
            return Ok(None);
        }
        if let Some(module_not_found_error) = module_not_found_error {
            if is_for_augmentation {
                // The untyped-resolution face is dead (the resolver
                // only yields typed results); the NOT-FOUND face rides
                // the plain tail below in tsc too.
            }
            let diagnostics_before = self.diagnostics.len();
            self.error_at(
                Some(error_node),
                module_not_found_error,
                &[module_reference],
            );
            if self.is_in_js_file(error_node) {
                self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
            }
        }
        Ok(None)
    }

    /// tsrs-native: containment scope for a resolver-suppressed module
    /// augmentation. Preserve each augmentation container symbol
    /// rather than flattening its members to strings: resolved member
    /// tables then retain computed names and index signatures.
    fn record_unresolved_module_augmentation(
        &mut self,
        augmentation: NodeId,
        module_reference: &str,
    ) {
        let Some(root) = self.node_symbol(augmentation) else {
            return;
        };
        let augmentation_file = self.binder.source_of_node(augmentation).file_name.clone();
        let mut worklist = vec![(root, Vec::new())];
        let mut seen = std::collections::HashSet::new();
        while let Some((current, path)) = worklist.pop() {
            if !seen.insert(current) {
                continue;
            }
            let entry = crate::state::UnresolvedModuleAugmentation {
                module_reference: module_reference.to_owned(),
                augmentation_file: augmentation_file.clone(),
                container_path: path.clone(),
                container_symbol: current,
            };
            let entries = self
                .unresolved_module_augmentations
                .entry(path.clone())
                .or_default();
            if !entries.contains(&entry) {
                entries.push(entry);
            }
            let children = self
                .binder
                .symbol(current)
                .exports
                .iter()
                .map(|(name, &child)| {
                    let mut child_path = path.clone();
                    child_path.push(name.clone());
                    (child, child_path)
                })
                .collect::<Vec<_>>();
            worklist.extend(children);
        }
    }

    /// tsrs-native: whether a declaration source is a plausible target
    /// of an unresolved module reference. Bare specifiers are anchored
    /// to their node_modules package (including DefinitelyTyped
    /// layout); relative/baseUrl-like references compare normalized TS
    /// stems.
    pub(crate) fn unresolved_module_reference_matches_source(
        &self,
        augmentation_file: &str,
        module_reference: &str,
        source_file: &str,
    ) -> bool {
        let source = Self::normalize_program_path(source_file, "");
        let augmentation = Self::normalize_program_path(augmentation_file, "");
        let reference = module_reference
            .strip_prefix("node:")
            .unwrap_or(module_reference);
        let bare =
            !Self::is_external_module_name_relative(reference) && !reference.starts_with('/');
        if bare {
            // Node core declarations live under @types/node rather than
            // @types/<module>. Keep the exact core submodule (`fs` vs
            // `http`, `fs/promises` vs `timers/promises`) in the match.
            if Self::is_node_core_module(module_reference) {
                if let Some(root) = Self::node_modules_package_root(&source, "@types/node") {
                    return self
                        .nearest_visible_package_root(&augmentation, "@types/node")
                        .is_some_and(|nearest| nearest == root)
                        && Self::package_subpath_matches_source(&root, reference, &source);
                }
                return false;
            }

            let (package, subpath) = Self::bare_package_parts(reference);
            if let Some(root) = Self::node_modules_package_root(&source, &package) {
                if self
                    .nearest_visible_package_root(&augmentation, &package)
                    .is_some_and(|nearest| nearest == root)
                    && Self::package_subpath_matches_source(&root, &subpath, &source)
                {
                    return true;
                }
            }
            let types_package = match package.split_once('/') {
                Some((scope, name)) if scope.starts_with('@') => {
                    format!("@types/{}__{name}", scope.trim_start_matches('@'))
                }
                _ => format!("@types/{package}"),
            };
            if let Some(root) = Self::node_modules_package_root(&source, &types_package) {
                if self
                    .nearest_visible_package_root(&augmentation, &types_package)
                    .is_some_and(|nearest| nearest == root)
                    && Self::package_subpath_matches_source(&root, &subpath, &source)
                {
                    return true;
                }
            }
            // Without baseUrl, a bare specifier cannot directly name a
            // same-spelled workspace file. package.json self-name and
            // paths mappings remain genuinely unidentified; treating
            // every `pkg.ts` as their target recreates the name-only
            // false-negative this provenance gate exists to prevent.
            if self.options.base_url.is_none() {
                return false;
            }
        }

        let augmentation_dir = augmentation
            .rfind('/')
            .map_or("", |position| &augmentation[..position]);
        let candidate =
            if Self::is_external_module_name_relative(reference) || reference.starts_with('/') {
                Self::normalize_program_path(reference, augmentation_dir)
            } else if let Some(base_url) = &self.options.base_url {
                Self::normalize_program_path(reference, base_url)
            } else {
                Self::normalize_program_path(reference, "")
            };
        Self::module_source_stem(&source) == Self::module_source_stem(&candidate)
    }

    fn bare_package_parts(reference: &str) -> (String, String) {
        let package_segments = if reference.starts_with('@') { 2 } else { 1 };
        let mut segments = reference.split('/');
        let package = segments
            .by_ref()
            .take(package_segments)
            .collect::<Vec<_>>()
            .join("/");
        (package, segments.collect::<Vec<_>>().join("/"))
    }

    fn node_modules_package_root(source: &str, package: &str) -> Option<String> {
        let marker = format!("/node_modules/{package}");
        source
            .match_indices(&marker)
            .filter(|(position, _)| {
                source
                    .as_bytes()
                    .get(position + marker.len())
                    .is_none_or(|&byte| byte == b'/')
            })
            .map(|(position, _)| source[..position + marker.len()].to_owned())
            .last()
    }

    fn nearest_visible_package_root(
        &self,
        augmentation_file: &str,
        package: &str,
    ) -> Option<String> {
        let cache_key = (augmentation_file.to_owned(), package.to_owned());
        if let Some(cached) = self.unresolved_package_root_cache.borrow().get(&cache_key) {
            return cached.clone();
        }
        let marker = format!("/node_modules/{package}");
        let nearest = self
            .host_file_paths
            .iter()
            .filter_map(|path| {
                let root = Self::node_modules_package_root(path, package)?;
                let owner = root.strip_suffix(&marker)?;
                let visible = owner.is_empty()
                    || augmentation_file == owner
                    || augmentation_file
                        .strip_prefix(owner)
                        .is_some_and(|suffix| suffix.starts_with('/'));
                visible.then_some((owner.len(), root))
            })
            .max_by_key(|(owner_length, _)| *owner_length)
            .map(|(_, root)| root);
        self.unresolved_package_root_cache
            .borrow_mut()
            .insert(cache_key, nearest.clone());
        nearest
    }

    fn package_subpath_matches_source(package_root: &str, subpath: &str, source: &str) -> bool {
        // A root entry may be redirected by package.json's types/exports
        // field, which this resolver intentionally does not parse. The
        // selected package instance is still authoritative. Subpaths,
        // however, must keep their spelling so `pkg/a` cannot claim a
        // symbol declared by `pkg/b`.
        if subpath.is_empty() {
            return true;
        }
        let Some(relative) = source.strip_prefix(package_root) else {
            return false;
        };
        let relative = relative.trim_start_matches('/');
        Self::module_source_stem(relative) == subpath
    }

    fn module_source_stem(path: &str) -> String {
        let mut stem = path.to_owned();
        for extension in [
            ".d.json.ts",
            ".d.mts",
            ".d.cts",
            ".d.ts",
            ".mts",
            ".cts",
            ".tsx",
            ".ts",
            ".jsx",
            ".js",
            ".json",
        ] {
            if stem.ends_with(extension) {
                stem.truncate(stem.len() - extension.len());
                break;
            }
        }
        if stem.ends_with("/index") {
            stem.truncate(stem.len() - "/index".len());
        }
        stem
    }

    /// tsc-port: tryFindAmbientModule @6.0.3
    /// tsc-hash: 1f3cc9031465a237b7ec0d5712660a1201e66cb8853b050b360e2de131a1a314
    /// tsc-span: _tsc.js:59499-59505
    pub(crate) fn try_find_ambient_module(
        &mut self,
        module_name: &str,
        with_augmentations: bool,
    ) -> Option<SymbolId> {
        if Self::is_external_module_name_relative(module_name) {
            return None;
        }
        let quoted = format!("\"{module_name}\"");
        let symbol = self.globals.get(&quoted).copied()?;
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::VALUE_MODULE)
        {
            return None;
        }
        if with_augmentations {
            Some(self.get_merged_symbol(symbol))
        } else {
            Some(symbol)
        }
    }

    /// tsc-port: isExternalModuleNameRelative @6.0.3
    /// tsc-hash: e5546324dce58e277ab9df485e26bb2c9cafa5a7e7b154366be6fc45784ad14d
    /// tsc-span: _tsc.js:11234-11236
    fn is_external_module_name_relative(module_name: &str) -> bool {
        module_name.starts_with("./")
            || module_name.starts_with("../")
            || module_name == "."
            || module_name == ".."
    }

    /// tsc-port: getCannotResolveModuleNameErrorForSpecificModule @6.0.3
    /// tsc-hash: ee2ad998f3261c273071d09a3eccf2dff25ca6b077a2cecde706808b06672523
    /// tsc-span: _tsc.js:69377-69388
    ///
    /// usesWildcardTypes reads the `types` option — unmodeled-absent →
    /// constant false → the "and then add node to the types field"
    /// flavor.
    fn get_cannot_resolve_module_name_error_for_specific_module(
        &self,
        module_name: NodeId,
    ) -> Option<&'static DiagnosticMessage> {
        let text = match self.data_of(module_name) {
            NodeData::StringLiteral(data) => &data.text,
            _ => return None,
        };
        if Self::is_node_core_module(text) {
            Some(&diagnostics::Cannot_find_name_0_Do_you_need_to_install_type_definitions_for_node_Try_npm_i_save_dev_types_node_and_then_add_node_to_the_types_field_in_your_tsconfig)
        } else {
            None
        }
    }

    /// tsc-port: nodeCoreModules @6.0.3
    /// tsc-hash: 0e632ec6b143db94f7d3289fd8c884f1e2d5cba30ad853d6cd66d2e6f35149da
    /// tsc-span: _tsc.js:19947-20016
    fn is_node_core_module(name: &str) -> bool {
        const UNPREFIXED: &[&str] = &[
            "assert",
            "assert/strict",
            "async_hooks",
            "buffer",
            "child_process",
            "cluster",
            "console",
            "constants",
            "crypto",
            "dgram",
            "diagnostics_channel",
            "dns",
            "dns/promises",
            "domain",
            "events",
            "fs",
            "fs/promises",
            "http",
            "http2",
            "https",
            "inspector",
            "inspector/promises",
            "module",
            "net",
            "os",
            "path",
            "path/posix",
            "path/win32",
            "perf_hooks",
            "process",
            "punycode",
            "querystring",
            "readline",
            "readline/promises",
            "repl",
            "stream",
            "stream/consumers",
            "stream/promises",
            "stream/web",
            "string_decoder",
            "sys",
            "timers",
            "timers/promises",
            "tls",
            "trace_events",
            "tty",
            "url",
            "util",
            "util/types",
            "v8",
            "vm",
            "wasi",
            "worker_threads",
            "zlib",
        ];
        const EXCLUSIVELY_PREFIXED: &[&str] = &[
            "node:quic",
            "node:sea",
            "node:sqlite",
            "node:test",
            "node:test/reporters",
        ];
        if UNPREFIXED.contains(&name) || EXCLUSIVELY_PREFIXED.contains(&name) {
            return true;
        }
        name.strip_prefix("node:")
            .is_some_and(|unprefixed| UNPREFIXED.contains(&unprefixed))
    }

    /// tsrs-native: the host.getResolvedModule seam over the in-memory
    /// program file set (program-and-modules.md §2): relative/absolute
    /// specifiers resolve against the importing file's directory with
    /// the TS-family candidate order (exact-with-extension first,
    /// marking resolvedUsingTsExtension); directory candidates probe
    /// the index.* family (non-Classic only); .js-family specifiers
    /// substitute their TS twins (non-Classic only); non-relative
    /// specifiers walk up the directory tree under Classic and probe
    /// baseUrl, and are otherwise only resolvable through machinery
    /// the port does not model (node_modules, paths) — those misses
    /// SUPPRESS rather than fabricate 2307 (FP=0 rule; risk §14.7).
    /// Case SENSITIVE (the oracle host's useCaseSensitiveFileNames=
    /// true).
    fn resolve_program_module(
        &self,
        location: NodeId,
        module_reference: &str,
    ) -> ProgramModuleResolution {
        if module_reference.is_empty() {
            return ProgramModuleResolution::Missed;
        }
        let importing_index = self.binder.file_index_of_node(location);
        let importer =
            Self::normalize_program_path(&self.binder.source(importing_index).file_name, "");
        let importer_dir = match importer.rfind('/') {
            Some(position) => importer[..position].to_owned(),
            None => String::new(),
        };
        let is_classic = self.options.emit_module_resolution_kind() == 1;
        let relative = Self::is_external_module_name_relative(module_reference)
            || module_reference.starts_with('/');
        if relative {
            let candidate = Self::normalize_program_path(module_reference, &importer_dir);
            // "." / ".." / trailing-slash specifiers reference the
            // DIRECTORY: only the index.* family resolves them
            // (importFromDot pins "." → ./index.ts over ./<dir>.ts).
            let directory_reference = module_reference == "."
                || module_reference == ".."
                || module_reference.ends_with('/');
            let node_mode_resolution = matches!(self.options.emit_module_resolution_kind(), 3 | 99);
            let extensionless = !Self::has_extension(module_reference);
            if node_mode_resolution {
                let resolution_mode = self.resolution_mode_for_usage(location);
                // Node ESM never probes an omitted extension or a
                // directory index, for either static imports or
                // import() calls. The failure tail selects 2834/2835
                // for extensionless specifiers and 2307 otherwise.
                if resolution_mode == ModuleResolutionMode::EsNext
                    && (extensionless || directory_reference)
                {
                    return ProgramModuleResolution::Missed;
                }
                // package.json among the host inputs makes an unknown
                // implied format mode-dependent. Directory references
                // also stay undecidable in a concrete CJS mode because
                // package `main` can redirect the index probe. Either
                // case could fabricate a resolution target or a
                // 2307/2835 failure face, so suppress (FN, ledger).
                let has_package_json = self
                    .host_file_paths
                    .iter()
                    .any(|path| path.ends_with("/package.json"));
                if extensionless
                    && has_package_json
                    && (resolution_mode == ModuleResolutionMode::Unknown || directory_reference)
                {
                    return ProgramModuleResolution::Suppressed;
                }
            }
            if directory_reference {
                if !is_classic {
                    for extension in [".ts", ".tsx", ".d.ts"] {
                        let probed =
                            format!("{}/index{extension}", candidate.trim_end_matches('/'));
                        if let Some(&index) = self.program_path_index.get(&probed) {
                            return ProgramModuleResolution::Resolved(ResolvedProgramModule {
                                file_index: index,
                                resolved_using_ts_extension: false,
                                is_tsx: probed.ends_with(".tsx"),
                            });
                        }
                    }
                }
                if self.miss_is_undecidable(&candidate) {
                    return ProgramModuleResolution::Suppressed;
                }
                return ProgramModuleResolution::Missed;
            }
            if let Some(resolved) = self.probe_module_candidates(&candidate, is_classic) {
                return ProgramModuleResolution::Resolved(resolved);
            }
            if self.miss_is_undecidable(&candidate) {
                return ProgramModuleResolution::Suppressed;
            }
            ProgramModuleResolution::Missed
        } else {
            if is_classic {
                // Classic non-relative: walk up from the importing
                // directory probing the candidate at each level.
                let mut dir = importer_dir;
                loop {
                    let candidate = Self::normalize_program_path(module_reference, &dir);
                    if let Some(resolved) = self.probe_module_candidates(&candidate, is_classic) {
                        return ProgramModuleResolution::Resolved(resolved);
                    }
                    match dir.rfind('/') {
                        Some(0) if dir.len() > 1 => dir = "/".to_owned(),
                        Some(position) if position > 0 => dir.truncate(position),
                        _ if !dir.is_empty() && dir != "/" => dir = "/".to_owned(),
                        _ => break,
                    }
                }
            }
            // baseUrl-relative candidate (classic + node-ish + bundler
            // all honor baseUrl; paths mapping stays unmodeled).
            if let Some(base_url) = &self.options.base_url {
                let base = Self::normalize_program_path(base_url, "");
                let candidate = Self::normalize_program_path(module_reference, &base);
                if let Some(resolved) = self.probe_module_candidates(&candidate, is_classic) {
                    return ProgramModuleResolution::Resolved(resolved);
                }
                let candidate_prefix = format!("{}/", candidate.trim_end_matches('/'));
                if self
                    .host_file_paths
                    .iter()
                    .any(|path| path.starts_with(&candidate_prefix))
                {
                    return ProgramModuleResolution::Suppressed;
                }
            }
            // Keep only genuinely plausible package-resolution misses
            // in the undecidable band. An unrelated node_modules entry
            // or package.json cannot resolve this specifier and must
            // not hide tsc's 2307.
            let package_name = Self::package_name_from_module_reference(module_reference);
            let node_modules_marker = package_name
                .as_deref()
                .map(|name| format!("/node_modules/{name}/"));
            let types_package_marker = package_name.as_deref().map(|name| {
                let types_name = if let Some(scoped) = name.strip_prefix('@') {
                    scoped.replace('/', "__")
                } else {
                    name.to_owned()
                };
                format!("/node_modules/@types/{types_name}/")
            });
            let has_matching_node_modules_package =
                node_modules_marker.as_deref().is_some_and(|marker| {
                    self.host_file_paths
                        .iter()
                        .any(|path| path.contains(marker))
                }) || types_package_marker.as_deref().is_some_and(|marker| {
                    self.host_file_paths
                        .iter()
                        .any(|path| path.contains(marker))
                });
            let has_matching_package_scope = self
                .nearest_package_name_for_file(&importer)
                .is_some_and(|name| {
                    package_name
                        .as_deref()
                        .is_some_and(|package_name| name == package_name)
                });
            let has_package_import_scope = module_reference.starts_with('#')
                && self.nearest_package_name_for_file(&importer).is_some();
            if has_matching_node_modules_package
                || has_matching_package_scope
                || has_package_import_scope
            {
                return ProgramModuleResolution::Suppressed;
            }
            ProgramModuleResolution::Missed
        }
    }

    /// tsc-port: checkExternalEmitHelpers @6.0.3
    /// tsc-hash: 5f72636b67358cec9e7c94abf473b9595b180848fb2dbf04231af134818b46e6
    /// tsc-span: _tsc.js:88907-88944
    ///
    /// The host boundary is the same three-way in-memory resolver used
    /// by ordinary imports. An ambient or in-program `tslib` is
    /// authoritative; a definite miss reports 2354; a package-host
    /// miss remains FN-side so node_modules cannot fabricate 2343 or
    /// 2807. requestedExternalEmitHelpers is module-wide in tsc and is
    /// therefore keyed by the resolved module symbol here.
    pub(crate) fn check_external_emit_helpers(
        &mut self,
        location: NodeId,
        helpers: u32,
    ) -> CheckResult2<()> {
        if self.options.import_helpers != Some(true)
            || !self.is_effective_external_module(location)
            || tsrs2_types::NodeFlags::from_bits(self.node_flags(location))
                .intersects(tsrs2_types::NodeFlags::AMBIENT)
        {
            return Ok(());
        }

        let source_root = self.binder.source_of_node(location).root;
        let helpers_module = if let Some(&cached) = self.external_helpers_modules.get(&source_root)
        {
            cached
        } else {
            let resolved = if let Some(ambient) =
                self.try_find_ambient_module("tslib", /*with_augmentations*/ true)
            {
                Some(ambient)
            } else {
                match self.resolve_program_module(location, "tslib") {
                    ProgramModuleResolution::Resolved(resolved) => {
                        let root = self.binder.source(resolved.file_index).root;
                        self.binder
                            .node_symbol(root)
                            .map(|symbol| self.get_merged_symbol(symbol))
                    }
                    ProgramModuleResolution::Suppressed => None,
                    ProgramModuleResolution::Missed => {
                        let diagnostics_before = self.diagnostics.len();
                        self.error_at(
                            Some(location),
                            &diagnostics::This_syntax_requires_an_imported_helper_but_module_0_cannot_be_found,
                            &["tslib"],
                        );
                        if self.is_in_js_file(location) {
                            self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
                        }
                        None
                    }
                }
            };
            self.external_helpers_modules.insert(source_root, resolved);
            resolved
        };
        let Some(helpers_module) = helpers_module else {
            return Ok(());
        };

        let requested = self
            .requested_external_emit_helpers
            .get(&helpers_module)
            .copied()
            .unwrap_or(0);
        let unchecked = helpers & !requested;
        if unchecked == 0 {
            return Ok(());
        }

        let exports = self.get_exports_of_module(helpers_module)?;
        let legacy_decorators = self.options.experimental_decorators;
        let mut helper = 1u32;
        while helper <= EMIT_HELPER_ADD_DISPOSABLE_RESOURCE_AND_DISPOSE_RESOURCES {
            if unchecked & helper != 0 {
                for &name in Self::external_emit_helper_names(helper, legacy_decorators) {
                    let exported = exports
                        .get(&escape_leading_underscores(name))
                        .copied()
                        .filter(|&symbol| {
                            self.binder
                                .symbol(symbol)
                                .flags
                                .intersects(SymbolFlags::VALUE)
                        });
                    let symbol = self.resolve_symbol_ex(exported, false)?;
                    let Some(symbol) = symbol.filter(|&symbol| symbol != self.unknown_symbol)
                    else {
                        let diagnostics_before = self.diagnostics.len();
                        self.error_at(
                            Some(location),
                            &diagnostics::This_syntax_requires_an_imported_helper_named_1_which_does_not_exist_in_0_Consider_upgrading_your_version_of_0,
                            &["tslib", name],
                        );
                        if self.is_in_js_file(location) {
                            self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
                        }
                        continue;
                    };
                    let required_parameter_count = match helper {
                        524_288 => Some(4usize),
                        1_048_576 => Some(5usize),
                        EMIT_HELPER_SPREAD_ARRAY => Some(3usize),
                        _ => None,
                    };
                    if let Some(required) = required_parameter_count {
                        let mut compatible = false;
                        for signature in self.get_signatures_of_symbol(Some(symbol))? {
                            if self.get_parameter_count(signature)? >= required {
                                compatible = true;
                                break;
                            }
                        }
                        if !compatible {
                            let required = required.to_string();
                            let diagnostics_before = self.diagnostics.len();
                            self.error_at(
                                Some(location),
                                &diagnostics::This_syntax_requires_an_imported_helper_named_1_with_2_parameters_which_is_not_compatible_with_the_one_in_0_Consider_upgrading_your_version_of_0,
                                &["tslib", name, &required],
                            );
                            if self.is_in_js_file(location) {
                                self.mark_non_jsdoc_js_diagnostics_since(diagnostics_before);
                            }
                        }
                    }
                }
            }
            helper <<= 1;
        }
        self.requested_external_emit_helpers
            .insert(helpers_module, requested | helpers);
        Ok(())
    }

    fn external_emit_helper_names(helper: u32, legacy_decorators: bool) -> &'static [&'static str] {
        match helper {
            1 => &["__extends"],
            2 => &["__assign"],
            4 => &["__rest"],
            EMIT_HELPER_DECORATE if legacy_decorators => &["__decorate"],
            EMIT_HELPER_DECORATE => &["__esDecorate", "__runInitializers"],
            16 => &["__metadata"],
            32 => &["__param"],
            64 => &["__awaiter"],
            128 => &["__generator"],
            256 => &["__values"],
            EMIT_HELPER_READ => &["__read"],
            EMIT_HELPER_SPREAD_ARRAY => &["__spreadArray"],
            2_048 => &["__await"],
            4_096 => &["__asyncGenerator"],
            8_192 => &["__asyncDelegator"],
            16_384 => &["__asyncValues"],
            EMIT_HELPER_EXPORT_STAR => &["__exportStar"],
            EMIT_HELPER_IMPORT_STAR => &["__importStar"],
            EMIT_HELPER_IMPORT_DEFAULT => &["__importDefault"],
            262_144 => &["__makeTemplateObject"],
            524_288 => &["__classPrivateFieldGet"],
            1_048_576 => &["__classPrivateFieldSet"],
            2_097_152 => &["__classPrivateFieldIn"],
            EMIT_HELPER_SET_FUNCTION_NAME => &["__setFunctionName"],
            EMIT_HELPER_PROP_KEY => &["__propKey"],
            EMIT_HELPER_ADD_DISPOSABLE_RESOURCE_AND_DISPOSE_RESOURCES => {
                &["__addDisposableResource", "__disposeResources"]
            }
            _ => &[],
        }
    }

    /// host.getEmitModuleFormatOfFile(location) < ModuleKind.System.
    /// Node-flavored module kinds use their per-file package format;
    /// ordinary kinds use the explicit/computed module kind directly.
    /// tsrs-native: reduction over the in-memory host's modeled module
    /// format seam.
    pub(crate) fn emit_module_format_is_pre_system(&self, location: NodeId) -> bool {
        let module_kind = self.options.emit_module_kind();
        if (100..=199).contains(&module_kind) {
            self.implied_node_format_for_file(location) == ModuleResolutionMode::CommonJs
        } else {
            module_kind < 4
        }
    }

    fn package_name_from_module_reference(module_reference: &str) -> Option<String> {
        if module_reference.starts_with('#') {
            return None;
        }
        let mut parts = module_reference.split('/');
        let first = parts.next()?;
        if first.is_empty() {
            return None;
        }
        if first.starts_with('@') {
            let second = parts.next()?;
            (!second.is_empty()).then(|| format!("{first}/{second}"))
        } else {
            Some(first.to_owned())
        }
    }

    fn nearest_package_name_for_file(&self, file_name: &str) -> Option<&str> {
        let file_name = Self::normalize_program_path(file_name, "");
        let mut directory = file_name
            .rsplit_once('/')
            .map(|(directory, _)| directory)
            .unwrap_or("");
        loop {
            let package_json = if directory.is_empty() {
                "/package.json".to_owned()
            } else {
                format!("{directory}/package.json")
            };
            if let Some(name) = self.host_package_json_names.get(&package_json) {
                return Some(name);
            }
            let Some((parent, _)) = directory.rsplit_once('/') else {
                break;
            };
            directory = parent;
        }
        None
    }

    /// A relative-candidate miss is tsc-undecidable when the stem
    /// exists among the HOST inputs under an extension the port does
    /// not resolve (allowJs .js/.jsx targets, .json under
    /// resolveJsonModule, declaration twins of either) — tsc may
    /// resolve there.
    fn miss_is_undecidable(&self, candidate: &str) -> bool {
        const UNMODELED: &[&str] = &[
            "",
            ".js",
            ".jsx",
            ".json",
            ".d.json.ts",
            ".js.ts",
            ".json.ts",
        ];
        let base = candidate.trim_end_matches('/');
        if UNMODELED.iter().any(|extension| {
            let probed = format!("{base}{extension}");
            self.host_file_paths.contains(&probed)
        }) {
            return true;
        }
        // Directory index misses over the same unmodeled set — and a
        // directory-local package.json (main/exports redirect) makes
        // any directory candidate tsc-resolvable (bundlerDirectory
        // Module / bundlerRelative1 pin the main-redirect faces).
        if ["/index.js", "/index.jsx", "/index.json", "/package.json"]
            .iter()
            .any(|suffix| {
                let probed = format!("{base}{suffix}");
                self.host_file_paths.contains(&probed)
            })
        {
            return true;
        }
        // Arbitrary-extension declaration twin (allowArbitraryExtensions
        // .d.<ext>.ts): "./file.html" may resolve to file.d.html.ts.
        // The arbitrary-extension branch is NOT entered for the
        // resolver's recognized TS/JS extension groups: "./file.js"
        // must not be hidden by file.d.js.ts, nor "./file.d.mts" by
        // file.d.d.mts.ts. In those groups tryAddingExtensions already
        // made the complete authoritative probe above.
        let recognized_ts_js_extension =
            [".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs"]
                .iter()
                .any(|extension| base.ends_with(extension));
        if !recognized_ts_js_extension {
            if let Some(dot) = base.rfind('.') {
                let slash = base.rfind('/').map_or(0, |position| position + 1);
                if dot > slash {
                    let (stem, extension) = base.split_at(dot);
                    let twin = format!("{stem}.d{extension}.ts");
                    if self.host_file_paths.contains(&twin) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The per-candidate extension probe half of the resolver seam
    /// (tsrs-native, split for the walk-up loop).
    fn probe_module_candidates(
        &self,
        candidate: &str,
        is_classic: bool,
    ) -> Option<ResolvedProgramModule> {
        const TS_EXTENSIONS: &[&str] =
            &[".ts", ".tsx", ".d.ts", ".mts", ".cts", ".d.mts", ".d.cts"];
        let lookup = |path: &str| self.program_path_index.get(path).copied();
        let make = |file_index: usize, resolved_using_ts_extension: bool, path: &str| {
            ResolvedProgramModule {
                file_index,
                resolved_using_ts_extension,
                is_tsx: path.ends_with(".tsx") && !path.ends_with(".d.tsx"),
            }
        };
        if self.options.resolve_json_module_effective() && candidate.ends_with(".json") {
            // allowArbitraryExtensions declaration twin: tsc's TYPES
            // extension group tries <stem>.d.json.ts BEFORE the JSON
            // file wins, and the option is unmodeled here — with a
            // twin present the winner is undecidable, so fall through
            // and let miss_is_undecidable route the import to the
            // Suppressed channel (its arbitrary-extension arm already
            // matches the twin). Rendering the JSON shape in that
            // world fabricated a 2322 (FP-gate catch #7).
            let twin = format!("{}.d.json.ts", candidate.trim_end_matches(".json"));
            if !self.program_path_index.contains_key(&twin) {
                if let Some(index) = lookup(candidate) {
                    return Some(make(index, false, candidate));
                }
            }
        }
        // Exact name with a recognized TS-family extension.
        for extension in TS_EXTENSIONS {
            if candidate.ends_with(extension) {
                if let Some(index) = lookup(candidate) {
                    return Some(make(index, true, candidate));
                }
            }
        }
        // Known-extension SUBSTITUTION (loadModuleFromFile's
        // tryAddingExtensions over the stripped base — classic shares
        // it): "./tsx.d.ts" resolves to tsx.tsx, "./dts.js" to
        // dts.d.ts (allowImportingTsExtensions fixture pins all
        // faces).
        for (known, subs) in [
            (".d.ts", &[".ts", ".tsx", ".d.ts"][..]),
            (".ts", &[".ts", ".tsx", ".d.ts"][..]),
            (".tsx", &[".ts", ".tsx", ".d.ts"][..]),
            (".js", &[".ts", ".tsx", ".d.ts"][..]),
            (".jsx", &[".ts", ".tsx", ".d.ts"][..]),
            (".d.mts", &[".mts", ".d.mts"][..]),
            (".mts", &[".mts", ".d.mts"][..]),
            (".mjs", &[".mts", ".d.mts"][..]),
            (".d.cts", &[".cts", ".d.cts"][..]),
            (".cts", &[".cts", ".d.cts"][..]),
            (".cjs", &[".cts", ".d.cts"][..]),
        ] {
            if let Some(base) = candidate.strip_suffix(known) {
                for extension in subs {
                    let probed = format!("{base}{extension}");
                    if let Some(index) = lookup(&probed) {
                        return Some(make(index, false, &probed));
                    }
                }
                break;
            }
        }
        // Extension appends are only the extensionless probe group.
        // Once a recognized extension substitution group has failed,
        // loadModuleFromFile does not then try `<candidate>.ts`
        // (`file.d.ts` must not resolve to `file.d.ts.ts`).
        if !Self::has_extension(candidate) {
            for extension in [".ts", ".tsx", ".d.ts"] {
                let probed = format!("{candidate}{extension}");
                if let Some(index) = lookup(&probed) {
                    return Some(make(index, false, &probed));
                }
            }
        }
        // Directory → index.* family (Classic has no directory
        // resolution). Trailing-slash specifiers ("./") collapse to
        // the bare directory.
        if !is_classic {
            let base = candidate.trim_end_matches('/');
            for extension in [".ts", ".tsx", ".d.ts"] {
                let probed = format!("{base}/index{extension}");
                if let Some(index) = lookup(&probed) {
                    return Some(make(index, false, &probed));
                }
            }
        }
        None
    }

    /// tsrs-native: path normalization for the resolver seam —
    /// absolute-izes against `base_dir` (harness cwd is `/`), collapses
    /// `.`/`..` segments.
    pub(crate) fn normalize_program_path(path: &str, base_dir: &str) -> String {
        let path = path.replace('\\', "/");
        let base_dir = base_dir.replace('\\', "/");
        let joined = if path.starts_with('/') {
            path
        } else {
            format!("{base_dir}/{path}")
        };
        let mut segments: Vec<&str> = Vec::new();
        for segment in joined.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                other => segments.push(other),
            }
        }
        format!("/{}", segments.join("/"))
    }

    fn is_declaration_file_name(name: &str) -> bool {
        name.ends_with(".d.ts")
            || name.ends_with(".d.cts")
            || name.ends_with(".d.mts")
            || (name.ends_with(".ts")
                && name
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(name)
                    .contains(".d."))
    }

    /// tsc hasExtension: any dot in the final path component counts.
    fn has_extension(name: &str) -> bool {
        let trimmed = name.trim_end_matches(['/', '\\']);
        trimmed
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(trimmed)
            .contains('.')
    }

    /// tsc importSyntaxAffectsModuleResolution over the represented
    /// option set. resolvePackageJsonExports/Imports have no explicit
    /// fields yet; their computed defaults are true for Node16,
    /// NodeNext, and Bundler.
    fn import_syntax_affects_module_resolution(&self) -> bool {
        matches!(self.options.emit_module_resolution_kind(), 3 | 99 | 100)
    }

    /// tsc getModeForUsageLocationWorker / getEmitSyntaxForUsageLocationWorker.
    /// A valid type-only resolution-mode override wins before the
    /// compiler-option gate.
    fn resolution_mode_for_usage(&self, location: NodeId) -> ModuleResolutionMode {
        if let Some(mode) = self.resolution_mode_override_for_usage(location) {
            return mode;
        }
        if !self.import_syntax_affects_module_resolution() {
            return ModuleResolutionMode::Unknown;
        }
        if self.has_ancestor_kind(location, SyntaxKind::ImportEqualsDeclaration) {
            return ModuleResolutionMode::CommonJs;
        }
        if self.has_import_call_ancestor(location) {
            let module_kind = self.options.emit_module_kind();
            if (100..=199).contains(&module_kind) || module_kind == 200 {
                return ModuleResolutionMode::EsNext;
            }
            if let Some(mode) = self.implied_resolution_mode_from_extension(location) {
                return mode;
            }
            return if module_kind < 5 {
                ModuleResolutionMode::CommonJs
            } else {
                ModuleResolutionMode::EsNext
            };
        }
        self.static_resolution_mode_for_file(location)
    }

    fn resolution_mode_override_for_usage(&self, location: NodeId) -> Option<ModuleResolutionMode> {
        let mut current = Some(location);
        while let Some(node) = current {
            let attributes = match self.data_of(node) {
                NodeData::ImportDeclaration(data) => {
                    if !self.is_exclusively_type_only_import_or_export(node) {
                        return None;
                    }
                    data.attributes
                }
                NodeData::ExportDeclaration(data) => {
                    if !self.is_exclusively_type_only_import_or_export(node) {
                        return None;
                    }
                    data.attributes
                }
                NodeData::ImportType(data) => data.attributes,
                _ => {
                    current = self.parent_of(node);
                    continue;
                }
            };
            return attributes.and_then(|attributes| {
                match self.parse_resolution_mode_override(attributes) {
                    ResolutionModeOverrideParse::Valid(mode) => Some(mode),
                    _ => None,
                }
            });
        }
        None
    }

    pub(crate) fn implied_resolution_mode_from_extension(
        &self,
        location: NodeId,
    ) -> Option<ModuleResolutionMode> {
        let file_name = &self.binder.source_of_node(location).file_name;
        if file_name.ends_with(".mts") || file_name.ends_with(".mjs") {
            Some(ModuleResolutionMode::EsNext)
        } else if file_name.ends_with(".cts") || file_name.ends_with(".cjs") {
            Some(ModuleResolutionMode::CommonJs)
        } else {
            None
        }
    }

    /// tsrs-native: in-memory host seam for tsc's
    /// host.getImpliedNodeFormatForEmit.
    ///
    /// tsc host.getImpliedNodeFormatForEmit for the in-memory host:
    /// explicit module extensions win; otherwise Node-flavored module
    /// kinds use the nearest package.json scope and default to CommonJS.
    pub(crate) fn implied_node_format_for_file(&self, location: NodeId) -> ModuleResolutionMode {
        if let Some(mode) = self.implied_resolution_mode_from_extension(location) {
            return mode;
        }
        self.implied_node_format_for_file_name(&self.binder.source_of_node(location).file_name)
    }

    fn implied_node_format_for_file_index(&self, file_index: usize) -> ModuleResolutionMode {
        self.implied_node_format_for_file_name(&self.binder.source(file_index).file_name)
    }

    fn implied_node_format_for_file_name(&self, file_name: &str) -> ModuleResolutionMode {
        if file_name.ends_with(".mts") || file_name.ends_with(".mjs") {
            return ModuleResolutionMode::EsNext;
        }
        if file_name.ends_with(".cts") || file_name.ends_with(".cjs") {
            return ModuleResolutionMode::CommonJs;
        }
        self.package_scope_node_format_for_file_name(file_name)
            .unwrap_or(ModuleResolutionMode::CommonJs)
    }

    fn package_scope_node_format_for_file_name(
        &self,
        file_name: &str,
    ) -> Option<ModuleResolutionMode> {
        let file_name = Self::normalize_program_path(file_name, "");
        let mut directory = file_name
            .rsplit_once('/')
            .map(|(directory, _)| directory)
            .unwrap_or("");
        loop {
            let package_json = if directory.is_empty() {
                "/package.json".to_owned()
            } else {
                format!("{directory}/package.json")
            };
            if let Some(&is_module) = self.host_package_json_module_types.get(&package_json) {
                return Some(if is_module {
                    ModuleResolutionMode::EsNext
                } else {
                    ModuleResolutionMode::CommonJs
                });
            }
            let Some((parent, _)) = directory.rsplit_once('/') else {
                break;
            };
            directory = parent;
        }
        None
    }

    fn static_resolution_mode_for_file(&self, location: NodeId) -> ModuleResolutionMode {
        if let Some(mode) = self.implied_resolution_mode_from_extension(location) {
            return mode;
        }
        let module_kind = self.options.emit_module_kind();
        if module_kind == 1 {
            ModuleResolutionMode::CommonJs
        } else if (100..=199).contains(&module_kind) {
            self.implied_node_format_for_file(location)
        } else if (5..=99).contains(&module_kind) || module_kind == 200 {
            ModuleResolutionMode::EsNext
        } else {
            ModuleResolutionMode::Unknown
        }
    }

    /// tsc-port: getEmitSyntaxForUsageLocationWorker @6.0.3
    /// tsc-hash: 4922201818d439fdf20dd97dfe55d99061c8844612f5d2b50b92ef4de1a92545
    /// tsc-span: _tsc.js:122290-122308
    ///
    /// Declaration face only: the caller's specifier is an
    /// Import/ExportDeclaration module specifier, so the worker's
    /// import-equals / require-call / import-call heads cannot be its
    /// parents. Composes getEmitModuleFormatOfFileWorker /
    /// getImpliedNodeFormatForEmitWorker: under the Node module kinds
    /// the file's implied format decides; outside them only decisive
    /// extension evidence keeps the implied format for emit — the
    /// explicit package `"type": "commonjs"` arm needs a tri-state
    /// package model the host does not carry, and omitting it only
    /// under-fires the CommonJS grammar row (FN, never FP).
    fn emit_syntax_for_declaration_specifier(
        &self,
        specifier: NodeId,
    ) -> Option<ModuleResolutionMode> {
        let module_kind = self.options.emit_module_kind();
        let implied_for_emit = if (100..=199).contains(&module_kind) {
            Some(self.implied_node_format_for_file(specifier))
        } else if self.import_syntax_affects_module_resolution() {
            self.implied_resolution_mode_from_extension(specifier)
        } else {
            // No implied node format is computed at all outside the
            // syntax-sensitive resolution modes.
            None
        };
        match implied_for_emit {
            Some(mode) => Some(mode),
            // fileEmitMode falls back to the emit module kind.
            None => {
                if module_kind == 1 {
                    Some(ModuleResolutionMode::CommonJs)
                } else if (5..=99).contains(&module_kind) || module_kind == 200 {
                    Some(ModuleResolutionMode::EsNext)
                } else {
                    None
                }
            }
        }
    }

    fn has_import_call_ancestor(&self, location: NodeId) -> bool {
        let mut current = Some(location);
        while let Some(node) = current {
            if self.is_import_call(node) {
                return true;
            }
            current = self.parent_of(node);
        }
        false
    }

    fn has_ancestor_kind(&self, location: NodeId, kind: SyntaxKind) -> bool {
        let mut current = Some(location);
        while let Some(node) = current {
            if self.kind_of(node) == kind {
                return true;
            }
            current = self.parent_of(node);
        }
        false
    }

    /// tsc suggestedExtensions (47456-47466) over the host set: the
    /// first actual extension whose file exists selects the import
    /// extension for the 2834 suggestion.
    fn suggested_extension_for(
        &self,
        location: NodeId,
        module_reference: &str,
    ) -> Option<&'static str> {
        let importing_index = self.binder.file_index_of_node(location);
        let importer =
            Self::normalize_program_path(&self.binder.source(importing_index).file_name, "");
        let importer_dir = match importer.rfind('/') {
            Some(position) => importer[..position].to_owned(),
            None => String::new(),
        };
        let candidate = Self::normalize_program_path(module_reference, &importer_dir);
        let jsx_preserve = self.options.jsx == Some(1);
        let table: [(&str, &'static str); 9] = [
            (".mts", ".mjs"),
            (".ts", ".js"),
            (".cts", ".cjs"),
            (".mjs", ".mjs"),
            (".js", ".js"),
            (".cjs", ".cjs"),
            (".tsx", if jsx_preserve { ".jsx" } else { ".js" }),
            (".jsx", ".jsx"),
            (".json", ".json"),
        ];
        table
            .iter()
            .find(|(actual, _)| {
                let probed = format!("{candidate}{actual}");
                self.host_file_paths.contains(&probed)
            })
            .map(|&(_, import_extension)| import_extension)
    }

    fn try_extract_ts_extension(name: &str) -> Option<&'static str> {
        // tsc supportedTSExtensionsForExtractExtension order: the
        // declaration extension wins over the plain one.
        [".d.ts", ".d.cts", ".d.mts", ".cts", ".mts", ".ts", ".tsx"]
            .into_iter()
            .find(|extension| name.ends_with(extension))
    }

    /// getSuggestedImportSource reduced: non-Node ESM emit at the
    /// modeled defaults strips the extension for CJS-ish kinds and
    /// maps .ts-family → .js under ES module kinds.
    fn suggested_import_source(
        &self,
        location: NodeId,
        module_reference: &str,
        ts_extension: &str,
    ) -> String {
        let without_extension = module_reference
            .strip_suffix(ts_extension)
            .unwrap_or(module_reference);
        let module_kind = self.options.emit_module_kind();
        if !(5..=99).contains(&module_kind)
            && self.resolution_mode_for_usage(location) != ModuleResolutionMode::EsNext
        {
            return without_extension.to_owned();
        }
        let prefer_ts_extension = Self::is_declaration_file_name(module_reference)
            && self.options.allow_importing_ts_extensions == Some(true);
        let runtime_extension = match ts_extension {
            ".mts" | ".d.mts" => {
                if prefer_ts_extension {
                    ".mts"
                } else {
                    ".mjs"
                }
            }
            ".cts" => {
                if prefer_ts_extension {
                    ".cts"
                } else {
                    ".cjs"
                }
            }
            _ => {
                if prefer_ts_extension {
                    ".ts"
                } else {
                    ".js"
                }
            }
        };
        format!("{without_extension}{runtime_extension}")
    }

    /// The type-only probe the ts-extension rows share
    /// (importOrExport.isTypeOnly || findAncestor(location,
    /// isImportTypeNode)).
    fn import_location_is_type_only(&self, location: NodeId) -> bool {
        let mut current = Some(location);
        while let Some(node) = current {
            match self.data_of(node) {
                NodeData::ImportDeclaration(data) => {
                    return data
                        .import_clause
                        .is_some_and(|clause| match self.data_of(clause) {
                            NodeData::ImportClause(data) => data.is_type_only,
                            _ => false,
                        });
                }
                NodeData::ImportEqualsDeclaration(data) => return data.is_type_only,
                NodeData::ExportDeclaration(data) => return data.is_type_only,
                NodeData::ImportType(_) => return true,
                _ => {}
            }
            current = self.parent_of(node);
        }
        false
    }

    /// tsc isSideEffectImport (the File_0_is_not_a_module /
    /// implicit-any suppressor): the enclosing import declaration has
    /// no import clause.
    fn is_side_effect_import(&self, node: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(ancestor) = current {
            if let NodeData::ImportDeclaration(data) = self.data_of(ancestor) {
                return data.import_clause.is_none();
            }
            current = self.parent_of(ancestor);
        }
        false
    }

    /// tsc findBestPatternMatch (1065) over patternAmbientModules:
    /// longest matching prefix wins.
    fn find_best_pattern_match(&self, candidate: &str) -> Option<SymbolId> {
        let mut matched: Option<(usize, SymbolId)> = None;
        for (prefix, suffix, symbol) in &self.pattern_ambient_modules {
            if candidate.len() >= prefix.len() + suffix.len()
                && candidate.starts_with(prefix.as_str())
                && candidate.ends_with(suffix.as_str())
            {
                match matched {
                    Some((best_len, _)) if prefix.len() <= best_len => {}
                    _ => matched = Some((prefix.len(), *symbol)),
                }
            }
        }
        matched.map(|(_, symbol)| symbol)
    }

    // ================================================================
    // Module symbol resolution + exports
    // ================================================================

    /// tsc-port: resolveExternalModuleSymbol @6.0.3
    /// tsc-hash: 9709dc0b3cc0d5fe0c9562a970e1f60f17e1c6dd8d492c8d18833b7a9f43bd3c
    /// tsc-span: _tsc.js:49683-49690
    pub(crate) fn resolve_external_module_symbol(
        &mut self,
        module_symbol: Option<SymbolId>,
        dont_resolve_alias: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let Some(module_symbol) = module_symbol else {
            return Ok(None);
        };
        // tsc guards on moduleSymbol?.exports — ours: every symbol has
        // an exports table (possibly empty); the guard is the Some.
        let export_equals = self
            .binder
            .symbol(module_symbol)
            .exports
            .get(InternalSymbolName::EXPORT_EQUALS)
            .copied();
        let export_equals = self.resolve_symbol_ex(export_equals, dont_resolve_alias)?;
        let merged_export = export_equals.map(|symbol| self.get_merged_symbol(symbol));
        let merged_module = self.get_merged_symbol(module_symbol);
        let exported = self.get_common_js_export_equals(merged_export, merged_module)?;
        Ok(Some(exported.unwrap_or(module_symbol)))
    }

    /// tsc-port: getCommonJsExportEquals @6.0.3
    /// tsc-hash: 396ba5dc8bf645b06b1a048e0ebabf2bac9dd3ae94b486d6e50cd2ccb0f72b5e
    /// tsc-span: _tsc.js:49691-49714
    fn get_common_js_export_equals(
        &mut self,
        exported: Option<SymbolId>,
        module_symbol: SymbolId,
    ) -> CheckResult2<Option<SymbolId>> {
        let Some(exported) = exported else {
            return Ok(None);
        };
        if exported == self.unknown_symbol
            || exported == module_symbol
            || self.binder.symbol(module_symbol).exports.len() == 1
            || self
                .binder
                .symbol(exported)
                .flags
                .intersects(SymbolFlags::ALIAS)
        {
            return Ok(Some(exported));
        }
        if let Some(merged) = self.links.symbol(exported).cjs_export_merged {
            return Ok(Some(merged));
        }
        let merged = if self
            .binder
            .symbol(exported)
            .flags
            .intersects(SymbolFlags::TRANSIENT)
        {
            exported
        } else {
            self.clone_symbol(exported)
        };
        {
            let symbol = self.binder.symbol_mut(merged);
            symbol.flags |= SymbolFlags::VALUE_MODULE;
        }
        let module_exports: Vec<(String, SymbolId)> = self
            .binder
            .symbol(module_symbol)
            .exports
            .iter()
            .map(|(name, &symbol)| (name.clone(), symbol))
            .collect();
        for (name, source_symbol) in module_exports {
            if name == InternalSymbolName::EXPORT_EQUALS {
                continue;
            }
            let existing = self.binder.symbol(merged).exports.get(&name).copied();
            let value = match existing {
                Some(existing) => self.merge_symbol(existing, source_symbol, false),
                None => source_symbol,
            };
            self.binder.symbol_mut(merged).exports.insert(name, value);
        }
        if merged == exported {
            // tsc resets the memoized member/export resolutions after
            // mutating the transient in place.
            self.links.revert_symbol_resolved_exports(merged);
            self.links.revert_symbol_resolved_members(merged);
        }
        self.links
            .set_symbol_cjs_export_merged(self.speculation_depth, merged, merged);
        self.links
            .set_symbol_cjs_export_merged(self.speculation_depth, exported, merged);
        Ok(Some(merged))
    }

    /// tsc-port: resolveESModuleSymbol @6.0.3
    /// tsc-hash: f20024ad0bb1ee9307d7ca335709632fd30257dc4e437c62da4ddc46f27b1910
    /// tsc-span: _tsc.js:49715-49760
    pub(crate) fn resolve_es_module_symbol(
        &mut self,
        module_symbol: Option<SymbolId>,
        referencing_location: NodeId,
        dont_resolve_alias: bool,
        suppress_interop_error: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        let symbol = self.resolve_external_module_symbol(module_symbol, dont_resolve_alias)?;
        let Some(symbol) = symbol else {
            return Ok(None);
        };
        if dont_resolve_alias {
            return Ok(Some(symbol));
        }
        let symbol_flags = self.binder.symbol(symbol).flags;
        let has_source_file_declaration = self
            .binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| self.kind_of(declaration) == SyntaxKind::SourceFile);
        if !suppress_interop_error
            && !symbol_flags.intersects(SymbolFlags::MODULE | SymbolFlags::VARIABLE)
            && !has_source_file_declaration
        {
            let compiler_option_name = if self.options.emit_module_kind() >= 5 {
                "allowSyntheticDefaultImports"
            } else {
                "esModuleInterop"
            };
            self.error_at(
                Some(referencing_location),
                &diagnostics::This_module_can_only_be_referenced_with_ECMAScript_imports_exports_by_turning_on_the_0_flag_and_referencing_its_default_export,
                &[compiler_option_name],
            );
            return Ok(Some(symbol));
        }
        if let Some(reference_parent) = self.parent_of(referencing_location) {
            let namespace_import =
                if self.kind_of(reference_parent) == SyntaxKind::ImportDeclaration {
                    self.get_namespace_declaration_node(reference_parent)
                } else {
                    None
                };
            if namespace_import.is_some() || self.is_import_call(reference_parent) {
                let reference = referencing_location;
                let ty = self.get_type_of_symbol(symbol)?;
                let original_symbol =
                    module_symbol.expect("a resolved symbol implies a module symbol");
                if let Some(default_only_type) = self.get_type_with_synthetic_default_only(
                    ty,
                    symbol,
                    original_symbol,
                    reference,
                )? {
                    return Ok(Some(self.clone_type_as_module_type(
                        symbol,
                        default_only_type,
                        reference_parent,
                    )?));
                }
                let target_file = self.source_file_index_of_symbol(original_symbol);
                let usage_mode = self.resolution_mode_for_usage(reference);
                if let (Some(namespace_import), Some(target_file)) = (namespace_import, target_file)
                {
                    if (102..=199).contains(&self.options.emit_module_kind())
                        && usage_mode == ModuleResolutionMode::CommonJs
                        && self.implied_node_format_for_file_index(target_file)
                            == ModuleResolutionMode::EsNext
                    {
                        if let Some(module_exports) = self.resolve_export_by_name(
                            symbol,
                            "module.exports",
                            Some(namespace_import),
                            dont_resolve_alias,
                        )? {
                            if !suppress_interop_error
                                && !symbol_flags
                                    .intersects(SymbolFlags::MODULE | SymbolFlags::VARIABLE)
                            {
                                self.error_at(
                                    Some(referencing_location),
                                    &diagnostics::This_module_can_only_be_referenced_with_ECMAScript_imports_exports_by_turning_on_the_0_flag_and_referencing_its_default_export,
                                    &["esModuleInterop"],
                                );
                            }
                            if self.options.es_module_interop_effective()
                                && self.has_interop_signatures(ty)?
                            {
                                return Ok(Some(self.clone_type_as_module_type(
                                    module_exports,
                                    ty,
                                    reference_parent,
                                )?));
                            }
                            return Ok(Some(module_exports));
                        }
                    }
                }
                let is_esm_cjs_ref = target_file.is_some_and(|target_file| {
                    usage_mode == ModuleResolutionMode::EsNext
                        && self.implied_node_format_for_file_index(target_file)
                            == ModuleResolutionMode::CommonJs
                });
                if self.options.es_module_interop_effective() || is_esm_cjs_ref {
                    let has_default_property = self
                        .get_property_of_type_ex(
                            ty,
                            InternalSymbolName::DEFAULT,
                            /*skip_object_function_property_augment*/ true,
                        )?
                        .is_some();
                    if self.has_interop_signatures(ty)? || has_default_property || is_esm_cjs_ref {
                        let module_type = if self
                            .tables
                            .flags_of(ty)
                            .intersects(TypeFlags::STRUCTURED_TYPE)
                        {
                            self.get_type_with_synthetic_default_import_type(
                                ty,
                                symbol,
                                original_symbol,
                                reference,
                            )?
                        } else {
                            let parent = self.binder.symbol(symbol).parent;
                            self.create_default_property_wrapper_for_module(
                                symbol, parent, /*anonymous_symbol*/ None,
                            )?
                        };
                        return Ok(Some(self.clone_type_as_module_type(
                            symbol,
                            module_type,
                            reference_parent,
                        )?));
                    }
                }
            }
        }
        Ok(Some(symbol))
    }

    /// tsc-port: hasSignatures @6.0.3
    /// tsc-hash: 2085cf5841568be535290c3072cfba462715c318e4c09e1ccb6d9082cdc00f15
    /// tsc-span: _tsc.js:49761-49763
    ///
    /// getSignaturesOfStructuredType for both kinds (no apparent-type
    /// hop — the interop probes hand it resolved module types).
    fn has_interop_signatures(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::STRUCTURED_TYPE)
        {
            return Ok(false);
        }
        let members = self.resolve_structured_type_members(ty)?;
        let resolved = self.members_of(members);
        Ok(!resolved.call_signatures.is_empty() || !resolved.construct_signatures.is_empty())
    }

    /// tsc-port: createDefaultPropertyWrapperForModule @6.0.3
    /// tsc-hash: 35e7604c38f11ec8025e4a3363b87b2adad01a4cd441a4a5481947d351e7e0d8
    /// tsc-span: _tsc.js:77768-77776
    fn create_default_property_wrapper_for_module(
        &mut self,
        symbol: SymbolId,
        original_symbol: Option<SymbolId>,
        anonymous_symbol: Option<SymbolId>,
    ) -> CheckResult2<TypeId> {
        let new_symbol = self
            .binder
            .create_symbol(SymbolFlags::ALIAS, InternalSymbolName::DEFAULT.to_owned());
        self.binder.symbol_mut(new_symbol).parent = original_symbol;
        let name_type = self
            .tables
            .get_string_literal_type(InternalSymbolName::DEFAULT);
        self.links
            .set_symbol_name_type(self.speculation_depth, new_symbol, Some(name_type));
        // newSymbol.links.aliasTarget = resolveSymbol(symbol): the
        // pre-resolved slot short-circuits future resolveAlias hops
        // (the transient alias has no alias declaration to walk).
        let alias_target = self
            .resolve_symbol_ex(Some(symbol), false)?
            .expect("resolveSymbol of Some is Some");
        self.links
            .set_fresh_symbol_alias_target(new_symbol, LinkSlot::Resolved(alias_target));
        let mut member_table = SymbolTable::default();
        member_table.insert(InternalSymbolName::DEFAULT.to_owned(), new_symbol);
        // createAnonymousType's getNamedMembers over the one-entry
        // table: "default" is not reserved, and symbolIsValue's alias
        // branch reads the pre-resolved target's flags (getSymbolFlags)
        // — inlined here because the shared get_named_members models
        // only the own-VALUE face.
        let properties = if self
            .get_symbol_flags_of(new_symbol)?
            .intersects(SymbolFlags::VALUE)
        {
            vec![new_symbol]
        } else {
            Vec::new()
        };
        Ok(self.make_resolved_anonymous_type(
            anonymous_symbol,
            member_table,
            properties,
            Vec::new(),
            ObjectFlags::ANONYMOUS,
        ))
    }

    /// tsc-port: getTypeWithSyntheticDefaultOnly @6.0.3
    /// tsc-hash: f0fe34a3f1bed618e0e685ea82902848b5175f760629d7588298b6546e06fbf1
    /// tsc-span: _tsc.js:77778-77788
    pub(crate) fn get_type_with_synthetic_default_only(
        &mut self,
        ty: TypeId,
        symbol: SymbolId,
        original_symbol: SymbolId,
        module_specifier: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        if !self.is_only_importable_as_default(module_specifier, Some(original_symbol))?
            || self.tables.is_error_type(ty)
        {
            return Ok(None);
        }
        if let Some(memo) = self.links.ty(ty).default_only_type {
            return Ok(Some(memo));
        }
        let default_only =
            self.create_default_property_wrapper_for_module(symbol, Some(original_symbol), None)?;
        self.links
            .set_type_default_only_type(self.speculation_depth, ty, default_only);
        Ok(Some(default_only))
    }

    /// tsc-port: getTypeWithSyntheticDefaultImportType @6.0.3
    /// tsc-hash: 15aa12c477c71d19f9afd99525fd494140635a859a647923799ae5d335b8d913
    /// tsc-span: _tsc.js:77789-77822
    pub(crate) fn get_type_with_synthetic_default_import_type(
        &mut self,
        ty: TypeId,
        symbol: SymbolId,
        original_symbol: SymbolId,
        module_specifier: NodeId,
    ) -> CheckResult2<TypeId> {
        if !self.options.allow_synthetic_default_imports_effective()
            || self.tables.is_error_type(ty)
        {
            return Ok(ty);
        }
        if let Some(memo) = self.links.ty(ty).synthetic_type {
            return Ok(memo);
        }
        let file_index = self.source_file_index_of_symbol(original_symbol);
        let has_synthetic_default = self.can_have_synthetic_default(
            file_index,
            original_symbol,
            /*dont_resolve_alias*/ false,
            Some(module_specifier),
        )?;
        let synthetic = if has_synthetic_default {
            let anonymous_symbol = self.binder.create_symbol(
                SymbolFlags::TYPE_LITERAL,
                InternalSymbolName::TYPE.to_owned(),
            );
            let default_containing_object = self.create_default_property_wrapper_for_module(
                symbol,
                Some(original_symbol),
                Some(anonymous_symbol),
            )?;
            self.links.set_fresh_symbol_type(
                anonymous_symbol,
                LinkSlot::Resolved(default_containing_object),
            );
            if self.is_valid_spread_type(ty)? {
                self.get_spread_type(
                    ty,
                    default_containing_object,
                    Some(anonymous_symbol),
                    ObjectFlags::NONE,
                    /*readonly*/ false,
                )?
            } else {
                default_containing_object
            }
        } else {
            ty
        };
        self.links
            .set_type_synthetic_type(self.speculation_depth, ty, synthetic);
        Ok(synthetic)
    }

    /// tsc-port: cloneTypeAsModuleType @6.0.3
    /// tsc-hash: 4556e023b03e0b161550d949218a06b9c5ae7efc98f6c9a704f3c5a3c01b4ab6
    /// tsc-span: _tsc.js:49764-49777
    ///
    /// The cloneSymbol copy set + links.target/originatingImport, with
    /// the clone's type rebuilt over the module type's resolved members
    /// and NO signatures — that drop is the observable (2349 + the
    /// namespace-import related-info band).
    fn clone_type_as_module_type(
        &mut self,
        symbol: SymbolId,
        module_type: TypeId,
        reference_parent: NodeId,
    ) -> CheckResult2<SymbolId> {
        let original = self.binder.symbol(symbol);
        let flags = original.flags;
        let escaped_name = original.escaped_name.clone();
        let declarations = original.declarations.clone();
        let parent = original.parent;
        let value_declaration = original.value_declaration;
        let const_enum_only_module = original.const_enum_only_module;
        let members = original.members.clone();
        let exports = original.exports.clone();
        let result = self.binder.create_symbol(flags, escaped_name);
        let cloned = self.binder.symbol_mut(result);
        cloned.declarations = declarations;
        cloned.parent = parent;
        cloned.value_declaration = value_declaration;
        if const_enum_only_module == Some(true) {
            cloned.const_enum_only_module = Some(true);
        }
        cloned.members = members;
        cloned.exports = exports;
        self.links
            .set_symbol_target(self.speculation_depth, result, symbol);
        self.links
            .set_symbol_originating_import(self.speculation_depth, result, reference_parent);
        let members_id = self.resolve_structured_type_members(module_type)?;
        let resolved = self.members_of(members_id);
        let member_table = resolved.members.clone();
        let index_infos = resolved.index_infos.clone();
        let properties = self.get_named_members(&member_table)?;
        let anonymous = self.make_resolved_anonymous_type(
            Some(result),
            member_table,
            properties,
            index_infos,
            ObjectFlags::ANONYMOUS,
        );
        self.links
            .set_fresh_symbol_type(result, LinkSlot::Resolved(anonymous));
        Ok(result)
    }

    /// tsc getNamespaceDeclarationNode: the NamespaceImport of an
    /// import declaration's clause (import-equals handled at its own
    /// arm).
    fn get_namespace_declaration_node(&self, node: NodeId) -> Option<NodeId> {
        let clause = match self.data_of(node) {
            NodeData::ImportDeclaration(data) => data.import_clause?,
            _ => return None,
        };
        let named_bindings = match self.data_of(clause) {
            NodeData::ImportClause(data) => data.named_bindings?,
            _ => return None,
        };
        if self.kind_of(named_bindings) == SyntaxKind::NamespaceImport {
            Some(named_bindings)
        } else {
            None
        }
    }

    /// tsc-port: hasExportAssignmentSymbol @6.0.3
    /// tsc-hash: 3d1e04328f162417862d224672d4717233138c504e22a4e342e4e3fe2e6f3ce7
    /// tsc-span: _tsc.js:49778-49780
    pub(crate) fn has_export_assignment_symbol(&self, module_symbol: SymbolId) -> bool {
        self.binder
            .symbol(module_symbol)
            .exports
            .contains_key(InternalSymbolName::EXPORT_EQUALS)
    }

    /// tsc-port: isShorthandAmbientModuleSymbol @6.0.3
    /// tsc-hash: 145568251fbab021f53f6cec68de6c9548409ec13d04160d2562739c46c89317
    /// tsc-span: _tsc.js:13725-13730
    pub(crate) fn is_shorthand_ambient_module_symbol(&self, module_symbol: SymbolId) -> bool {
        let Some(value_declaration) = self.binder.symbol(module_symbol).value_declaration else {
            return false;
        };
        self.kind_of(value_declaration) == SyntaxKind::ModuleDeclaration
            && match self.data_of(value_declaration) {
                NodeData::ModuleDeclaration(data) => data.body.is_none(),
                _ => false,
            }
    }

    /// tsc-port: getExportsOfModule @6.0.3
    /// tsc-hash: c8828ee8f46fd85b914f1cf8c1d478739e696951f22ebbcc49592a56e17a2682
    /// tsc-span: _tsc.js:49837-49845
    pub(crate) fn get_exports_of_module(
        &mut self,
        module_symbol: SymbolId,
    ) -> CheckResult2<SymbolTable> {
        if let LinkSlot::Resolved(exports) = self.links.symbol(module_symbol).resolved_exports {
            return Ok(exports);
        }
        let (exports, type_only_export_star_map) =
            self.get_exports_of_module_worker(module_symbol)?;
        self.links.set_symbol_module_exports(
            self.speculation_depth,
            module_symbol,
            exports.clone(),
            type_only_export_star_map,
        );
        Ok(exports)
    }

    /// tsc-port: getExportsOfModuleWorker @6.0.3
    /// tsc-hash: 6646b6aa219ba46b93920a4edbcda42d39415260d756ebbcf05e8dff99229fbc
    /// tsc-span: _tsc.js:49868-49931
    fn get_exports_of_module_worker(
        &mut self,
        module_symbol: SymbolId,
    ) -> CheckResult2<(
        SymbolTable,
        Option<std::collections::HashMap<String, NodeId>>,
    )> {
        let mut visited: Vec<SymbolId> = Vec::new();
        let mut type_only_export_star_map: Option<std::collections::HashMap<String, NodeId>> = None;
        let mut non_type_only_names: indexmap::IndexSet<String> = indexmap::IndexSet::new();
        let module_symbol = self
            .resolve_external_module_symbol(Some(module_symbol), false)?
            .expect("resolveExternalModuleSymbol(Some) is Some");
        let exports = self
            .visit_module_exports(
                Some(module_symbol),
                None,
                false,
                &mut visited,
                &mut non_type_only_names,
                &mut type_only_export_star_map,
            )?
            .unwrap_or_default();
        if let Some(map) = &mut type_only_export_star_map {
            for name in &non_type_only_names {
                map.remove(name);
            }
        }
        Ok((exports, type_only_export_star_map))
    }

    /// The worker's recursive `visit` closure. `export_star` is the
    /// ExportDeclaration the caller came through (None at the root).
    fn visit_module_exports(
        &mut self,
        symbol: Option<SymbolId>,
        export_star: Option<NodeId>,
        is_type_only: bool,
        visited: &mut Vec<SymbolId>,
        non_type_only_names: &mut indexmap::IndexSet<String>,
        type_only_export_star_map: &mut Option<std::collections::HashMap<String, NodeId>>,
    ) -> CheckResult2<Option<SymbolTable>> {
        if !is_type_only {
            if let Some(symbol) = symbol {
                for name in self.binder.symbol(symbol).exports.keys() {
                    non_type_only_names.insert(name.clone());
                }
            }
        }
        let Some(symbol) = symbol else {
            return Ok(None);
        };
        if visited.contains(&symbol) {
            return Ok(None);
        }
        visited.push(symbol);
        let mut symbols = self.binder.symbol(symbol).exports.clone();
        let export_stars = symbols.get(InternalSymbolName::EXPORT_STAR).copied();
        if let Some(export_stars) = export_stars {
            let mut nested_symbols = SymbolTable::default();
            // (specifier text, exportsWithDuplicate) per name —
            // IndexMap so the 2308 emission order matches tsc's Map
            // iteration.
            let mut lookup_table: ExportLookupTable = ExportLookupTable::new();
            for node in self.binder.symbol(export_stars).declarations.clone() {
                let specifier = match self.data_of(node) {
                    NodeData::ExportDeclaration(data) => data.module_specifier,
                    _ => None,
                };
                let node_is_type_only = match self.data_of(node) {
                    NodeData::ExportDeclaration(data) => data.is_type_only,
                    _ => false,
                };
                let Some(specifier) = specifier else {
                    continue;
                };
                let resolved_module = self.resolve_external_module_name(node, specifier, false)?;
                let exported_symbols = self.visit_module_exports(
                    resolved_module,
                    Some(node),
                    is_type_only || node_is_type_only,
                    visited,
                    non_type_only_names,
                    type_only_export_star_map,
                )?;
                let specifier_text = self.text_of_node(specifier)?;
                self.extend_export_symbols(
                    &mut nested_symbols,
                    exported_symbols.as_ref(),
                    Some((&mut lookup_table, node, &specifier_text)),
                )?;
            }
            for (id, (specifier_text, exports_with_duplicate)) in &lookup_table {
                if id == "export=" || exports_with_duplicate.is_empty() || symbols.contains_key(id)
                {
                    continue;
                }
                for &node in exports_with_duplicate {
                    self.error_at(
                        Some(node),
                        &diagnostics::Module_0_has_already_exported_a_member_named_1_Consider_explicitly_re_exporting_to_resolve_the_ambiguity,
                        &[specifier_text, unescape_leading_underscores(id)],
                    );
                }
            }
            self.extend_export_symbols(&mut symbols, Some(&nested_symbols), None)?;
        }
        if let Some(export_star) = export_star {
            let export_star_is_type_only = match self.data_of(export_star) {
                NodeData::ExportDeclaration(data) => data.is_type_only,
                _ => false,
            };
            if export_star_is_type_only {
                let map = type_only_export_star_map.get_or_insert_with(Default::default);
                for name in symbols.keys() {
                    map.insert(name.clone(), export_star);
                }
            }
        }
        Ok(Some(symbols))
    }

    /// tsc-port: extendExportSymbols @6.0.3
    /// tsc-hash: da484ffd3b8fc37b2fd91430d2b08e4c0e9fd6c1b197022f2899e9cca8b3cb7f
    /// tsc-span: _tsc.js:49846-49867
    fn extend_export_symbols(
        &mut self,
        target: &mut SymbolTable,
        source: Option<&SymbolTable>,
        mut lookup: Option<(&mut ExportLookupTable, NodeId, &str)>,
    ) -> CheckResult2<()> {
        let Some(source) = source else {
            return Ok(());
        };
        for (id, &source_symbol) in source {
            if id == InternalSymbolName::DEFAULT {
                continue;
            }
            match target.get(id).copied() {
                None => {
                    target.insert(id.clone(), source_symbol);
                    if let Some((lookup_table, _, specifier_text)) = &mut lookup {
                        lookup_table
                            .entry(id.clone())
                            .or_insert_with(|| ((*specifier_text).to_owned(), Vec::new()));
                    }
                }
                Some(target_symbol) => {
                    if let Some((lookup_table, export_node, _)) = &mut lookup {
                        let resolved_target = self.resolve_symbol_ex(Some(target_symbol), false)?;
                        let resolved_source = self.resolve_symbol_ex(Some(source_symbol), false)?;
                        if resolved_target != resolved_source {
                            if let Some((_, duplicates)) = lookup_table.get_mut(id) {
                                duplicates.push(*export_node);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // ================================================================
    // Shared small helpers
    // ================================================================

    /// tsc-port: isTypeOnlyImportDeclaration @6.0.3
    /// tsc-hash: 4f5b5c8c661652505d6ced2e769976213b3d9d1d065e634d899ef9978f6c43c1
    /// tsc-span: _tsc.js:11899-11925
    ///
    /// Folded with isTypeOnlyExportDeclaration into the OR the checker
    /// consumes (isTypeOnlyImportOrExportDeclaration). ImportClause
    /// is_type_only mirrors tsc's derivation: true exactly for the
    /// TypeKeyword phase, so `import defer` (DeferKeyword phase) stays
    /// a value import here.
    pub(crate) fn is_type_only_import_or_export_declaration(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::ImportSpecifier(data) => {
                data.is_type_only
                    || self
                        .parent_of(node)
                        .and_then(|named| self.parent_of(named))
                        .is_some_and(|clause| match self.data_of(clause) {
                            NodeData::ImportClause(data) => data.is_type_only,
                            _ => false,
                        })
            }
            NodeData::ExportSpecifier(data) => {
                data.is_type_only
                    || self
                        .parent_of(node)
                        .and_then(|named| self.parent_of(named))
                        .is_some_and(|declaration| match self.data_of(declaration) {
                            NodeData::ExportDeclaration(data) => data.is_type_only,
                            _ => false,
                        })
            }
            NodeData::NamespaceImport(_) => {
                self.parent_of(node)
                    .is_some_and(|clause| match self.data_of(clause) {
                        NodeData::ImportClause(data) => data.is_type_only,
                        _ => false,
                    })
            }
            NodeData::NamespaceExport(_) => {
                self.parent_of(node)
                    .is_some_and(|declaration| match self.data_of(declaration) {
                        NodeData::ExportDeclaration(data) => data.is_type_only,
                        _ => false,
                    })
            }
            NodeData::ImportClause(data) => data.is_type_only,
            NodeData::ImportEqualsDeclaration(data) => data.is_type_only,
            NodeData::ExportDeclaration(data) => {
                data.is_type_only && data.module_specifier.is_some() && data.export_clause.is_none()
            }
            _ => false,
        }
    }

    /// tsc-port: moduleExportNameTextEscaped @6.0.3
    /// tsc-hash: a5d471e067b6750272e8550bdd2185a829aa9267b305cba573c0064f77634dfe
    /// tsc-span: _tsc.js:13026-13034
    ///
    /// Folded family: TextUnescaped/TextEscaped/IsDefault.
    pub(crate) fn module_export_name_text_escaped(&self, name: NodeId) -> String {
        match self.data_of(name) {
            NodeData::StringLiteral(data) => escape_leading_underscores(&data.text),
            NodeData::Identifier(data) => data.escaped_text.clone(),
            _ => String::new(),
        }
    }

    /// tsc-port: moduleExportNameTextUnescaped @6.0.3
    /// tsc-hash: 6ca98eb0675711e5d04bc7ab9c263795d5a4e08c689ed2361947370c00473ddc
    /// tsc-span: _tsc.js:13026-13028
    pub(crate) fn module_export_name_text_unescaped(&self, name: NodeId) -> String {
        match self.data_of(name) {
            NodeData::StringLiteral(data) => data.text.clone(),
            NodeData::Identifier(data) => {
                unescape_leading_underscores(&data.escaped_text).to_owned()
            }
            _ => String::new(),
        }
    }

    /// tsc-port: moduleExportNameIsDefault @6.0.3
    /// tsc-hash: a5be65407a7f18b94596f3fec078ba710f3c23f8594057edb6e1550afa87cdf0
    /// tsc-span: _tsc.js:13032-13034
    pub(crate) fn module_export_name_is_default(&self, name: NodeId) -> bool {
        match self.data_of(name) {
            NodeData::StringLiteral(data) => data.text == InternalSymbolName::DEFAULT,
            NodeData::Identifier(data) => data.escaped_text == InternalSymbolName::DEFAULT,
            _ => false,
        }
    }

    /// tsrs-native: the SourceFile declaration index of a module
    /// symbol (`moduleSymbol.declarations?.find(isSourceFile)` as a
    /// program file index).
    pub(crate) fn source_file_index_of_symbol(&self, symbol: SymbolId) -> Option<usize> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .find(|&&declaration| self.kind_of(declaration) == SyntaxKind::SourceFile)
            .map(|&declaration| self.binder.file_index_of_node(declaration))
    }

    /// The import-binding NAME the 2594-family rows point at
    /// (node.name of an ImportClause/ImportSpecifier/ExportSpecifier).
    fn name_of_import_binding(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::ImportClause(data) => data.name,
            NodeData::ImportSpecifier(data) => data.name,
            NodeData::ExportSpecifier(data) => data.name,
            _ => None,
        }
    }

    /// isRightSideOfQualifiedNameOrPropertyAccess for the
    /// import-equals RHS walk.
    fn is_right_side_of_qualified_name_or_property_access(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::QualifiedName(data) => data.right == Some(node),
            NodeData::PropertyAccessExpression(data) => data.name == Some(node),
            _ => false,
        }
    }

    /// EntityName kind probe (Identifier | QualifiedName).
    fn is_entity_name_kind(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::Identifier | SyntaxKind::QualifiedName
        )
    }

    // ================================================================
    // §8 module band + §9 checker drivers
    // ================================================================

    /// tsc-port: checkModuleDeclaration @6.0.3
    /// tsc-hash: fd08e6535aca4619488e996c96744781fbed038c3af10dcd3cc1da26b11b8ec1
    /// tsc-span: _tsc.js:85840-85924
    ///
    /// addLazyDiagnostic = eager identity (5.4). Dead rows at the
    /// modeled defaults: erasableSyntaxOnly / isolatedModules /
    /// verbatimModuleSyntax (options unmodeled). The M7
    /// registerForUnusedIdentifiersCheck registration is inert.
    pub(crate) fn check_module_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        let (name, body) = match self.data_of(node) {
            NodeData::ModuleDeclaration(data) => (data.name, data.body),
            _ => return Ok(()),
        };
        if let Some(body) = body {
            self.check_source_element(Some(body));
        }
        let source = self.binder.source_of_node(node);
        let is_global_augmentation = node_util::is_global_scope_augmentation(source, node);
        let in_ambient_context = self
            .binder
            .flags_of(node)
            .intersects(tsrs2_types::NodeFlags::AMBIENT);
        if is_global_augmentation && !in_ambient_context {
            self.error_at(
                name.or(Some(node)),
                &diagnostics::Augmentations_for_the_global_scope_should_have_declare_modifier_unless_they_appear_in_already_ambient_context,
                &[],
            );
        }
        let is_ambient_external_module = node_util::is_ambient_module(source, node);
        let context_error_message = if is_ambient_external_module {
            &diagnostics::An_ambient_module_declaration_is_only_allowed_at_the_top_level_in_a_file
        } else {
            &diagnostics::A_namespace_declaration_is_only_allowed_at_the_top_level_of_a_namespace_or_module
        };
        if self.check_grammar_module_element_context(node, context_error_message) {
            return Ok(());
        }
        let Some(name) = name else {
            return Ok(());
        };
        if !self.check_grammar_modifiers(node) {
            // The M7-stub interplay (m4-58 §0): a REAL modifier error
            // suppresses this follower in tsc — contain when modifiers
            // are present.
            let has_modifiers = match self.data_of(node) {
                NodeData::ModuleDeclaration(data) => data.modifiers.is_some(),
                _ => false,
            };
            if !has_modifiers
                && !in_ambient_context
                && self.kind_of(name) == SyntaxKind::StringLiteral
            {
                self.grammar_error_on_node(
                    name,
                    &diagnostics::Only_ambient_modules_can_use_quoted_names,
                    &[],
                );
            }
        }
        if self.kind_of(name) == SyntaxKind::Identifier {
            self.check_collisions_for_declaration_name(node, Some(name));
            if !self.binder.flags_of(node).intersects(
                tsrs2_types::NodeFlags::NAMESPACE | tsrs2_types::NodeFlags::GLOBAL_AUGMENTATION,
            ) {
                self.error_at(
                    Some(name),
                    &diagnostics::A_namespace_declaration_should_not_be_declared_using_the_module_keyword_Please_use_the_namespace_keyword_instead,
                    &[],
                );
            }
        }
        self.check_exports_on_merged_declarations(node)?;
        let symbol = self.get_symbol_of_declaration(node)?;
        let symbol_flags = self.binder.symbol(symbol).flags;
        if symbol_flags.intersects(SymbolFlags::VALUE_MODULE)
            && !in_ambient_context
            && self.is_instantiated_module(node)
        {
            // erasableSyntaxOnly / isolatedModules global-script rows:
            // options unmodeled — dead.
            if self.binder.symbol(symbol).declarations.len() > 1 {
                let first_non_ambient =
                    self.get_first_non_ambient_class_or_function_declaration(symbol);
                if let Some(first_non_ambient) = first_non_ambient {
                    let node_file = self.binder.file_index_of_node(node);
                    let other_file = self.binder.file_index_of_node(first_non_ambient);
                    if node_file != other_file {
                        self.error_at(
                            Some(name),
                            &diagnostics::A_namespace_declaration_cannot_be_in_a_different_file_from_a_class_or_function_with_which_it_is_merged,
                            &[],
                        );
                    } else if self.binder.source_of_node(node).arena.node(node).pos
                        < self
                            .binder
                            .source_of_node(first_non_ambient)
                            .arena
                            .node(first_non_ambient)
                            .pos
                    {
                        self.error_at(
                            Some(name),
                            &diagnostics::A_namespace_declaration_cannot_be_located_prior_to_a_class_or_function_with_which_it_is_merged,
                            &[],
                        );
                    }
                }
                let merged_class = self
                    .binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .copied()
                    .find(|&declaration| self.kind_of(declaration) == SyntaxKind::ClassDeclaration);
                if let Some(merged_class) = merged_class {
                    if self.in_same_lexical_scope(node, merged_class) {
                        self.links.or_node_check_flags(
                            self.speculation_depth,
                            node,
                            tsrs2_types::NodeCheckFlags::LEXICAL_MODULE_MERGES_WITH_CLASS,
                        );
                    }
                }
            }
            // verbatimModuleSyntax CJS export-modifier row: dead.
        }
        if is_ambient_external_module {
            let source = self.binder.source_of_node(node);
            if node_util::is_module_augmentation_external(source, node) {
                // External-module AUGMENTATION: check the body per
                // element when global-augment or Transient (merged)
                // module symbol.
                let declaration_symbol = self.get_symbol_of_declaration(node)?;
                let check_body = is_global_augmentation
                    || self
                        .binder
                        .symbol(declaration_symbol)
                        .flags
                        .intersects(SymbolFlags::TRANSIENT);
                if check_body {
                    if let Some(body) = body {
                        if let Some(statements) =
                            node_util::statements_of(self.binder.source_of_node(node), body)
                        {
                            for statement in self.nodes_of(Some(statements)) {
                                self.check_module_augmentation_element(
                                    statement,
                                    is_global_augmentation,
                                )?;
                            }
                        }
                    }
                }
            } else if self
                .parent_of(node)
                .is_some_and(|parent| self.is_global_source_file_node(parent))
            {
                if is_global_augmentation {
                    self.error_at(
                        Some(name),
                        &diagnostics::Augmentations_for_the_global_scope_can_only_be_directly_nested_in_external_modules_or_ambient_module_declarations,
                        &[],
                    );
                } else {
                    let source = self.binder.source_of_node(node);
                    let text = node_util::get_text_of_identifier_or_literal(source, name)
                        .unwrap_or_default();
                    if Self::is_external_module_name_relative(&text) {
                        self.error_at(
                            Some(name),
                            &diagnostics::Ambient_module_declaration_cannot_specify_relative_module_name,
                            &[],
                        );
                    }
                }
            } else if is_global_augmentation {
                self.error_at(
                    Some(name),
                    &diagnostics::Augmentations_for_the_global_scope_can_only_be_directly_nested_in_external_modules_or_ambient_module_declarations,
                    &[],
                );
            } else {
                self.error_at(
                    Some(name),
                    &diagnostics::Ambient_modules_cannot_be_nested_in_other_modules_or_namespaces,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: isInstantiatedModule @6.0.3
    /// tsc-hash: 9238b53892ec74fb5a88a76ea8c1c61a87d17fe8dbca44178b26c79e4b74a670
    /// tsc-span: _tsc.js:46434-46437
    ///
    /// The preserveConstEnums parameter bakes in (both callers pass
    /// the computed option). (pub(crate): isSourceElementUnreachable's
    /// ModuleDeclaration arm consumes it since 6.6b.)
    pub(crate) fn is_instantiated_module(&self, node: NodeId) -> bool {
        let state = self.module_instance_state_of(node);
        state == tsrs2_binder::containers::ModuleInstanceState::Instantiated
            || (self.options.should_preserve_const_enums()
                && state == tsrs2_binder::containers::ModuleInstanceState::ConstEnumOnly)
    }

    /// tsc-port: getFirstNonAmbientClassOrFunctionDeclaration @6.0.3
    /// tsc-hash: 39cf2e0ded5e6df45da9c09ed2f6d4b2908981478850d4d6c7a0c90be9a09e80
    /// tsc-span: _tsc.js:85818-85828
    fn get_first_non_ambient_class_or_function_declaration(
        &self,
        symbol: SymbolId,
    ) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                let kind = self.kind_of(declaration);
                let bodied_function = kind == SyntaxKind::FunctionDeclaration
                    && node_util::body_of(self.binder.source_of_node(declaration), declaration)
                        .is_some();
                (kind == SyntaxKind::ClassDeclaration || bodied_function)
                    && !self
                        .binder
                        .flags_of(declaration)
                        .intersects(tsrs2_types::NodeFlags::AMBIENT)
            })
    }

    /// tsc-port: inSameLexicalScope @6.0.3
    /// tsc-hash: 2f9fc18eaf66b3da8e5e44aa670fd518998ed44a8fc6e96188080982506bd8e5
    /// tsc-span: _tsc.js:85829-85839
    fn in_same_lexical_scope(&self, node1: NodeId, node2: NodeId) -> bool {
        let container1 = self.get_enclosing_block_scope_container(node1);
        let container2 = self.get_enclosing_block_scope_container(node2);
        let global1 =
            container1.is_some_and(|container| self.is_global_source_file_node(container));
        let global2 =
            container2.is_some_and(|container| self.is_global_source_file_node(container));
        if global1 {
            global2
        } else if global2 {
            false
        } else {
            container1 == container2
        }
    }

    /// tsc isGlobalSourceFile (14116): SourceFile && not external/CJS.
    fn is_global_source_file_node(&self, node: NodeId) -> bool {
        self.kind_of(node) == SyntaxKind::SourceFile
            && !self.binder.is_external_or_common_js_module_of_node(node)
    }

    /// tsc-port: checkModuleAugmentationElement @6.0.3
    /// tsc-hash: 6c143dd8955aecc6f413dd606e2213dd5aa0c917704594539eb53df32258d3ee
    /// tsc-span: _tsc.js:85925-85963
    ///
    /// The isGlobalAugmentation flag only feeds recursion — in tsc
    /// too (its `return` vs `break` arms are both fall-off-the-end).
    #[allow(clippy::only_used_in_recursion)]
    fn check_module_augmentation_element(
        &mut self,
        node: NodeId,
        is_global_augmentation: bool,
    ) -> CheckResult2<()> {
        match self.kind_of(node) {
            SyntaxKind::VariableStatement => {
                let declarations = match self.data_of(node) {
                    NodeData::VariableStatement(data) => {
                        data.declaration_list
                            .and_then(|list| match self.data_of(list) {
                                NodeData::VariableDeclarationList(data) => data.declarations,
                                _ => None,
                            })
                    }
                    _ => None,
                };
                for declaration in self.nodes_of(declarations) {
                    self.check_module_augmentation_element(declaration, is_global_augmentation)?;
                }
            }
            SyntaxKind::ExportAssignment | SyntaxKind::ExportDeclaration => {
                self.grammar_error_on_first_token(
                    node,
                    &diagnostics::Exports_and_export_assignments_are_not_permitted_in_module_augmentations,
                    &[],
                );
            }
            SyntaxKind::ImportEqualsDeclaration
                if self.is_internal_module_import_equals_declaration(node) => {}
            SyntaxKind::ImportEqualsDeclaration | SyntaxKind::ImportDeclaration => {
                self.grammar_error_on_first_token(
                    node,
                    &diagnostics::Imports_are_not_permitted_in_module_augmentations_Consider_moving_them_to_the_enclosing_external_module,
                    &[],
                );
            }
            SyntaxKind::BindingElement | SyntaxKind::VariableDeclaration => {
                let name = match self.data_of(node) {
                    NodeData::BindingElement(data) => data.name,
                    NodeData::VariableDeclaration(data) => data.name,
                    _ => None,
                };
                if let Some(name) = name {
                    if matches!(
                        self.kind_of(name),
                        SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                    ) {
                        let elements = match self.data_of(name) {
                            NodeData::ObjectBindingPattern(data) => data.elements,
                            NodeData::ArrayBindingPattern(data) => data.elements,
                            _ => None,
                        };
                        for element in self.nodes_of(elements) {
                            self.check_module_augmentation_element(
                                element,
                                is_global_augmentation,
                            )?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// tsc-port: isInternalModuleImportEqualsDeclaration @6.0.3
    /// tsc-hash: 932a80f8017555d577060021099a98a7f7bac95d092e28d05c9a0aa0c90e7bbf
    /// tsc-span: _tsc.js:14877-14879
    pub(crate) fn is_internal_module_import_equals_declaration(&self, node: NodeId) -> bool {
        if self.kind_of(node) != SyntaxKind::ImportEqualsDeclaration {
            return false;
        }
        match self.data_of(node) {
            NodeData::ImportEqualsDeclaration(data) => {
                data.module_reference.is_some_and(|reference| {
                    self.kind_of(reference) != SyntaxKind::ExternalModuleReference
                })
            }
            _ => false,
        }
    }

    /// tsc-port: checkExternalImportOrExportDeclaration @6.0.3
    /// tsc-hash: a5fb1551d6d2e65b3b5d0f9c6e6cc64473914dbc3742409fd1c3d503ea0ac33a
    /// tsc-span: _tsc.js:85983-86018
    fn check_external_import_or_export_declaration(&mut self, node: NodeId) -> CheckResult2<bool> {
        let module_name = self.get_external_module_name_of(node);
        let Some(module_name) = module_name else {
            return Ok(false);
        };
        if node_util::node_is_missing(self.binder.source_of_node(module_name), Some(module_name)) {
            return Ok(false);
        }
        if self.kind_of(module_name) != SyntaxKind::StringLiteral {
            self.error_at(
                Some(module_name),
                &diagnostics::String_literal_expected,
                &[],
            );
            return Ok(false);
        }
        let parent = self.parent_of(node);
        let in_ambient_external_module = parent.is_some_and(|parent| {
            self.kind_of(parent) == SyntaxKind::ModuleBlock
                && self.parent_of(parent).is_some_and(|grand| {
                    node_util::is_ambient_module(self.binder.source_of_node(grand), grand)
                })
        });
        if parent.is_some_and(|parent| self.kind_of(parent) != SyntaxKind::SourceFile)
            && !in_ambient_external_module
        {
            let message = if self.kind_of(node) == SyntaxKind::ExportDeclaration {
                &diagnostics::Export_declarations_are_not_permitted_in_a_namespace
            } else {
                &diagnostics::Import_declarations_in_a_namespace_cannot_reference_a_module
            };
            self.error_at(Some(module_name), message, &[]);
            return Ok(false);
        }
        if in_ambient_external_module {
            let text = match self.data_of(module_name) {
                NodeData::StringLiteral(data) => data.text.clone(),
                _ => String::new(),
            };
            if Self::is_external_module_name_relative(&text)
                && !self.is_top_level_in_external_module_augmentation(node)
            {
                self.error_at(
                    Some(node),
                    &diagnostics::Import_or_export_declaration_in_an_ambient_module_declaration_cannot_reference_module_through_relative_module_name,
                    &[],
                );
                return Ok(false);
            }
        }
        if self.kind_of(node) != SyntaxKind::ImportEqualsDeclaration {
            let attributes = match self.data_of(node) {
                NodeData::ImportDeclaration(data) => data.attributes,
                NodeData::ExportDeclaration(data) => data.attributes,
                _ => None,
            };
            if let Some(attributes) = attributes {
                let (token, elements) = match self.data_of(attributes) {
                    NodeData::ImportAttributes(data) => (data.token, data.elements),
                    _ => (SyntaxKind::WithKeyword, None),
                };
                let message = if token == SyntaxKind::WithKeyword {
                    &diagnostics::Import_attribute_values_must_be_string_literal_expressions
                } else {
                    &diagnostics::Import_assertion_values_must_be_string_literal_expressions
                };
                let mut has_error = false;
                for attribute in self.nodes_of(elements) {
                    let value = match self.data_of(attribute) {
                        NodeData::ImportAttribute(data) => data.value,
                        _ => None,
                    };
                    if let Some(value) = value {
                        if self.kind_of(value) != SyntaxKind::StringLiteral {
                            has_error = true;
                            self.error_at(Some(value), message, &[]);
                        }
                    }
                }
                return Ok(!has_error);
            }
        }
        Ok(true)
    }

    /// tsc getExternalModuleName: the specifier expression of an
    /// import/export declaration or import-equals external reference.
    fn get_external_module_name_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::ImportDeclaration(data) => data.module_specifier,
            NodeData::ExportDeclaration(data) => data.module_specifier,
            NodeData::ImportEqualsDeclaration(data) => {
                let reference = data.module_reference?;
                match self.data_of(reference) {
                    NodeData::ExternalModuleReference(data) => data.expression,
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// tsc isTopLevelInExternalModuleAugmentation.
    fn is_top_level_in_external_module_augmentation(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        if self.kind_of(parent) != SyntaxKind::ModuleBlock {
            return false;
        }
        self.parent_of(parent).is_some_and(|grand| {
            let source = self.binder.source_of_node(grand);
            node_util::is_ambient_module(source, grand)
                && node_util::is_module_augmentation_external(source, grand)
        })
    }

    /// tsc-port: checkModuleExportName @6.0.3
    /// tsc-hash: 71ca29f090ef36bd1e84610275aceb2096279520a8a204c38683ef4a157c9e5d
    /// tsc-span: _tsc.js:86019-86028
    fn check_module_export_name(
        &mut self,
        name: Option<NodeId>,
        allow_string_literal: bool,
    ) -> CheckResult2<()> {
        let Some(name) = name else {
            return Ok(());
        };
        if self.kind_of(name) != SyntaxKind::StringLiteral {
            return Ok(());
        }
        if !allow_string_literal {
            self.grammar_error_on_node(name, &diagnostics::Identifier_expected, &[]);
        } else if matches!(self.options.emit_module_kind(), 5 | 6) {
            self.grammar_error_on_node(
                name,
                &diagnostics::String_literal_import_and_export_names_are_not_supported_when_the_module_flag_is_set_to_es2015_or_es2020,
                &[],
            );
        }
        Ok(())
    }

    /// tsc-port: checkGrammarModuleElementContext @6.0.3
    /// tsc-hash: a6c2a83bd6298d40a08730abc38b97f03d61518a79ca058c6c5c1f6d08ad9040
    /// tsc-span: _tsc.js:86347-86353
    fn check_grammar_module_element_context(
        &mut self,
        node: NodeId,
        error_message: &'static DiagnosticMessage,
    ) -> bool {
        let in_appropriate_context = self.parent_of(node).is_some_and(|parent| {
            matches!(
                self.kind_of(parent),
                SyntaxKind::SourceFile | SyntaxKind::ModuleBlock | SyntaxKind::ModuleDeclaration
            )
        });
        if !in_appropriate_context {
            self.grammar_error_on_first_token(node, error_message, &[]);
        }
        !in_appropriate_context
    }

    /// tsc-port: getAliasDeclarationFromName @6.0.3
    /// tsc-hash: be72387554698d2058b7a2c7ef3379155a639555e57e25ec30257039bd5b5438
    /// tsc-span: _tsc.js:15712-15728
    pub(crate) fn get_alias_declaration_from_name(&self, node: NodeId) -> Option<NodeId> {
        let parent = self.parent_of(node)?;
        match self.kind_of(parent) {
            SyntaxKind::ImportClause
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ExportSpecifier
            | SyntaxKind::ExportAssignment
            | SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::NamespaceExport => Some(parent),
            SyntaxKind::QualifiedName => {
                let mut current = parent;
                while let Some(next) = self.parent_of(current) {
                    if self.kind_of(next) == SyntaxKind::QualifiedName {
                        current = next;
                    } else {
                        break;
                    }
                }
                self.get_alias_declaration_from_name(current)
            }
            _ => None,
        }
    }

    /// tsc-port: checkAliasSymbol @6.0.3
    /// tsc-hash: f38114c6ed2d310327581d9af646d70544ff4e4ae3434764b00d8818bf197779
    /// tsc-span: _tsc.js:86029-86137
    ///
    /// The JS arm is dead (TS files only); the isolatedModules/
    /// verbatimModuleSyntax/Preserve-CJS/ambient-const-enum faces
    /// (86074-86128) are all behind unmodeled options — dead with this
    /// note (getIsolatedModules = isolatedModules ||
    /// verbatimModuleSyntax, both absent). The ImportSpecifier
    /// deprecation walk is suggestion-band — skipped.
    fn check_alias_symbol(&mut self, node: NodeId) -> CheckResult2<()> {
        let symbol = self.get_symbol_of_declaration(node)?;
        let target = self.resolve_alias(symbol)?;
        if target == self.unknown_symbol {
            return Ok(());
        }
        let export_symbol = self.binder.symbol(symbol).export_symbol.unwrap_or(symbol);
        let symbol = self.get_merged_symbol(export_symbol);
        let target_flags = self.get_symbol_flags_of(target)?;
        let symbol_flags = self.binder.symbol(symbol).flags;
        let mut excluded_meanings = SymbolFlags::NONE;
        if symbol_flags.intersects(SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE) {
            excluded_meanings |= SymbolFlags::VALUE;
        }
        if symbol_flags.intersects(SymbolFlags::TYPE) {
            excluded_meanings |= SymbolFlags::TYPE;
        }
        if symbol_flags.intersects(SymbolFlags::NAMESPACE) {
            excluded_meanings |= SymbolFlags::NAMESPACE;
        }
        if target_flags.intersects(excluded_meanings) {
            let message = if self.kind_of(node) == SyntaxKind::ExportSpecifier {
                &diagnostics::Export_declaration_conflicts_with_exported_declaration_of_0
            } else {
                &diagnostics::Import_declaration_conflicts_with_local_declaration_of_0
            };
            let display = self.symbol_display_name(symbol);
            self.error_at(Some(node), message, &[&display]);
        }
        Ok(())
    }

    /// tsc-port: checkImportBinding @6.0.3
    /// tsc-hash: bdbcb3f8fcc5ff897e583528e7557fe9192fd3fe24a8648e97734bb1539cfba8
    /// tsc-span: _tsc.js:86163-86172
    ///
    /// The esModuleInterop default-import probe verifies
    /// `__importDefault` when this file emits CommonJS.
    fn check_import_binding(&mut self, node: NodeId) -> CheckResult2<()> {
        let name = match self.data_of(node) {
            NodeData::ImportClause(data) => data.name,
            NodeData::NamespaceImport(data) => data.name,
            NodeData::ImportSpecifier(data) => data.name,
            NodeData::ImportEqualsDeclaration(data) => data.name,
            _ => None,
        };
        self.check_collisions_for_declaration_name(node, name);
        self.check_alias_symbol(node)?;
        if self.kind_of(node) == SyntaxKind::ImportSpecifier {
            let property_name = match self.data_of(node) {
                NodeData::ImportSpecifier(data) => data.property_name,
                _ => None,
            };
            self.check_module_export_name(property_name, true)?;
            let imported_name = property_name.or(name);
            if imported_name
                .is_some_and(|name| self.module_export_name_text_unescaped(name) == "default")
                && self.options.es_module_interop_effective()
                && self.emit_module_format_is_pre_system(node)
            {
                self.check_external_emit_helpers(node, EMIT_HELPER_IMPORT_DEFAULT)?;
            }
        }
        Ok(())
    }

    /// tsc-port: checkImportAttributes @6.0.3
    /// tsc-hash: 3364af7eedac773cdcb46f3c4f3c52e917037d11f996b2e129f1f9841f76adbb
    /// tsc-span: _tsc.js:86173-86216
    ///
    /// ignoreDeprecations is unmodeled-absent, so the
    /// assert-deprecation row IS live once the module kind supports
    /// attributes. The CommonJS-require row (2856/2836) rides the
    /// specifier's emit syntax and takes priority over the type-only
    /// (2857) and resolution-mode (1454) rows — the oracle-correction
    /// epoch made it observable corpus-wide.
    fn check_import_attributes_of(&mut self, declaration: NodeId) -> CheckResult2<()> {
        let attributes = match self.data_of(declaration) {
            NodeData::ImportDeclaration(data) => data.attributes,
            NodeData::ExportDeclaration(data) => data.attributes,
            _ => None,
        };
        let Some(node) = attributes else {
            return Ok(());
        };
        let import_attributes_type = self.get_global_type("ImportAttributes", 0, true)?;
        if let Some(import_attributes_type) = import_attributes_type {
            if import_attributes_type != self.empty_object_type {
                let source = self.get_type_from_import_attributes(node)?;
                let target = self.get_nullable_type(
                    import_attributes_type,
                    tsrs2_types::TypeFlags::UNDEFINED.bits(),
                );
                self.check_type_assignable_to(
                    source,
                    target,
                    Some(node),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
            }
        }
        let valid_for_type_attributes = self.is_exclusively_type_only_import_or_export(declaration);
        let overridden = self.get_resolution_mode_override(node, valid_for_type_attributes)?;
        let (token, _) = match self.data_of(node) {
            NodeData::ImportAttributes(data) => (data.token, data.elements),
            _ => (SyntaxKind::WithKeyword, None),
        };
        let is_import_attributes = token == SyntaxKind::WithKeyword;
        if valid_for_type_attributes && overridden {
            return Ok(());
        }
        let module_kind = self.options.emit_module_kind();
        let supports =
            (101..=199).contains(&module_kind) || module_kind == 200 || module_kind == 99;
        if !supports {
            let message = if is_import_attributes {
                &diagnostics::Import_attributes_are_only_supported_when_the_module_option_is_set_to_esnext_node18_node20_nodenext_or_preserve
            } else {
                &diagnostics::Import_assertions_are_only_supported_when_the_module_option_is_set_to_esnext_node18_node20_nodenext_or_preserve
            };
            self.grammar_error_on_node(node, message, &[]);
            return Ok(());
        }
        if (102..=199).contains(&module_kind) && !is_import_attributes {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::Import_assertions_have_been_replaced_by_import_attributes_Use_with_instead_of_assert,
                &[],
            );
            return Ok(());
        }
        if !is_import_attributes {
            // ignoreDeprecations !== "6.0" — the option is unmodeled-
            // absent, so the row fires.
            self.grammar_error_on_first_token(
                node,
                &diagnostics::Import_assertions_have_been_replaced_by_import_attributes_Use_with_instead_of_assert,
                &[],
            );
        }
        // CommonJS-require row: an attribute on a statement whose
        // specifier EMITS as a require call. Takes priority over the
        // type-only and resolution-mode rows below (tsc order).
        let module_specifier = match self.data_of(declaration) {
            NodeData::ImportDeclaration(data) => data.module_specifier,
            NodeData::ExportDeclaration(data) => data.module_specifier,
            _ => None,
        };
        if let Some(specifier) = module_specifier {
            if self.emit_syntax_for_declaration_specifier(specifier)
                == Some(ModuleResolutionMode::CommonJs)
            {
                let message = if is_import_attributes {
                    &diagnostics::Import_attributes_are_not_allowed_on_statements_that_compile_to_CommonJS_require_calls
                } else {
                    &diagnostics::Import_assertions_are_not_allowed_on_statements_that_compile_to_CommonJS_require_calls
                };
                self.grammar_error_on_node(node, message, &[]);
                return Ok(());
            }
        }
        let is_type_only = match self.data_of(declaration) {
            NodeData::ImportDeclaration(data) => {
                data.import_clause
                    .is_some_and(|clause| match self.data_of(clause) {
                        NodeData::ImportClause(data) => data.is_type_only,
                        _ => false,
                    })
            }
            NodeData::ExportDeclaration(data) => data.is_type_only,
            _ => false,
        };
        if is_type_only {
            let message = if is_import_attributes {
                &diagnostics::Import_attributes_cannot_be_used_with_type_only_imports_or_exports
            } else {
                &diagnostics::Import_assertions_cannot_be_used_with_type_only_imports_or_exports
            };
            self.grammar_error_on_node(node, message, &[]);
            return Ok(());
        }
        if overridden {
            self.grammar_error_on_node(
                node,
                &diagnostics::resolution_mode_can_only_be_set_for_type_only_imports,
                &[],
            );
        }
        Ok(())
    }

    /// tsc isExclusivelyTypeOnlyImportOrExport: the declaration-level
    /// type-only flag (import type { } / export type { }).
    fn is_exclusively_type_only_import_or_export(&self, declaration: NodeId) -> bool {
        match self.data_of(declaration) {
            NodeData::ExportDeclaration(data) => data.is_type_only,
            NodeData::ImportDeclaration(data) => {
                data.import_clause
                    .is_some_and(|clause| match self.data_of(clause) {
                        NodeData::ImportClause(data) => data.is_type_only,
                        _ => false,
                    })
            }
            _ => false,
        }
    }

    /// tsc-port: getResolutionModeOverride @6.0.3
    /// tsc-hash: cb7d87d78b2941af6b52f61b119f071d490a99d91dba856caa43f0e60ca37f34
    /// tsc-span: _tsc.js:122309-122334
    ///
    /// Returns whether a valid resolution-mode override is present;
    /// the same parser feeds host resolution so grammar checking and
    /// mode selection cannot disagree about the attribute value.
    pub(crate) fn get_resolution_mode_override(
        &mut self,
        node: NodeId,
        report: bool,
    ) -> CheckResult2<bool> {
        match self.parse_resolution_mode_override(node) {
            ResolutionModeOverrideParse::Valid(_) => Ok(true),
            ResolutionModeOverrideParse::WrongCardinality { token } => {
                if report {
                    let message = if token == SyntaxKind::WithKeyword {
                        &diagnostics::Type_import_attributes_should_have_exactly_one_key_resolution_mode_with_value_import_or_require
                    } else {
                        &diagnostics::Type_import_assertions_should_have_exactly_one_key_resolution_mode_with_value_import_or_require
                    };
                    self.grammar_error_on_node(node, message, &[]);
                }
                Ok(false)
            }
            ResolutionModeOverrideParse::InvalidName { token, name } => {
                if report {
                    let message = if token == SyntaxKind::WithKeyword {
                        &diagnostics::resolution_mode_is_the_only_valid_key_for_type_import_attributes
                    } else {
                        &diagnostics::resolution_mode_is_the_only_valid_key_for_type_import_assertions
                    };
                    self.grammar_error_on_node(name, message, &[]);
                }
                Ok(false)
            }
            ResolutionModeOverrideParse::InvalidValue { value } => {
                if report {
                    self.grammar_error_on_node(
                        value,
                        &diagnostics::resolution_mode_should_be_either_require_or_import,
                        &[],
                    );
                }
                Ok(false)
            }
            ResolutionModeOverrideParse::Missing => Ok(false),
        }
    }

    fn parse_resolution_mode_override(&self, node: NodeId) -> ResolutionModeOverrideParse {
        let (token, elements) = match self.data_of(node) {
            NodeData::ImportAttributes(data) => (data.token, data.elements),
            _ => return ResolutionModeOverrideParse::Missing,
        };
        let elements: Vec<NodeId> = self.nodes_of(elements);
        if elements.len() != 1 {
            return ResolutionModeOverrideParse::WrongCardinality { token };
        }
        let (name, value) = match self.data_of(elements[0]) {
            NodeData::ImportAttribute(data) => (data.name, data.value),
            _ => return ResolutionModeOverrideParse::Missing,
        };
        let Some(name) = name else {
            return ResolutionModeOverrideParse::Missing;
        };
        let name_text = match self.data_of(name) {
            NodeData::StringLiteral(data) => data.text.as_str(),
            _ => return ResolutionModeOverrideParse::Missing,
        };
        if name_text != "resolution-mode" {
            return ResolutionModeOverrideParse::InvalidName { token, name };
        }
        let Some(value) = value else {
            return ResolutionModeOverrideParse::Missing;
        };
        let value_text = match self.data_of(value) {
            NodeData::StringLiteral(data) => data.text.as_str(),
            NodeData::NoSubstitutionTemplateLiteral(data) => data.text.as_str(),
            _ => return ResolutionModeOverrideParse::Missing,
        };
        let mode = match value_text {
            "import" => ModuleResolutionMode::EsNext,
            "require" => ModuleResolutionMode::CommonJs,
            _ => return ResolutionModeOverrideParse::InvalidValue { value },
        };
        ResolutionModeOverrideParse::Valid(mode)
    }

    /// tsc-port: getTypeFromImportAttributes @6.0.3
    /// tsc-hash: 230cca270e69688831e489ada99f4658563ce2394ee48cf2beae61b44380947b
    /// tsc-span: _tsc.js:56560-56579
    fn get_type_from_import_attributes(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let object_symbol = self
            .binder
            .create_symbol(SymbolFlags::OBJECT_LITERAL, "__importAttributes".to_owned());
        let mut members = SymbolTable::default();
        let mut properties = Vec::new();
        let elements = match self.data_of(node) {
            NodeData::ImportAttributes(data) => data.elements,
            _ => None,
        };
        for attribute in self.nodes_of(elements) {
            let (name, value) = match self.data_of(attribute) {
                NodeData::ImportAttribute(data) => (data.name, data.value),
                _ => continue,
            };
            let (Some(name), Some(value)) = (name, value) else {
                continue;
            };
            let source = self.binder.source_of_node(name);
            let member_name = node_util::get_text_of_identifier_or_literal(source, name)
                .map(|text| escape_leading_underscores(&text))
                .unwrap_or_default();
            let member = self
                .binder
                .create_symbol(SymbolFlags::PROPERTY, member_name.clone());
            self.binder.symbol_mut(member).parent = Some(object_symbol);
            let value_type = self.check_expression_cached(value, CheckMode::NORMAL)?;
            let member_type = self.tables.get_regular_type_of_literal_type(value_type);
            self.links
                .set_fresh_symbol_type(member, crate::links::LinkSlot::Resolved(member_type));
            self.links
                .set_symbol_target(self.speculation_depth, member, member);
            members.insert(member_name, member);
            properties.push(member);
        }
        let ty = self.make_resolved_anonymous_type(
            Some(object_symbol),
            members,
            properties,
            Vec::new(),
            tsrs2_types::ObjectFlags::OBJECT_LITERAL
                | tsrs2_types::ObjectFlags::NON_INFERRABLE_TYPE,
        );
        self.links
            .set_node_resolved_type(self.speculation_depth, node, LinkSlot::Resolved(ty));
        Ok(ty)
    }

    /// tsc-port: checkImportDeclaration @6.0.3
    /// tsc-hash: fdfed72af1f7631d13e0d945b5dce6c52fe41b4df2e38cdd7f7146bfd1c0f3e6
    /// tsc-span: _tsc.js:86220-86261
    ///
    /// The An_import_declaration_cannot_have_modifiers follower rides
    /// the M7-stub checkGrammarModifiers — contained (a real modifier
    /// error suppresses it in tsc; emitting alongside our stub would
    /// swap codes). noUncheckedSideEffectImports is absent from the
    /// modeled options.
    pub(crate) fn check_import_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        if self.check_grammar_module_element_context(
            node,
            &diagnostics::An_import_declaration_can_only_be_used_at_the_top_level_of_a_namespace_or_module,
        ) {
            return Ok(());
        }
        let _ = self.check_grammar_modifiers(node);
        if self.check_external_import_or_export_declaration(node)? {
            let mut resolved_module = None;
            let import_clause = match self.data_of(node) {
                NodeData::ImportDeclaration(data) => data.import_clause,
                _ => None,
            };
            if let Some(import_clause) = import_clause {
                if !self.check_grammar_import_clause(import_clause)? {
                    let (name, named_bindings) = match self.data_of(import_clause) {
                        NodeData::ImportClause(data) => (data.name, data.named_bindings),
                        _ => (None, None),
                    };
                    if name.is_some() {
                        self.check_import_binding(import_clause)?;
                    }
                    if let Some(named_bindings) = named_bindings {
                        if self.kind_of(named_bindings) == SyntaxKind::NamespaceImport {
                            self.check_import_binding(named_bindings)?;
                            if self.options.es_module_interop_effective()
                                && self.emit_module_format_is_pre_system(node)
                            {
                                self.check_external_emit_helpers(node, EMIT_HELPER_IMPORT_STAR)?;
                            }
                        } else {
                            let module_specifier = match self.data_of(node) {
                                NodeData::ImportDeclaration(data) => data.module_specifier,
                                _ => None,
                            };
                            if let Some(module_specifier) = module_specifier {
                                resolved_module = self.resolve_external_module_name(
                                    node,
                                    module_specifier,
                                    false,
                                )?;
                                if resolved_module.is_some() {
                                    let elements = match self.data_of(named_bindings) {
                                        NodeData::NamedImports(data) => data.elements,
                                        _ => None,
                                    };
                                    for element in self.nodes_of(elements) {
                                        self.check_import_binding(element)?;
                                    }
                                }
                            }
                        }
                    }
                    let is_type_only = matches!(
                        self.data_of(import_clause),
                        NodeData::ImportClause(data) if data.is_type_only
                    );
                    let module_kind = self.options.emit_module_kind();
                    let module_specifier = match self.data_of(node) {
                        NodeData::ImportDeclaration(data) => data.module_specifier,
                        _ => None,
                    };
                    let requires_json_attribute =
                        !is_type_only && (101..=199).contains(&module_kind);
                    let is_default_only = if requires_json_attribute {
                        match module_specifier {
                            Some(module_specifier) => self
                                .is_only_importable_as_default(module_specifier, resolved_module)?,
                            None => false,
                        }
                    } else {
                        false
                    };
                    if requires_json_attribute
                        && is_default_only
                        && !self.has_type_json_import_attribute(node)
                    {
                        let module_kind_name = match module_kind {
                            101 => "Node18",
                            102 => "Node20",
                            199 => "NodeNext",
                            _ => "NodeNext",
                        };
                        self.error_at(
                            module_specifier,
                            &diagnostics::Importing_a_JSON_file_into_an_ECMAScript_module_requires_a_type_json_import_attribute_when_module_is_set_to_0,
                            &[module_kind_name],
                        );
                    }
                }
            }
        }
        self.check_import_attributes_of(node)
    }

    /// tsc-port: hasTypeJsonImportAttribute @6.0.3
    /// tsc-hash: 9f158dc8bd728ee72d1feaef826e583a1bb40a5bb4e8787ed16b12345b411220
    /// tsc-span: _tsc.js:86262-86266
    fn has_type_json_import_attribute(&self, declaration: NodeId) -> bool {
        let attributes = match self.data_of(declaration) {
            NodeData::ImportDeclaration(data) => data.attributes,
            _ => None,
        };
        let elements = attributes.and_then(|attributes| match self.data_of(attributes) {
            NodeData::ImportAttributes(data) => data.elements,
            _ => None,
        });
        self.nodes_of(elements).into_iter().any(|attribute| {
            let (name, value) = match self.data_of(attribute) {
                NodeData::ImportAttribute(data) => (data.name, data.value),
                _ => return false,
            };
            let Some(name) = name else {
                return false;
            };
            let source = self.binder.source_of_node(name);
            let name_matches = node_util::get_text_of_identifier_or_literal(source, name)
                .is_some_and(|text| text == "type");
            name_matches
                && value.is_some_and(|value| {
                    matches!(
                        self.data_of(value),
                        NodeData::StringLiteral(data) if data.text == "json"
                    )
                })
        })
    }

    /// tsc-port: checkGrammarImportClause @6.0.3
    /// tsc-hash: 135005635990b013ef4d869f68234202f2703dae3e7cb87768337c7a642852c3
    /// tsc-span: _tsc.js:90396-90417
    ///
    /// The defer-phase arm (tsc 90405-90414: diags 18058/18059/18060 off
    /// phase_modifier == DeferKeyword) is not ported yet — type-only arms
    /// only.
    fn check_grammar_import_clause(&mut self, node: NodeId) -> CheckResult2<bool> {
        let (is_type_only, name, named_bindings) = match self.data_of(node) {
            NodeData::ImportClause(data) => (data.is_type_only, data.name, data.named_bindings),
            _ => return Ok(false),
        };
        if is_type_only {
            if name.is_some() && named_bindings.is_some() {
                return Ok(self.grammar_error_on_node(
                    node,
                    &diagnostics::A_type_only_import_can_specify_a_default_import_or_named_bindings_but_not_both,
                    &[],
                ));
            }
            if let Some(named_bindings) = named_bindings {
                if self.kind_of(named_bindings) == SyntaxKind::NamedImports {
                    return self.check_grammar_named_imports_or_exports(named_bindings);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: checkGrammarNamedImportsOrExports @6.0.3
    /// tsc-hash: eaaf92dfe56485390b1e6ed697a21d644959d0151d80c999953723c41ca882d4
    /// tsc-span: _tsc.js:90418-90427
    fn check_grammar_named_imports_or_exports(
        &mut self,
        named_bindings: NodeId,
    ) -> CheckResult2<bool> {
        let elements = match self.data_of(named_bindings) {
            NodeData::NamedImports(data) => data.elements,
            NodeData::NamedExports(data) => data.elements,
            _ => None,
        };
        for specifier in self.nodes_of(elements) {
            let is_type_only = match self.data_of(specifier) {
                NodeData::ImportSpecifier(data) => data.is_type_only,
                NodeData::ExportSpecifier(data) => data.is_type_only,
                _ => false,
            };
            if is_type_only {
                let message = if self.kind_of(specifier) == SyntaxKind::ImportSpecifier {
                    &diagnostics::The_type_modifier_cannot_be_used_on_a_named_import_when_import_type_is_used_on_its_import_statement
                } else {
                    &diagnostics::The_type_modifier_cannot_be_used_on_a_named_export_when_export_type_is_used_on_its_export_statement
                };
                return Ok(self.grammar_error_on_first_token(specifier, message, &[]));
            }
        }
        Ok(false)
    }

    /// tsc-port: checkImportEqualsDeclaration @6.0.3
    /// tsc-hash: 917a3ecc1c32fbf5aaef34a7341ab5a1f9ae58bd9a73123726ae69689ac60388
    /// tsc-span: _tsc.js:86268-86302
    ///
    /// erasableSyntaxOnly is unmodeled-dead; markLinkedReferences is
    /// emit machinery.
    pub(crate) fn check_import_equals_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        if self.check_grammar_module_element_context(
            node,
            &diagnostics::An_import_declaration_can_only_be_used_at_the_top_level_of_a_namespace_or_module,
        ) {
            return Ok(());
        }
        let _ = self.check_grammar_modifiers(node);
        let is_internal = self.is_internal_module_import_equals_declaration(node);
        if is_internal || self.check_external_import_or_export_declaration(node)? {
            self.check_import_binding(node)?;
            let (is_type_only, module_reference) = match self.data_of(node) {
                NodeData::ImportEqualsDeclaration(data) => {
                    (data.is_type_only, data.module_reference)
                }
                _ => (false, None),
            };
            let Some(module_reference) = module_reference else {
                return Ok(());
            };
            if self.kind_of(module_reference) != SyntaxKind::ExternalModuleReference {
                let symbol = self.get_symbol_of_declaration(node)?;
                let target = self.resolve_alias(symbol)?;
                if target != self.unknown_symbol {
                    let target_flags = self.get_symbol_flags_of(target)?;
                    if target_flags.intersects(SymbolFlags::VALUE) {
                        let module_name = self.first_identifier(module_reference);
                        let resolved = self.resolve_entity_name(
                            module_name,
                            SymbolFlags::VALUE | SymbolFlags::NAMESPACE,
                            /*ignore_errors*/ false,
                            None,
                        )?;
                        let resolved_is_namespace = resolved.is_some_and(|resolved| {
                            self.binder
                                .symbol(resolved)
                                .flags
                                .intersects(SymbolFlags::NAMESPACE)
                        });
                        if !resolved_is_namespace {
                            let display = node_util::declaration_name_to_string(
                                self.binder.source_of_node(module_name),
                                Some(module_name),
                            );
                            self.error_at(
                                Some(module_name),
                                &diagnostics::Module_0_is_hidden_by_a_local_declaration_with_the_same_name,
                                &[&display],
                            );
                        }
                    }
                    if target_flags.intersects(SymbolFlags::TYPE) {
                        if let Some(name) = match self.data_of(node) {
                            NodeData::ImportEqualsDeclaration(data) => data.name,
                            _ => None,
                        } {
                            self.check_type_name_is_reserved(
                                name,
                                &diagnostics::Import_name_cannot_be_0,
                            );
                        }
                    }
                }
                if is_type_only {
                    self.grammar_error_on_node(
                        node,
                        &diagnostics::An_import_alias_cannot_use_import_type,
                        &[],
                    );
                }
            } else {
                let module_kind = self.options.emit_module_kind();
                let ambient = self
                    .binder
                    .flags_of(node)
                    .intersects(tsrs2_types::NodeFlags::AMBIENT);
                if (5..=99).contains(&module_kind) && !is_type_only && !ambient {
                    self.grammar_error_on_node(
                        node,
                        &diagnostics::Import_assignment_cannot_be_used_when_targeting_ECMAScript_modules_Consider_using_import_as_ns_from_mod_import_a_from_mod_import_d_from_mod_or_another_module_format_instead,
                        &[],
                    );
                }
            }
        }
        Ok(())
    }

    /// tsc-port: checkExportDeclaration @6.0.3
    /// tsc-hash: f4ecd02d9c128e962482b8cf3e24b113cbf7b2cf3f377c26f9b05684f0fef49d
    /// tsc-span: _tsc.js:86303-86339
    ///
    /// The An_export_declaration_cannot_have_modifiers follower rides
    /// the M7-stub checkGrammarModifiers — contained (same as the
    /// import flavor). Import/export helper availability follows the
    /// source file's effective emit format.
    pub(crate) fn check_export_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        if self.check_grammar_module_element_context(
            node,
            &diagnostics::An_export_declaration_can_only_be_used_at_the_top_level_of_a_namespace_or_module,
        ) {
            return Ok(());
        }
        let _ = self.check_grammar_modifiers(node);
        self.check_grammar_export_declaration(node)?;
        let (module_specifier, export_clause) = match self.data_of(node) {
            NodeData::ExportDeclaration(data) => (data.module_specifier, data.export_clause),
            _ => (None, None),
        };
        if module_specifier.is_none() || self.check_external_import_or_export_declaration(node)? {
            let is_namespace_export = export_clause
                .is_some_and(|clause| self.kind_of(clause) == SyntaxKind::NamespaceExport);
            if let (Some(export_clause), false) = (export_clause, is_namespace_export) {
                let elements = match self.data_of(export_clause) {
                    NodeData::NamedExports(data) => data.elements,
                    _ => None,
                };
                for element in self.nodes_of(elements) {
                    self.check_export_specifier(element)?;
                }
                let parent = self.parent_of(node);
                let in_ambient_external_module = parent.is_some_and(|parent| {
                    self.kind_of(parent) == SyntaxKind::ModuleBlock
                        && self.parent_of(parent).is_some_and(|grand| {
                            node_util::is_ambient_module(self.binder.source_of_node(grand), grand)
                        })
                });
                let in_ambient_namespace_declaration = !in_ambient_external_module
                    && parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ModuleBlock)
                    && module_specifier.is_none()
                    && self
                        .binder
                        .flags_of(node)
                        .intersects(tsrs2_types::NodeFlags::AMBIENT);
                if parent.is_some_and(|parent| self.kind_of(parent) != SyntaxKind::SourceFile)
                    && !in_ambient_external_module
                    && !in_ambient_namespace_declaration
                {
                    self.error_at(
                        Some(node),
                        &diagnostics::Export_declarations_are_not_permitted_in_a_namespace,
                        &[],
                    );
                }
            } else if let Some(module_specifier) = module_specifier {
                let module_symbol =
                    self.resolve_external_module_name(node, module_specifier, false)?;
                if let Some(module_symbol) = module_symbol {
                    if self.has_export_assignment_symbol(module_symbol) {
                        let display = self.symbol_display_name(module_symbol);
                        self.error_at(
                            Some(module_specifier),
                            &diagnostics::Module_0_uses_export_and_cannot_be_used_with_export,
                            &[&display],
                        );
                    } else if let Some(export_clause) = export_clause {
                        self.check_alias_symbol(export_clause)?;
                        let clause_name = match self.data_of(export_clause) {
                            NodeData::NamespaceExport(data) => data.name,
                            _ => None,
                        };
                        self.check_module_export_name(clause_name, true)?;
                    }
                } else if let Some(export_clause) = export_clause {
                    self.check_alias_symbol(export_clause)?;
                    let clause_name = match self.data_of(export_clause) {
                        NodeData::NamespaceExport(data) => data.name,
                        _ => None,
                    };
                    self.check_module_export_name(clause_name, true)?;
                }
                if self.emit_module_format_is_pre_system(node) {
                    if export_clause.is_some() {
                        if self.options.es_module_interop_effective() {
                            self.check_external_emit_helpers(node, EMIT_HELPER_IMPORT_STAR)?;
                        }
                    } else {
                        self.check_external_emit_helpers(node, EMIT_HELPER_EXPORT_STAR)?;
                    }
                }
            }
        }
        self.check_import_attributes_of(node)
    }

    /// tsc-port: checkGrammarExportDeclaration @6.0.3
    /// tsc-hash: 73214d3d7eb2caaa902d8aed441dbdcb290a02834dd7e14469c594bbc131b2fb
    /// tsc-span: _tsc.js:86340-86346
    fn check_grammar_export_declaration(&mut self, node: NodeId) -> CheckResult2<bool> {
        let (is_type_only, export_clause) = match self.data_of(node) {
            NodeData::ExportDeclaration(data) => (data.is_type_only, data.export_clause),
            _ => return Ok(false),
        };
        if is_type_only {
            if let Some(export_clause) = export_clause {
                if self.kind_of(export_clause) == SyntaxKind::NamedExports {
                    return self.check_grammar_named_imports_or_exports(export_clause);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: checkExportSpecifier @6.0.3
    /// tsc-hash: 9140193d8815ff3c9aef7ceed229317e0c48d27a8badb69d6b3051e5508ae137
    /// tsc-span: _tsc.js:86354-86390
    ///
    /// collectLinkedAliases/markLinkedReferences are declaration-emit
    /// bookkeeping; a default re-export can require
    /// `__importDefault` under CommonJS interop.
    fn check_export_specifier(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_alias_symbol(node)?;
        let (property_name, name) = match self.data_of(node) {
            NodeData::ExportSpecifier(data) => (data.property_name, data.name),
            _ => (None, None),
        };
        let has_module_specifier = self
            .parent_of(node)
            .and_then(|named| self.parent_of(named))
            .is_some_and(|declaration| match self.data_of(declaration) {
                NodeData::ExportDeclaration(data) => data.module_specifier.is_some(),
                _ => false,
            });
        self.check_module_export_name(property_name, has_module_specifier)?;
        self.check_module_export_name(name, true)?;
        if !has_module_specifier {
            let Some(exported_name) = property_name.or(name) else {
                return Ok(());
            };
            if self.kind_of(exported_name) == SyntaxKind::StringLiteral {
                return Ok(());
            }
            let text = self.module_export_name_text_escaped(exported_name);
            let symbol = self.resolve_name(
                Some(exported_name),
                &text,
                SymbolFlags::VALUE
                    | SymbolFlags::TYPE
                    | SymbolFlags::NAMESPACE
                    | SymbolFlags::ALIAS,
                /*name_not_found_message*/ None,
                /*is_use*/ true,
                /*exclude_globals*/ false,
            )?;
            if let Some(symbol) = symbol {
                let is_global_declared = symbol != self.undefined_symbol
                    && symbol != self.global_this_symbol
                    && self
                        .binder
                        .symbol(symbol)
                        .declarations
                        .first()
                        .copied()
                        .and_then(|declaration| {
                            self.get_enclosing_block_scope_container(declaration)
                        })
                        .is_some_and(|container| self.is_global_source_file_node(container));
                if symbol == self.undefined_symbol
                    || symbol == self.global_this_symbol
                    || is_global_declared
                {
                    let display = self.module_export_name_text_unescaped(exported_name);
                    self.error_at(
                        Some(exported_name),
                        &diagnostics::Cannot_export_0_Only_local_declarations_can_be_exported_from_a_module,
                        &[&display],
                    );
                }
            }
        } else if property_name
            .or(name)
            .is_some_and(|name| self.module_export_name_text_unescaped(name) == "default")
            && self.options.es_module_interop_effective()
            && self.emit_module_format_is_pre_system(node)
        {
            self.check_external_emit_helpers(node, EMIT_HELPER_IMPORT_DEFAULT)?;
        }
        Ok(())
    }

    /// tsc-port: checkExportAssignment @6.0.3
    /// tsc-hash: 2481cb70fb33663829bfdf493fafea233783acafbd6069e1e83a2f85c5cf1a19
    /// tsc-span: _tsc.js:86391-86501
    ///
    /// Dead rows at the modeled defaults: erasableSyntaxOnly,
    /// verbatimModuleSyntax faces, the isolatedModules type-only
    /// re-export band, collectLinkedAliases (emit). The JSDoc type
    /// annotation arm is JS-only. The export= tails read the modeled
    /// module option (impliedNodeFormat reduces: non-ambient files
    /// carry no format, so `!== CommonJS` holds and the ES2015+ row
    /// fires; the ambient ESNext face never does).
    pub(crate) fn check_export_assignment(&mut self, node: NodeId) -> CheckResult2<()> {
        let is_export_equals = match self.data_of(node) {
            NodeData::ExportAssignment(data) => data.is_export_equals == Some(true),
            _ => false,
        };
        let illegal_context_message = if is_export_equals {
            &diagnostics::An_export_assignment_must_be_at_the_top_level_of_a_file_or_module_declaration
        } else {
            &diagnostics::A_default_export_must_be_at_the_top_level_of_a_file_or_module_declaration
        };
        if self.check_grammar_module_element_context(node, illegal_context_message) {
            return Ok(());
        }
        let container = match self.parent_of(node) {
            Some(parent) if self.kind_of(parent) == SyntaxKind::SourceFile => parent,
            Some(parent) => match self.parent_of(parent) {
                Some(grand) => grand,
                None => return Ok(()),
            },
            None => return Ok(()),
        };
        if self.kind_of(container) == SyntaxKind::ModuleDeclaration
            && !node_util::is_ambient_module(self.binder.source_of_node(container), container)
        {
            if is_export_equals {
                self.error_at(
                    Some(node),
                    &diagnostics::An_export_assignment_cannot_be_used_in_a_namespace,
                    &[],
                );
            } else {
                self.error_at(
                    Some(node),
                    &diagnostics::A_default_export_can_only_be_used_in_an_ECMAScript_style_module,
                    &[],
                );
            }
            return Ok(());
        }
        let _ = self.check_grammar_modifiers(node);
        let expression = match self.data_of(node) {
            NodeData::ExportAssignment(data) => data.expression,
            _ => None,
        };
        let Some(expression) = expression else {
            return Ok(());
        };
        let ambient = self
            .binder
            .flags_of(node)
            .intersects(tsrs2_types::NodeFlags::AMBIENT);
        if self.kind_of(expression) == SyntaxKind::Identifier {
            let sym = self.resolve_entity_name_ex(
                expression,
                SymbolFlags::ALL,
                /*ignore_errors*/ true,
                /*location*/ Some(node),
                /*dont_resolve_alias*/ true,
            )?;
            let sym = sym.map(|sym| self.get_export_symbol_of_value_symbol_if_exported(sym));
            if let Some(sym) = sym {
                if self
                    .get_symbol_flags_of(sym)?
                    .intersects(SymbolFlags::VALUE)
                {
                    // A pure-type export= does NOT check the
                    // expression (no 2693 here).
                    self.check_expression_cached(expression, CheckMode::NORMAL)?;
                }
            } else {
                self.check_expression_cached(expression, CheckMode::NORMAL)?;
            }
        } else {
            self.check_expression_cached(expression, CheckMode::NORMAL)?;
        }
        self.check_external_module_exports(container)?;
        if ambient && !self.is_entity_name_expression(expression) {
            self.grammar_error_on_node(
                expression,
                &diagnostics::The_expression_of_an_export_assignment_must_be_an_identifier_or_qualified_name_in_an_ambient_context,
                &[],
            );
        }
        if is_export_equals {
            let module_kind = self.options.emit_module_kind();
            let implied_node_format = self.implied_node_format_for_file(node);
            let invalid_esm_export_assignment = if ambient {
                (100..=199).contains(&module_kind)
                    && implied_node_format == ModuleResolutionMode::EsNext
            } else if (100..=199).contains(&module_kind) {
                implied_node_format != ModuleResolutionMode::CommonJs
            } else {
                true
            };
            if module_kind >= 5 && module_kind != 200 && invalid_esm_export_assignment {
                self.grammar_error_on_node(
                    node,
                    &diagnostics::Export_assignment_cannot_be_used_when_targeting_ECMAScript_modules_Consider_using_export_default_or_another_module_format_instead,
                    &[],
                );
            } else if module_kind == 4 && !ambient {
                self.grammar_error_on_node(
                    node,
                    &diagnostics::Export_assignment_is_not_supported_when_module_flag_is_system,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: checkExternalModuleExports @6.0.3
    /// tsc-hash: 2d0f81291dd10b3cf351c83edab8f11522efad0f2939cece56fa438d6e8cec05
    /// tsc-span: _tsc.js:86505-86542
    ///
    /// isDuplicatedCommonJSExport is JS-only and always false here.
    pub(crate) fn check_external_module_exports(&mut self, container: NodeId) -> CheckResult2<()> {
        let Some(module_symbol) = self.binder.node_symbol(container) else {
            return Ok(());
        };
        let module_symbol = self.get_merged_symbol(module_symbol);
        if self.links.symbol(module_symbol).exports_checked {
            return Ok(());
        }
        let export_equals_symbol = self
            .binder
            .symbol(module_symbol)
            .exports
            .get(InternalSymbolName::EXPORT_EQUALS)
            .copied();
        if let Some(export_equals_symbol) = export_equals_symbol {
            if self.has_exported_members(module_symbol) {
                let declaration = self
                    .get_declaration_of_alias_symbol(export_equals_symbol)
                    .or(self.binder.symbol(export_equals_symbol).value_declaration);
                if let Some(declaration) = declaration {
                    if !self.is_top_level_in_external_module_augmentation(declaration) {
                        self.error_at(
                            Some(declaration),
                            &diagnostics::An_export_assignment_cannot_be_used_in_a_module_with_other_exported_elements,
                            &[],
                        );
                    }
                }
            }
        }
        let exports = self.get_exports_of_module(module_symbol)?;
        for (id, &export_symbol) in &exports {
            if id == InternalSymbolName::EXPORT_STAR {
                continue;
            }
            let flags = self.binder.symbol(export_symbol).flags;
            if flags.intersects(SymbolFlags::NAMESPACE | SymbolFlags::ENUM) {
                continue;
            }
            let declarations = self.binder.symbol(export_symbol).declarations.clone();
            let exported_declarations_count = declarations
                .iter()
                .filter(|&&declaration| {
                    self.is_not_overload(declaration)
                        && !self.is_accessor_declaration(declaration)
                        && self.kind_of(declaration) != SyntaxKind::InterfaceDeclaration
                })
                .count();
            if flags.intersects(SymbolFlags::TYPE_ALIAS) && exported_declarations_count <= 2 {
                continue;
            }
            if exported_declarations_count > 1 {
                for &declaration in &declarations {
                    if self.is_not_overload(declaration) {
                        self.error_at(
                            Some(declaration),
                            &diagnostics::Cannot_redeclare_exported_variable_0,
                            &[unescape_leading_underscores(id)],
                        );
                    }
                }
            }
        }
        self.links
            .set_symbol_exports_checked(self.speculation_depth, module_symbol);
        Ok(())
    }

    /// tsc-port: hasExportedMembers @6.0.3
    /// tsc-hash: 2c3014a8e3dca95c995088d8049662c89a71a6a202029c56415a030dbd32bc94
    /// tsc-span: _tsc.js:86502-86504
    fn has_exported_members(&self, module_symbol: SymbolId) -> bool {
        self.binder
            .symbol(module_symbol)
            .exports
            .keys()
            .any(|id| id != InternalSymbolName::EXPORT_EQUALS)
    }

    /// tsc isNotOverload: not a bodiless function/method declaration.
    fn is_not_overload(&self, declaration: NodeId) -> bool {
        !matches!(
            self.kind_of(declaration),
            SyntaxKind::FunctionDeclaration | SyntaxKind::MethodDeclaration
        ) || node_util::body_of(self.binder.source_of_node(declaration), declaration).is_some()
    }

    /// Accessor probe for the checkExternalModuleExports count.
    fn is_accessor_declaration(&self, declaration: NodeId) -> bool {
        matches!(
            self.kind_of(declaration),
            SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{check_program, CompilerOptions, InputFile};

    /// Driver-level multi-file rows: (file, code, start, length) for
    /// located diagnostics (noLib artifacts are locationless and drop
    /// with the filter — the calls.rs checked_rows discipline).
    fn program_rows(
        files: &[(&str, &str)],
        options: &CompilerOptions,
    ) -> Vec<(String, u32, u32, u32)> {
        let inputs: Vec<InputFile> = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let result = check_program(&inputs, options);
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.file_name.is_some())
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().expect("filtered"),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    }

    fn rows(files: &[(&str, &str)]) -> Vec<(String, u32, u32, u32)> {
        program_rows(files, &CompilerOptions::default())
    }

    fn node16_options() -> CompilerOptions {
        CompilerOptions {
            module: Some(100),
            target: Some(9),
            ..CompilerOptions::default()
        }
    }

    /// Oracle pins (tsc 6.0.3, scratchpad probe58d/pins, 2026-07-15).
    #[test]
    fn not_a_module_and_missing_member_report_2306_and_2305() {
        let files = [
            ("script.ts", "var g = 1;\n"),
            ("other.ts", "export const yes = 1;\n"),
            (
                "amb.d.ts",
                "declare module \"amb\" { export const a: number; }\n",
            ),
            (
                "main.ts",
                "import { a } from \"amb\";\nimport * as s from \"./script\";\nimport { nope } from \"./other\";\na; s; nope;\n",
            ),
        ];
        assert_eq!(
            rows(&files),
            [
                ("main.ts".to_owned(), 2306, 44, 10),
                ("main.ts".to_owned(), 2305, 65, 4),
            ]
        );
    }

    #[test]
    fn export_assignment_rows_1203_2309_1202() {
        let files = [
            ("m.ts", "const x = 1;\nexport = x;\nexport const y = 2;\n"),
            ("main.ts", "import m = require(\"./m\");\nm;\n"),
        ];
        assert_eq!(
            rows(&files),
            [
                ("m.ts".to_owned(), 1203, 13, 11),
                ("m.ts".to_owned(), 2309, 13, 11),
                ("main.ts".to_owned(), 1202, 0, 26),
            ]
        );
    }

    #[test]
    fn module_keyword_and_quoted_name_rows_1540_1035() {
        let files = [(
            "a.ts",
            "module M { export const x = 1; }\nmodule \"bad\" {}\nexport {};\n",
        )];
        assert_eq!(
            rows(&files),
            [
                ("a.ts".to_owned(), 1540, 7, 1),
                ("a.ts".to_owned(), 1035, 40, 5),
            ]
        );
    }

    #[test]
    fn circular_import_alias_reports_2303() {
        let files = [("c.ts", "import A = B;\nimport B = A;\nA; B;\nexport {};\n")];
        assert_eq!(rows(&files), [("c.ts".to_owned(), 2303, 0, 13)]);
    }

    /// Static and dynamic imports in an .mts file use Node's ESM
    /// resolution mode and therefore never extension-probe.
    #[test]
    fn static_mts_imports_require_explicit_extensions_under_node16() {
        let files = [
            ("/src/foo.mts", "export function foo() { return \"\"; }\n"),
            (
                "/src/bar.mts",
                "import { foo } from \"./foo\";\nimport { baz } from \"./baz\";\n",
            ),
        ];
        assert_eq!(
            program_rows(&files, &node16_options()),
            [
                ("/src/bar.mts".to_owned(), 2835, 20, 7),
                ("/src/bar.mts".to_owned(), 2834, 49, 7),
            ]
        );
    }

    /// Static imports and import() share the ESM resolution mode in an
    /// .mts file. The noLib Promise 2711 rides the import call.
    #[test]
    fn static_and_dynamic_mts_imports_share_node_esm_resolution() {
        let files = [
            ("foo.ts", "export const x = 1;\n"),
            (
                "buzz.mts",
                "import(\"./foo\");\nimport { x } from \"./foo\";\nx;\n",
            ),
        ];
        assert_eq!(
            program_rows(&files, &node16_options()),
            [
                ("buzz.mts".to_owned(), 2711, 0, 15),
                ("buzz.mts".to_owned(), 2835, 7, 7),
                ("buzz.mts".to_owned(), 2835, 35, 7),
            ]
        );
    }

    #[test]
    fn dynamic_import_in_plain_ts_uses_node_esm_resolution() {
        let files = [
            ("foo.ts", "export const x = 1;\n"),
            ("main.ts", "import(\"./foo\");\n"),
        ];
        assert_eq!(
            program_rows(&files, &node16_options()),
            [
                ("main.ts".to_owned(), 2711, 0, 15),
                ("main.ts".to_owned(), 2835, 7, 7),
            ]
        );
    }

    #[test]
    fn import_equals_in_mts_uses_commonjs_resolution() {
        let files = [("main.mts", "import foo = require(\"./foo\");\n")];
        assert_eq!(
            program_rows(&files, &node16_options()),
            [("main.mts".to_owned(), 2307, 21, 7)]
        );
    }

    #[test]
    fn type_only_resolution_mode_override_controls_node_resolution() {
        let files = [
            ("foo.ts", "export type X = number;\n"),
            ("package.json", "{}\n"),
            (
                "main.mts",
                "import type { Missing } from \"./foo\" with { \"resolution-mode\": \"require\" };\n",
            ),
            (
                "main.cts",
                "import type { X } from \"./foo\" with { \"resolution-mode\": \"import\" };\n",
            ),
        ];
        assert_eq!(
            program_rows(&files, &node16_options()),
            [
                ("main.cts".to_owned(), 2835, 23, 7),
                ("main.mts".to_owned(), 2305, 14, 7),
            ]
        );
    }

    #[test]
    fn explicit_non_ts_extension_reports_plain_2307_under_node16() {
        let files = [("/src/main.mts", "export * from \"./missing.css\";\n")];
        assert_eq!(
            program_rows(&files, &node16_options()),
            [("/src/main.mts".to_owned(), 2307, 14, 15)]
        );
    }

    /// Recognized extensions use their fixed substitution groups.
    /// Arbitrary declaration twins such as file.d.js.ts must not make
    /// those authoritative misses host-dependent.
    #[test]
    fn recognized_extension_misses_ignore_arbitrary_declaration_twins() {
        let files = [
            (
                "/main.ts",
                "import d1 from \"./file.js\";\nimport d2 from \"./file.jsx\";\nimport d3 from \"./file.ts\";\nimport d4 from \"./file.tsx\";\nimport d5 from \"./file.mjs\";\nimport d6 from \"./file.cjs\";\nimport d7 from \"./file.mts\";\nimport d8 from \"./file.cts\";\nimport d9 from \"./file.d.ts\";\nimport d10 from \"./file.d.cts\";\nimport d11 from \"./file.d.mts\";\nimport d12 from \"./file.d.json.ts\";\nd1; d2; d3; d4; d5; d6; d7; d8; d9; d10; d11; d12;\n",
            ),
            ("/file.d.js.ts", "export {};\n"),
            ("/file.d.jsx.ts", "export {};\n"),
            ("/file.d.ts.ts", "export {};\n"),
            ("/file.d.tsx.ts", "export {};\n"),
            ("/file.d.mjs.ts", "export {};\n"),
            ("/file.d.cjs.ts", "export {};\n"),
            ("/file.d.mts.ts", "export {};\n"),
            ("/file.d.cts.ts", "export {};\n"),
            ("/file.d.d.ts.ts", "export {};\n"),
            ("/file.d.d.cts.ts", "export {};\n"),
            ("/file.d.d.mts.ts", "export {};\n"),
            ("/file.d.d.json.ts", "export {};\n"),
        ];
        let diagnostics = program_rows(&files, &node16_options());
        assert_eq!(diagnostics.len(), 12, "{diagnostics:?}");
        assert!(diagnostics
            .iter()
            .all(|(file, code, _, _)| file == "/main.ts" && *code == 2307));
    }

    #[test]
    fn checked_js_require_literal_publishes_definite_module_miss() {
        let files = [(
            "/a.js",
            "require(\"\" + \"./foo.ts\");\nrequire(\"./foo.ts\");\n",
        )];
        assert_eq!(
            program_rows(
                &files,
                &CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    ..CompilerOptions::default()
                }
            ),
            [("/a.js".to_owned(), 2307, 34, 10)]
        );
    }

    #[test]
    fn explicit_mts_cts_extensions_report_or_suggest_the_full_extension() {
        let files = [
            ("/main.ts", "import {} from \"./foo.d.mts\";\nimport {} from \"./bar.d.cts\";\nimport {} from \"./baz.mts\";\nimport {} from \"./qux.cts\";\n"),
            ("/foo.d.mts", "export {};\n"),
            ("/bar.d.cts", "export {};\n"),
            ("/baz.mts", "export {};\n"),
            ("/qux.cts", "export {};\n"),
        ];
        let inputs: Vec<InputFile> = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let diagnostics = check_program(&inputs, &CompilerOptions::default()).diagnostics;
        let pins: Vec<(u32, u32, String)> = diagnostics
            .iter()
            .filter_map(|diagnostic| {
                diagnostic.start.map(|start| {
                    (
                        diagnostic.code(),
                        start,
                        diagnostic.message_text().to_owned(),
                    )
                })
            })
            .collect();
        assert_eq!(
            pins,
            [
                (
                    2846,
                    15,
                    "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './foo.mjs' instead?".to_owned(),
                ),
                (
                    2846,
                    45,
                    "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './bar.js' instead?".to_owned(),
                ),
                (
                    5097,
                    75,
                    "An import path can only end with a '.mts' extension when 'allowImportingTsExtensions' is enabled.".to_owned(),
                ),
                (
                    5097,
                    103,
                    "An import path can only end with a '.cts' extension when 'allowImportingTsExtensions' is enabled.".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn declaration_import_suggestion_uses_usage_emit_mode() {
        let files = [
            ("/main.ts", "import(\"./foo.d.mts\");\n"),
            ("/foo.d.mts", "export {};\n"),
        ];
        let inputs: Vec<InputFile> = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let diagnostics = check_program(
            &inputs,
            &CompilerOptions {
                module: Some(1),
                target: Some(9),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
        let pins: Vec<(u32, u32, String)> = diagnostics
            .iter()
            .filter_map(|diagnostic| {
                diagnostic.start.map(|start| {
                    (
                        diagnostic.code(),
                        start,
                        diagnostic.message_text().to_owned(),
                    )
                })
            })
            .collect();
        assert_eq!(
            pins,
            [
                (
                    2711,
                    0,
                    "A dynamic import call returns a 'Promise'. Make sure you have a declaration for 'Promise' or include 'ES2015' in your '--lib' option.".to_owned(),
                ),
                (
                    2846,
                    7,
                    "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './foo' instead?".to_owned(),
                ),
            ]
        );

        let files = [
            ("/main.ts", "import {} from \"./foo.d.mts\";\n"),
            ("/foo.d.mts", "export {};\n"),
        ];
        let inputs: Vec<InputFile> = files
            .iter()
            .map(|(name, text)| InputFile {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let diagnostics = check_program(
            &inputs,
            &CompilerOptions {
                module: Some(200),
                target: Some(9),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
        )
        .diagnostics;
        let pins: Vec<(u32, u32, String)> = diagnostics
            .iter()
            .filter_map(|diagnostic| {
                diagnostic.start.map(|start| {
                    (
                        diagnostic.code(),
                        start,
                        diagnostic.message_text().to_owned(),
                    )
                })
            })
            .collect();
        assert_eq!(
            pins,
            [(
                2846,
                15,
                "A declaration file cannot be imported without 'import type'. Did you mean to import an implementation file './foo.mjs' instead?".to_owned(),
            )]
        );
    }
}

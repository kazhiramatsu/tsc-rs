//! M4 5.8d: the module/alias/import-export band (m4-58 §8-§9) — the
//! resolveAlias protocol + per-kind alias targets, external-module
//! resolution (the 2307 band), module symbol resolution (export=
//! chase), and the module exports worker (export-star + the 2308
//! ambiguity row).
//!
//! Mode machinery (impliedNodeFormat / Node16..NodeNext arms /
//! resolution-mode overrides) reduces at the modeled defaults
//! throughout — each elision is noted at its arm. The program-layer
//! resolver (`resolve_program_module`) is the host.getResolvedModule
//! seam: tsrs-native over the in-memory file set per
//! program-and-modules.md §2 (no node_modules, no package.json).

use tsrs2_binder::{node_util, SymbolId, SymbolTable};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage};
use tsrs2_syntax::{
    escape_leading_underscores, unescape_leading_underscores, NodeData, NodeId, SyntaxKind,
};
use tsrs2_types::{CheckMode, InternalSymbolName, SymbolFlags};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, Unsupported};

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
    /// The usage-mode block (Node16..NodeNext ESM/CJS splits, redirect
    /// probing) is mode machinery — dead at the modeled defaults. The
    /// JS-file tail is JS-only. What remains: the computed
    /// allowSyntheticDefaultImports gate (TS6 default TRUE), the
    /// declaration-file syntactic-default/__esModule probe, and the
    /// TS-file hasExportAssignmentSymbol read.
    fn can_have_synthetic_default(
        &mut self,
        file_index: Option<usize>,
        module_symbol: SymbolId,
        dont_resolve_alias: bool,
    ) -> CheckResult2<bool> {
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
        // TS source files (isSourceFileJS is constant-false: JS
        // checking unmodeled).
        Ok(self.has_export_assignment_symbol(module_symbol))
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
    ///
    /// The Node20 CJS→ESM module.exports arm is mode machinery (dead
    /// at the modeled defaults); isOnlyImportableAsDefault is
    /// constant-false for the same reason (Node16+ JSON gate).
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
            self.resolve_export_by_name(
                module_symbol,
                InternalSymbolName::DEFAULT,
                Some(node),
                dont_resolve_alias,
            )?
        };
        let Some(_specifier) = specifier else {
            return Ok(export_default_symbol);
        };
        let has_default_only = false; // isOnlyImportableAsDefault: mode machinery
        let has_synthetic_default =
            self.can_have_synthetic_default(file_index, module_symbol, dont_resolve_alias)?;
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
    /// The require-argument arm is JS-only; the JSON named-import row
    /// is mode machinery (isOnlyImportableAsDefault constant-false).
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
            if self.can_have_synthetic_default(file_index, module_symbol, dont_resolve_alias)? {
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
        if symbol.is_none() {
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
    /// Reduced at the modeled defaults: the mode machinery
    /// (impliedNodeFormat, Node16..Node18 sync-import rows,
    /// resolution-mode overrides), project-reference redirects,
    /// resolveJsonModule, rewriteRelativeImportExtensions, external
    /// library (node_modules) resolution and its implicit-any rows,
    /// and the alternateResult chain all reduce to nothing — the
    /// program resolver never produces those shapes
    /// (program-and-modules.md §2). LIVE rows: @types/ redirect,
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
                        let suggestion = format!(
                            "{}{}",
                            module_reference
                                .strip_suffix(ts_extension)
                                .unwrap_or(module_reference),
                            Self::suggested_runtime_extension(ts_extension)
                        );
                        self.error_at(
                            Some(error_node),
                            &diagnostics::A_declaration_file_cannot_be_imported_without_import_type_Did_you_mean_to_import_an_implementation_file_0_instead,
                            &[&suggestion],
                        );
                    }
                }
            } else if resolved.resolved_using_ts_extension {
                // allowImportingTsExtensions is unmodeled-absent →
                // constant false; the row is LIVE for .ts/.tsx
                // specifiers.
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
                // patternAmbientModuleAugmentations: merge.rs owns the
                // augmentation map; unmerged here means the plain
                // pattern symbol (getMergedSymbol covers the merged
                // case).
                return Ok(Some(self.get_merged_symbol(symbol)));
            }
        }
        let Some(error_node) = error_node else {
            return Ok(None);
        };
        if matches!(resolution, ProgramModuleResolution::Suppressed) {
            // tsrs-native FP=0 rule: the miss sits behind unmodeled
            // resolution machinery (node_modules/baseUrl-paths/allowJs
            // targets) — tsc may resolve, so the 2307 tail stays
            // silent (FN-side; ledger).
            return Ok(None);
        }
        if let Some(module_not_found_error) = module_not_found_error {
            if is_for_augmentation {
                // The untyped-resolution face is dead (the resolver
                // only yields typed results); the NOT-FOUND face rides
                // the plain tail below in tsc too.
            }
            self.error_at(
                Some(error_node),
                module_not_found_error,
                &[module_reference],
            );
        }
        Ok(None)
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
                // A baseUrl miss is tsc-undecidable (paths/rootDirs).
                return ProgramModuleResolution::Suppressed;
            }
            // node_modules or a package.json among the HOST inputs
            // (incl. .js/.json the program dropped): tsc's
            // node_modules walk (package.json exports/@types/
            // typesVersions), self-name imports, and `#` package
            // imports might resolve this bare specifier — undecidable.
            if self
                .host_file_paths
                .iter()
                .any(|path| path.contains("/node_modules/") || path.ends_with("/package.json"))
            {
                return ProgramModuleResolution::Suppressed;
            }
            ProgramModuleResolution::Missed
        }
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
        // Directory index misses over the same unmodeled set.
        if ["/index.js", "/index.jsx", "/index.json"]
            .iter()
            .any(|suffix| {
                let probed = format!("{base}{suffix}");
                self.host_file_paths.contains(&probed)
            })
        {
            return true;
        }
        // Arbitrary-extension declaration twin (allowArbitraryExtensions
        // .d.<ext>.ts): "./file.html" may resolve to file.d.html.ts —
        // present twin makes the miss undecidable (tsc emits either
        // nothing or the unmodeled 6263 family).
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
        // Exact name with a recognized TS-family extension.
        for extension in TS_EXTENSIONS {
            if candidate.ends_with(extension) {
                if let Some(index) = lookup(candidate) {
                    return Some(make(index, true, candidate));
                }
            }
        }
        // .js-family substitution (non-Classic resolution modes).
        if !is_classic {
            for (js, subs) in [
                (".js", &[".ts", ".tsx", ".d.ts"][..]),
                (".jsx", &[".tsx", ".d.ts"][..]),
                (".mjs", &[".mts", ".d.mts"][..]),
                (".cjs", &[".cts", ".d.cts"][..]),
            ] {
                if let Some(base) = candidate.strip_suffix(js) {
                    for extension in subs {
                        let probed = format!("{base}{extension}");
                        if let Some(index) = lookup(&probed) {
                            return Some(make(index, false, &probed));
                        }
                    }
                }
            }
        }
        // Extension appends (extensionless specifiers probe the plain
        // TS family only — .mts/.cts require the explicit form).
        for extension in [".ts", ".tsx", ".d.ts"] {
            let probed = format!("{candidate}{extension}");
            if let Some(index) = lookup(&probed) {
                return Some(make(index, false, &probed));
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
        let joined = if path.starts_with('/') {
            path.to_owned()
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
    }

    fn try_extract_ts_extension(name: &str) -> Option<&'static str> {
        // tsc supportedTSExtensionsForExtractExtension order: the
        // declaration extension wins over the plain one.
        [".d.ts", ".ts", ".tsx"]
            .into_iter()
            .find(|extension| name.ends_with(extension))
    }

    /// getSuggestedImportSource reduced: non-Node ESM emit at the
    /// modeled defaults strips the extension for CJS-ish kinds and
    /// maps .ts-family → .js under ES module kinds.
    fn suggested_runtime_extension(ts_extension: &str) -> &'static str {
        match ts_extension {
            ".d.ts" | ".ts" | ".tsx" => ".js",
            _ => ".js",
        }
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
    ///
    /// The Node20 module.exports arm and the usage-mode probes are
    /// mode machinery (dead at the modeled defaults). The synthetic-
    /// default MODULE TYPE cloning (getTypeWithSyntheticDefaultOnly /
    /// getTypeWithSyntheticDefaultImportType + cloneTypeAsModuleType)
    /// ESCAPES for now — es_module_interop defaults TRUE in TS6, so a
    /// plain passthrough would mis-type `ns.default` reads (FP risk);
    /// containment keeps those fixtures FN until the cloning lands
    /// (m4-58 §9 gate note).
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
        let reference_parent = self.parent_of(referencing_location);
        let namespace_import = reference_parent.is_some_and(|parent| {
            self.kind_of(parent) == SyntaxKind::ImportDeclaration
                && self.get_namespace_declaration_node(parent).is_some()
        });
        let import_call = reference_parent.is_some_and(|parent| self.is_import_call(parent));
        if namespace_import || import_call {
            // The synthetic-default interop cloning: engage exactly
            // where tsc would build a module type (export= module with
            // callable/default-carrying type) and contain.
            let export_equals_present = module_symbol.is_some_and(|module_symbol| {
                self.binder
                    .symbol(module_symbol)
                    .exports
                    .contains_key(InternalSymbolName::EXPORT_EQUALS)
            });
            if self.options.es_module_interop_effective() && export_equals_present {
                return Err(Unsupported::new(
                    "resolveESModuleSymbol synthetic-default module type cloning (M4 5.8d tail)",
                ));
            }
        }
        Ok(Some(symbol))
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
    /// consumes (isTypeOnlyImportOrExportDeclaration). phaseModifier
    /// (import defer) parses onto is_type_only in ours.
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

    /// The SourceFile declaration index of a module symbol
    /// (`moduleSymbol.declarations?.find(isSourceFile)` shape).
    fn source_file_index_of_symbol(&self, symbol: SymbolId) -> Option<usize> {
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
}

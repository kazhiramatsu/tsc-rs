//! tsgo `ls/autoimport/extract.go`: the exports of a file — its module's
//! exports, the exports of its module augmentations and, in a script, of
//! its ambient modules — with what an alias resolves to.
//!
//! tsgo reads a symbol's binding without its checker where it can (a local
//! alias resolved by the binder's name resolver) and leases the checker
//! otherwise; whether the checker was leased decides which flags, type-only
//! state and symbol kind an export records ([`CheckerLease`]).

use std::sync::Arc;

use tsc_binder::node_util;
use tsc_checker::operators::OuterExpressionKinds;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{
    CheckFlags, EscapedName, InternalSymbolName, ModifierFlags, NodeFlags, SymbolFlags, SymbolId,
};

use super::export::{Export, ExportId, ExportSyntax, ModuleId};
use crate::lsutil::{get_symbol_kind, get_symbol_modifiers, module_specifier_to_valid_identifier};
use crate::references::{collect_external_module_references, statements};

/// tsgo `checkerLease`: whether an export's extraction used the checker.
#[derive(Default)]
struct CheckerLease {
    used: bool,
}

impl CheckerLease {
    /// tsgo `GetChecker`.
    fn get_checker(&mut self) {
        self.used = true;
    }

    /// tsgo `TryChecker() != nil`.
    fn has_checker(&self) -> bool {
        self.used
    }
}

/// tsgo `symbolExtractor` over a program's checker, for its project bucket.
pub(crate) struct Extractor<'c, 's> {
    pub(crate) checker: &'c mut CheckerState<'s>,
    /// tsgo `ToPath` of a file name.
    pub(crate) path_of: &'c dyn Fn(&str) -> String,
    pub(crate) package_name: String,
}

/// A symbol's name as tsgo writes it (unescaped).
fn name_of(checker: &CheckerState<'_>, symbol: SymbolId) -> String {
    checker
        .binder
        .symbol(symbol)
        .escaped_name
        .unescape()
        .to_string_lossy()
        .into_owned()
}

/// tsgo `Symbol.IsExternalModule`: a module symbol with a quoted name.
pub(crate) fn is_external_module(checker: &CheckerState<'_>, symbol: SymbolId) -> bool {
    let data = checker.binder.symbol(symbol);
    data.flags.intersects(SymbolFlags::MODULE) && data.escaped_name.starts_with("\"")
}

/// tsgo `IsModuleWithStringLiteralName`.
fn is_module_with_string_literal_name(source: &SourceFile, node: NodeId) -> bool {
    matches!(
        &source.arena.node(node).data,
        NodeData::ModuleDeclaration(data)
            if data.name.is_some_and(|name| source.arena.node(name).kind == SyntaxKind::StringLiteral)
    )
}

/// The text of a string literal or identifier name.
fn name_text(source: &SourceFile, node: NodeId) -> String {
    match &source.arena.node(node).data {
        NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
        NodeData::Identifier(data) => data.text().to_owned(),
        _ => String::new(),
    }
}

/// tsgo `tryGetModuleIDAndFileNameOfModuleSymbol`.
pub(crate) fn try_get_module_id_and_file_name_of_module_symbol(
    checker: &CheckerState<'_>,
    path_of: &dyn Fn(&str) -> String,
    symbol: SymbolId,
) -> Option<(ModuleId, String)> {
    if !is_external_module(checker, symbol) {
        return None;
    }
    // tsgo `GetNonAugmentationDeclaration`.
    let declaration = checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| {
            let source = checker.binder.source_of_node(declaration);
            !is_external_module_augmentation(source, declaration)
                && !node_util::is_global_scope_augmentation(source, declaration)
        })?;
    let source = checker.binder.source_of_node(declaration);
    if source.arena.node(declaration).kind == SyntaxKind::SourceFile {
        let file_name = source.file_name.to_string_lossy().into_owned();
        return Some((ModuleId::File(path_of(&file_name)), file_name));
    }
    if is_module_with_string_literal_name(source, declaration) {
        let NodeData::ModuleDeclaration(data) = &source.arena.node(declaration).data else {
            unreachable!("a module declaration");
        };
        let name = name_text(source, data.name.expect("a module name"));
        return Some((ModuleId::Ambient(name), String::new()));
    }
    None
}

/// tsgo `SymbolToExport`: the export of its module a symbol is (the
/// first export its extraction gives), if any.
pub(crate) fn symbol_to_export(
    checker: &mut CheckerState<'_>,
    path_of: &dyn Fn(&str) -> String,
    symbol: SymbolId,
) -> CheckResult<Option<Export>> {
    let parent = checker.binder.symbol(symbol).parent;
    if let Some(parent) = parent.filter(|&parent| is_external_module(checker, parent)) {
        let Some((module_id, module_file_name)) =
            try_get_module_id_and_file_name_of_module_symbol(checker, path_of, parent)
        else {
            return Ok(None);
        };
        // tsgo `GetSourceFileOfModule`.
        let record = checker.binder.symbol(parent);
        let declaration = record.value_declaration.or_else(|| {
            record.declarations.iter().copied().find(|&declaration| {
                let source = checker.binder.source_of_node(declaration);
                !is_external_module_augmentation(source, declaration)
                    && !node_util::is_global_scope_augmentation(source, declaration)
            })
        });
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        let file = checker.binder.file_index_of_node(declaration);
        let name = name_of(checker, symbol);
        return extract_first_export(
            checker,
            path_of,
            &name,
            symbol,
            module_id,
            module_file_name,
            file,
        );
    }
    let Some(&declaration) = checker.binder.symbol(symbol).declarations.first() else {
        return Ok(None);
    };
    let file = checker.binder.file_index_of_node(declaration);
    let source = checker.binder.source(file);
    let Some(file_symbol) = checker.binder.node_symbol(source.root) else {
        return Ok(None);
    };
    let module_symbol = checker.get_merged_symbol(file_symbol);
    let module_file_name = source.file_name.to_string_lossy().into_owned();
    let module_id = ModuleId::File(path_of(&module_file_name));
    let skipped = checker.skip_alias(symbol)?;
    let target = checker.get_merged_symbol(skipped);
    let name = name_of(checker, symbol);
    for export_name in [
        InternalSymbolName::DEFAULT,
        InternalSymbolName::EXPORT_EQUALS,
        name.as_str(),
    ] {
        // tsgo `tryGetModuleExport`.
        let Some(exported) =
            checker.try_get_member_in_module_exports_and_properties(export_name, module_symbol)?
        else {
            continue;
        };
        let skipped = checker.skip_alias(exported)?;
        if checker.get_merged_symbol(skipped) == target {
            let exported_name = name_of(checker, exported);
            return extract_first_export(
                checker,
                path_of,
                &exported_name,
                exported,
                module_id,
                module_file_name,
                file,
            );
        }
    }
    Ok(None)
}

/// tsgo `extractFirstExport`.
fn extract_first_export(
    checker: &mut CheckerState<'_>,
    path_of: &dyn Fn(&str) -> String,
    name: &str,
    symbol: SymbolId,
    module_id: ModuleId,
    module_file_name: String,
    file: usize,
) -> CheckResult<Option<Export>> {
    let mut exports = Vec::new();
    let mut extractor = Extractor {
        checker,
        path_of,
        package_name: String::new(),
    };
    extractor.extract_from_symbol(
        name,
        symbol,
        &module_id,
        &module_file_name,
        file,
        &mut exports,
    )?;
    Ok(exports.into_iter().next())
}

/// tsgo `IsExternalModuleAugmentation`: a module augmentation in an
/// external module (a string-named module declaration at its top level).
fn is_external_module_augmentation(source: &SourceFile, node: NodeId) -> bool {
    if !is_module_with_string_literal_name(source, node) {
        return false;
    }
    let Some(parent) = source.arena.node(node).parent else {
        return false;
    };
    match source.arena.node(parent).kind {
        SyntaxKind::SourceFile => source.external_module_indicator.is_some(),
        SyntaxKind::ModuleBlock => source.arena.node(parent).parent.is_some_and(|module| {
            is_ambient_module(source, module)
                && source
                    .arena
                    .node(module)
                    .parent
                    .is_some_and(|file| source.arena.node(file).kind == SyntaxKind::SourceFile)
                && source.external_module_indicator.is_none()
        }),
        _ => false,
    }
}

/// tsgo `IsAmbientModule`.
fn is_ambient_module(source: &SourceFile, node: NodeId) -> bool {
    is_module_with_string_literal_name(source, node)
        || node_util::is_global_scope_augmentation(source, node)
}

impl Extractor<'_, '_> {
    /// tsgo `extractFromFile`.
    pub(crate) fn extract_from_file(&mut self, file: usize) -> CheckResult<Vec<Export>> {
        let source = self.checker.binder.source(file);
        if self.checker.binder.node_symbol(source.root).is_some() {
            return self.extract_from_module(file);
        }
        let references = collect_external_module_references(source);
        if references.ambient_module_names.is_empty() {
            return Ok(Vec::new());
        }
        let mut exports = Vec::new();
        for &statement in statements(source, source.root) {
            if is_module_with_string_literal_name(source, statement)
                && self.is_non_pattern_ambient_module_declaration(file, statement)
            {
                let NodeData::ModuleDeclaration(data) = &source.arena.node(statement).data else {
                    continue;
                };
                let name = name_text(source, data.name.expect("a module name"));
                self.extract_from_module_declaration(
                    statement,
                    file,
                    ModuleId::Ambient(name),
                    String::new(),
                    &mut exports,
                )?;
            }
        }
        Ok(exports)
    }

    /// tsgo `isNonPatternAmbientModuleDeclaration`.
    fn is_non_pattern_ambient_module_declaration(&self, file: usize, declaration: NodeId) -> bool {
        let Some(symbol) = self.checker.binder.node_symbol(declaration) else {
            return true;
        };
        !self
            .checker
            .binder
            .file(file)
            .pattern_ambient_modules
            .iter()
            .any(|&(_, _, pattern)| pattern == symbol)
    }

    /// tsgo `extractFromModule`.
    fn extract_from_module(&mut self, file: usize) -> CheckResult<Vec<Export>> {
        let source = self.checker.binder.source(file);
        let file_name = source.file_name.to_string_lossy().into_owned();
        let module_id = ModuleId::File((self.path_of)(&file_name));
        let file_symbol = self
            .checker
            .binder
            .node_symbol(source.root)
            .expect("a module's symbol");
        let mut exports = Vec::new();
        let table = Arc::clone(self.checker.binder.symbol(file_symbol).exports());
        for (name, &symbol) in table.iter() {
            let name = name.unescape().to_string_lossy().into_owned();
            self.extract_from_symbol(&name, symbol, &module_id, &file_name, file, &mut exports)?;
        }
        let augmentations: Vec<NodeId> = collect_external_module_references(source)
            .module_augmentations
            .iter()
            .filter_map(|&name| source.arena.node(name).parent)
            .filter(|&declaration| !node_util::is_global_scope_augmentation(source, declaration))
            .collect();
        for declaration in augmentations {
            let NodeData::ModuleDeclaration(data) = &source.arena.node(declaration).data else {
                continue;
            };
            let name = name_text(source, data.name.expect("a module name"));
            let mut module_id = ModuleId::Ambient(name.clone());
            let mut module_file_name = String::new();
            let mut unresolved_module_specifier = String::new();
            if crate::references::is_external_module_name_relative(&tsc_types::JsString::from(
                name.as_str(),
            )) {
                // tsgo resolves the name from the file; the program's
                // resolution of the augmentation is the same file.
                let resolved = data
                    .name
                    .map(|name| self.checker.resolve_external_module_name_of(name))
                    .transpose()?
                    .flatten()
                    .and_then(|module| {
                        try_get_module_id_and_file_name_of_module_symbol(
                            self.checker,
                            self.path_of,
                            module,
                        )
                    })
                    .filter(|(_, file_name): &(ModuleId, String)| !file_name.is_empty());
                match resolved {
                    Some((resolved_id, resolved_file_name)) => {
                        module_id = resolved_id;
                        module_file_name = resolved_file_name;
                    }
                    None => {
                        module_file_name = tsc_program::combine_paths(
                            tsc_program::get_directory_path(tsc_types::JsStr::from_str(&file_name))
                                .as_js(),
                            tsc_types::JsStr::from_str(&name),
                        )
                        .to_string_lossy()
                        .into_owned();
                        module_id = ModuleId::File((self.path_of)(&module_file_name));
                        unresolved_module_specifier = name.clone();
                    }
                }
            }
            let start = exports.len();
            self.extract_from_module_declaration(
                declaration,
                file,
                module_id,
                module_file_name,
                &mut exports,
            )?;
            for export in &mut exports[start..] {
                export.unresolved_module_specifier = unresolved_module_specifier.clone();
            }
        }
        Ok(exports)
    }

    /// tsgo `extractFromModuleDeclaration`.
    fn extract_from_module_declaration(
        &mut self,
        declaration: NodeId,
        file: usize,
        module_id: ModuleId,
        module_file_name: String,
        exports: &mut Vec<Export>,
    ) -> CheckResult<()> {
        let Some(symbol) = self.checker.binder.node_symbol(declaration) else {
            return Ok(());
        };
        let table = Arc::clone(self.checker.binder.symbol(symbol).exports());
        for (name, &member) in table.iter() {
            let name = name.unescape().to_string_lossy().into_owned();
            self.extract_from_symbol(&name, member, &module_id, &module_file_name, file, exports)?;
        }
        Ok(())
    }

    /// tsgo `extractFromSymbol`.
    pub(crate) fn extract_from_symbol(
        &mut self,
        name: &str,
        symbol: SymbolId,
        module_id: &ModuleId,
        module_file_name: &str,
        file: usize,
        exports: &mut Vec<Export>,
    ) -> CheckResult<()> {
        if should_ignore_symbol(self.checker, symbol) {
            return Ok(());
        }
        if name == InternalSymbolName::EXPORT_STAR {
            let mut lease = CheckerLease::default();
            let parent = self
                .checker
                .binder
                .symbol(symbol)
                .parent
                .expect("an export star's module");
            let mut all_exports = self.checker.get_exports_of_module_symbols(parent)?;
            // allExports includes named exports from the file that will be processed separately;
            // we want to add only the ones that come from the star
            let table = Arc::clone(self.checker.binder.symbol(parent).exports());
            for (named, &named_export) in table.iter() {
                if named.as_js() != InternalSymbolName::EXPORT_STAR {
                    if let Some(index) = all_exports.iter().position(|&known| known == named_export)
                    {
                        all_exports.remove(index);
                    }
                }
            }
            for reexported in all_exports {
                let (export, _) = self.create_export(
                    reexported,
                    module_id,
                    module_file_name,
                    ExportSyntax::Star,
                    file,
                    &mut lease,
                )?;
                let Some(mut export) = export else {
                    continue;
                };
                lease.get_checker();
                let parent = self
                    .checker
                    .binder
                    .symbol(reexported)
                    .parent
                    .map(|parent| self.checker.get_merged_symbol(parent));
                if let Some(parent) =
                    parent.filter(|&parent| is_external_module(self.checker, parent))
                {
                    if let Some((target_module_id, _)) =
                        try_get_module_id_and_file_name_of_module_symbol(
                            self.checker,
                            self.path_of,
                            parent,
                        )
                    {
                        export.target = Some(ExportId {
                            export_name: name_of(self.checker, reexported),
                            module_id: target_module_id,
                        });
                    }
                }
                export.through = InternalSymbolName::EXPORT_STAR.to_owned();
                exports.push(export);
            }
            return Ok(());
        }

        let syntax = get_syntax(self.checker, symbol);
        let mut lease = CheckerLease::default();
        let (export, target) = self.create_export(
            symbol,
            module_id,
            module_file_name,
            syntax,
            file,
            &mut lease,
        )?;
        let Some(export) = export else {
            return Ok(());
        };
        exports.push(export);

        if let Some(target) = target {
            if syntax == ExportSyntax::Equals
                && self
                    .checker
                    .binder
                    .symbol(target)
                    .flags
                    .intersects(SymbolFlags::NAMESPACE)
            {
                let table = Arc::clone(self.checker.binder.symbol(target).exports());
                for (inner_name, &named_export) in table.iter() {
                    if inner_name.as_js() != InternalSymbolName::EXPORT_STAR {
                        let (export, _) = self.create_export(
                            named_export,
                            module_id,
                            module_file_name,
                            syntax,
                            file,
                            &mut lease,
                        )?;
                        if let Some(mut export) = export {
                            export.through = name.to_owned();
                            exports.push(export);
                        }
                    }
                }
            }
        } else if syntax == ExportSyntax::CommonJsModuleExports {
            let declaration = self.checker.binder.symbol(symbol).declarations[0];
            let source = self.checker.binder.source_of_node(declaration);
            let NodeData::BinaryExpression(binary) = &source.arena.node(declaration).data else {
                return Ok(());
            };
            let Some(expression) = binary.right else {
                return Ok(());
            };
            let NodeData::ObjectLiteralExpression(object) = &source.arena.node(expression).data
            else {
                return Ok(());
            };
            let properties: Vec<NodeId> = object
                .properties
                .map(|list| source.arena.node_array(list).nodes.to_vec())
                .unwrap_or_default();
            // what is actually desirable here? I think it would be reasonable to only treat these as exports
            // if *every* property is a shorthand property or identifier: identifier
            // At least, it would be sketchy if there were any methods, computed properties...
            let Some(object_symbol) = self.checker.binder.node_symbol(expression) else {
                return Ok(());
            };
            let members = Arc::clone(self.checker.binder.symbol(object_symbol).members());
            for property in properties {
                let name_node = match &source.arena.node(property).data {
                    NodeData::ShorthandPropertyAssignment(data) => data.name,
                    NodeData::PropertyAssignment(data)
                        if data.name.is_some_and(|name| {
                            source.arena.node(name).kind == SyntaxKind::Identifier
                        }) =>
                    {
                        data.name
                    }
                    _ => continue,
                };
                let Some(name_node) = name_node else {
                    continue;
                };
                let property_name = name_text(source, name_node);
                let Some(&member) = members.get(EscapedName::escape(property_name.as_str().into()))
                else {
                    continue;
                };
                let (export, _) = self.create_export(
                    member,
                    module_id,
                    module_file_name,
                    syntax,
                    file,
                    &mut lease,
                )?;
                if let Some(mut export) = export {
                    export.through = name.to_owned();
                    exports.push(export);
                }
            }
        }
        Ok(())
    }

    /// tsgo `createExport`: the export and, for an alias, its target.
    fn create_export(
        &mut self,
        symbol: SymbolId,
        module_id: &ModuleId,
        module_file_name: &str,
        syntax: ExportSyntax,
        file: usize,
        lease: &mut CheckerLease,
    ) -> CheckResult<(Option<Export>, Option<SymbolId>)> {
        if should_ignore_symbol(self.checker, symbol) {
            return Ok((None, None));
        }
        let source = self.checker.binder.source(file);
        let file_name = source.file_name.to_string_lossy().into_owned();
        let symbol_name = name_of(self.checker, symbol);
        let mut export = Export {
            id: ExportId {
                module_id: module_id.clone(),
                export_name: symbol_name.clone(),
            },
            module_file_name: module_file_name.to_owned(),
            unresolved_module_specifier: String::new(),
            syntax,
            flags: combined_local_and_export_symbol_flags(self.checker, symbol),
            local_name: String::new(),
            through: String::new(),
            target: None,
            is_type_only: false,
            script_element_kind: crate::lsutil::ScriptElementKind::Unknown,
            script_element_kind_modifiers: crate::lsutil::ScriptElementKindModifier::NONE,
            path: (self.path_of)(&file_name),
            package_name: self.package_name.clone(),
        };
        if syntax == ExportSyntax::Umd {
            export.id.export_name = InternalSymbolName::EXPORT_EQUALS.to_owned();
            export.local_name = symbol_name.clone();
        }

        let mut target_symbol = None;
        if self
            .checker
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            target_symbol = self.try_resolve_symbol(symbol, syntax, lease)?;
            if let Some(target) = target_symbol {
                let mut declaration = self
                    .checker
                    .binder
                    .symbol(target)
                    .declarations
                    .first()
                    .copied();
                if declaration.is_none()
                    && self
                        .checker
                        .get_check_flags(target)
                        .intersects(CheckFlags::MAPPED)
                {
                    lease.get_checker();
                    declaration = self
                        .checker
                        .get_mapped_type_symbol_of_property(target)
                        .and_then(|mapped| {
                            self.checker
                                .binder
                                .symbol(mapped)
                                .declarations
                                .first()
                                .copied()
                        });
                }
                // !!! consider GetImmediateAliasedSymbol to go as far as we can
                let declaration = declaration
                    .or_else(|| {
                        self.checker
                            .binder
                            .symbol(symbol)
                            .declarations
                            .first()
                            .copied()
                    })
                    .expect("no declaration for aliased symbol");
                let mut parent = self.checker.binder.symbol(target).parent;
                if lease.has_checker() {
                    export.flags = self.checker.get_symbol_flags_of(target)?;
                    export.is_type_only = self
                        .checker
                        .get_type_only_alias_declaration(symbol)?
                        .is_some();
                    parent = parent.map(|parent| self.checker.get_merged_symbol(parent));
                } else {
                    export.flags = self.checker.binder.symbol(target).flags;
                    export.is_type_only = self
                        .checker
                        .binder
                        .symbol(symbol)
                        .declarations
                        .clone()
                        .into_iter()
                        .any(|declaration| {
                            self.checker
                                .is_part_of_type_only_import_or_export_declaration(declaration)
                        });
                }
                let with_checker = lease.has_checker();
                export.script_element_kind =
                    get_symbol_kind(self.checker, target, declaration, with_checker)?;
                export.script_element_kind_modifiers =
                    get_symbol_modifiers(self.checker, target, with_checker)?;
                let declaration_file = self
                    .checker
                    .binder
                    .source_of_node(declaration)
                    .file_name
                    .to_string_lossy()
                    .into_owned();
                let mut target_module_id = ModuleId::File((self.path_of)(&declaration_file));
                if let Some(parent) =
                    parent.filter(|&parent| is_external_module(self.checker, parent))
                {
                    if let Some((id, _)) = try_get_module_id_and_file_name_of_module_symbol(
                        self.checker,
                        self.path_of,
                        parent,
                    ) {
                        target_module_id = id;
                    }
                }
                export.target = Some(ExportId {
                    export_name: name_of(self.checker, target),
                    module_id: target_module_id,
                });
            }
        } else {
            let declaration = self.checker.binder.symbol(symbol).declarations[0];
            let with_checker = lease.has_checker();
            export.script_element_kind =
                get_symbol_kind(self.checker, symbol, declaration, with_checker)?;
            export.script_element_kind_modifiers =
                get_symbol_modifiers(self.checker, symbol, with_checker)?;
        }

        if symbol_name == InternalSymbolName::DEFAULT
            || symbol_name == InternalSymbolName::EXPORT_EQUALS
        {
            let named_symbol = self
                .checker
                .local_symbol_for_export_default(symbol)
                .unwrap_or(symbol);
            export.local_name =
                get_default_like_export_name_from_declaration(self.checker, named_symbol);
            if is_unusable_name(&export.local_name) {
                export.local_name = export
                    .target
                    .as_ref()
                    .map_or_else(String::new, |target| target.export_name.clone());
            }
            if is_unusable_name(&export.local_name) {
                if let Some(target) = target_symbol {
                    let named_symbol = self
                        .checker
                        .local_symbol_for_export_default(target)
                        .unwrap_or(target);
                    export.local_name =
                        get_default_like_export_name_from_declaration(self.checker, named_symbol);
                }
            }
            if is_unusable_name(&export.local_name) {
                // Last resort: derive identifier from the file name. Use FileName() (original
                // casing) rather than ModuleID/Path() which is lowercased on case-insensitive
                // file systems, losing PascalCase.
                export.local_name = module_specifier_to_valid_identifier(
                    &file_name_for_default_export_name(
                        self.checker,
                        target_symbol,
                        module_file_name,
                        module_id,
                    ),
                    false,
                );
            }
        }

        if is_unusable_name(export.name()) {
            return Ok((None, None));
        }
        Ok((Some(export), target_symbol))
    }

    /// tsgo `tryResolveSymbol`: an alias's target, from the binder's name
    /// resolution of a local name where it can, else from the checker.
    fn try_resolve_symbol(
        &mut self,
        symbol: SymbolId,
        syntax: ExportSyntax,
        lease: &mut CheckerLease,
    ) -> CheckResult<Option<SymbolId>> {
        if !is_non_local_alias(self.checker, symbol) {
            return Ok(Some(symbol));
        }
        let mut location = None;
        match syntax {
            ExportSyntax::Named => {
                let declaration =
                    declaration_of_kind(self.checker, symbol, SyntaxKind::ExportSpecifier);
                if let Some(declaration) = declaration {
                    let source = self.checker.binder.source_of_node(declaration);
                    let has_module_specifier = source
                        .arena
                        .node(declaration)
                        .parent
                        .and_then(|exports| source.arena.node(exports).parent)
                        .is_some_and(|export_declaration| {
                            matches!(
                                &source.arena.node(export_declaration).data,
                                NodeData::ExportDeclaration(data) if data.module_specifier.is_some()
                            )
                        });
                    if !has_module_specifier {
                        if let NodeData::ExportSpecifier(data) =
                            &source.arena.node(declaration).data
                        {
                            if let Some(name) = data.name.or(data.property_name) {
                                if source.arena.node(name).kind == SyntaxKind::Identifier {
                                    location = Some((name, name_text(source, name)));
                                }
                            }
                        }
                    }
                }
            }
            // !!! check if module.exports = foo is marked as an alias
            ExportSyntax::Equals | ExportSyntax::DefaultDeclaration
                if syntax == ExportSyntax::DefaultDeclaration
                    || name_of(self.checker, symbol) == InternalSymbolName::EXPORT_EQUALS =>
            {
                if let Some(declaration) =
                    declaration_of_kind(self.checker, symbol, SyntaxKind::ExportAssignment)
                {
                    let source = self.checker.binder.source_of_node(declaration);
                    if let NodeData::ExportAssignment(data) = &source.arena.node(declaration).data {
                        if let Some(expression) = data.expression {
                            if source.arena.node(expression).kind == SyntaxKind::Identifier {
                                location = Some((expression, name_text(source, expression)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some((location, name)) = location {
            if let Some(local) = resolve_local_name(self.checker, location, &name) {
                if !is_non_local_alias(self.checker, local) {
                    return Ok(Some(local));
                }
            }
        }
        lease.get_checker();
        let resolved = self.checker.get_aliased_symbol(symbol)?;
        Ok((!self.checker.is_unknown_symbol(resolved)).then_some(resolved))
    }
}

/// tsgo `IsNonLocalAlias(symbol, SymbolFlagsNone)`: an alias.
fn is_non_local_alias(checker: &CheckerState<'_>, symbol: SymbolId) -> bool {
    checker.binder.symbol(symbol).flags & SymbolFlags::ALIAS == SymbolFlags::ALIAS
}

/// tsgo `shouldIgnoreSymbol`.
fn should_ignore_symbol(checker: &CheckerState<'_>, symbol: SymbolId) -> bool {
    checker
        .binder
        .symbol(symbol)
        .flags
        .intersects(SymbolFlags::PROTOTYPE)
}

/// tsgo `Symbol.CombinedLocalAndExportSymbolFlags`.
fn combined_local_and_export_symbol_flags(
    checker: &CheckerState<'_>,
    symbol: SymbolId,
) -> SymbolFlags {
    let data = checker.binder.symbol(symbol);
    match data.export_symbol {
        Some(export) => data.flags | checker.binder.symbol(export).flags,
        None => data.flags,
    }
}

/// tsgo `GetDeclarationOfKind`.
fn declaration_of_kind(
    checker: &CheckerState<'_>,
    symbol: SymbolId,
    kind: SyntaxKind,
) -> Option<NodeId> {
    checker
        .binder
        .symbol(symbol)
        .declarations
        .iter()
        .copied()
        .find(|&declaration| {
            checker
                .binder
                .source_of_node(declaration)
                .arena
                .node(declaration)
                .kind
                == kind
        })
}

/// tsgo `getSyntax`: how the symbol's first declaration exports it.
fn get_syntax(checker: &CheckerState<'_>, symbol: SymbolId) -> ExportSyntax {
    for &declaration in checker.binder.symbol(symbol).declarations.iter() {
        let source = checker.binder.source_of_node(declaration);
        match &source.arena.node(declaration).data {
            NodeData::ExportSpecifier(_) => return ExportSyntax::Named,
            NodeData::ExportAssignment(data) => {
                return if data.is_export_equals == Some(true) {
                    ExportSyntax::Equals
                } else {
                    ExportSyntax::DefaultDeclaration
                };
            }
            NodeData::NamespaceExportDeclaration(_) => return ExportSyntax::Umd,
            NodeData::BinaryExpression(_) => {
                match tsc_binder::get_assignment_declaration_kind(source, declaration) {
                    tsc_binder::AssignmentDeclarationKind::ModuleExports => {
                        return ExportSyntax::CommonJsModuleExports;
                    }
                    tsc_binder::AssignmentDeclarationKind::ExportsProperty => {
                        return ExportSyntax::CommonJsExportsProperty;
                    }
                    _ => {}
                }
            }
            _ => {
                return if node_util::get_combined_modifier_flags(source, declaration)
                    .intersects(ModifierFlags::DEFAULT)
                {
                    ExportSyntax::DefaultModifier
                } else {
                    ExportSyntax::Modifier
                };
            }
        }
    }
    ExportSyntax::None
}

/// tsgo `isUnusableName`.
fn is_unusable_name(name: &str) -> bool {
    name.is_empty()
        || name == "_default"
        || name == InternalSymbolName::EXPORT_STAR
        || name == InternalSymbolName::DEFAULT
        || name == InternalSymbolName::EXPORT_EQUALS
}

/// tsgo `getDefaultLikeExportNameFromDeclaration`.
fn get_default_like_export_name_from_declaration(
    checker: &CheckerState<'_>,
    symbol: SymbolId,
) -> String {
    for &declaration in checker.binder.symbol(symbol).declarations.iter() {
        let source = checker.binder.source_of_node(declaration);
        match &source.arena.node(declaration).data {
            // "export default" in this case. See `ExportAssignment`for more details.
            NodeData::ExportAssignment(data) => {
                if let Some(expression) = data.expression {
                    let inner =
                        checker.skip_outer_expressions(expression, OuterExpressionKinds::ALL);
                    if source.arena.node(inner).kind == SyntaxKind::Identifier {
                        return name_text(source, inner);
                    }
                }
                continue;
            }
            // "export { ~ as default }"
            NodeData::ExportSpecifier(data)
                if checker
                    .binder
                    .node_symbol(declaration)
                    .is_some_and(|specifier| {
                        checker.binder.symbol(specifier).flags == SymbolFlags::ALIAS
                    })
                    && data.property_name.is_some() =>
            {
                let property_name = data.property_name.expect("a property name");
                if source.arena.node(property_name).kind == SyntaxKind::Identifier {
                    return name_text(source, property_name);
                }
                continue;
            }
            _ => {}
        }
        // GH#52694
        if let Some(name) = node_util::get_name_of_declaration(source, declaration) {
            if source.arena.node(name).kind == SyntaxKind::Identifier {
                return name_text(source, name);
            }
        }
        if let Some(parent) = checker.binder.symbol(symbol).parent {
            if !is_external_module(checker, parent) {
                return name_of(checker, parent);
            }
        }
    }
    String::new()
}

/// tsgo `fileNameForDefaultExportName`.
fn file_name_for_default_export_name(
    checker: &CheckerState<'_>,
    target: Option<SymbolId>,
    module_file_name: &str,
    module_id: &ModuleId,
) -> String {
    if let Some(&declaration) =
        target.and_then(|target| checker.binder.symbol(target).declarations.first())
    {
        let file_name = checker
            .binder
            .source_of_node(declaration)
            .file_name
            .to_string_lossy();
        if !file_name.is_empty() {
            return file_name.into_owned();
        }
    }
    if !module_file_name.is_empty() {
        return module_file_name.to_owned();
    }
    module_id.as_str().to_owned()
}

/// tsgo binder `NameResolver.Resolve(loc, name, SymbolFlagsAll, nil, false,
/// false)` without the checker: the locals of the scopes around the
/// location and a module's exports, no globals.
fn resolve_local_name(
    checker: &CheckerState<'_>,
    location: NodeId,
    name: &str,
) -> Option<SymbolId> {
    let source = checker.binder.source_of_node(location);
    let escaped = EscapedName::escape(name.into());
    let lookup = |table: &tsc_binder::SymbolTable, meaning: SymbolFlags| {
        table
            .get(escaped)
            .copied()
            .filter(|&symbol| checker.binder.symbol(symbol).flags.intersects(meaning))
    };
    let mut current = Some(location);
    let mut last: Option<NodeId> = None;
    while let Some(node) = current {
        let record = source.arena.node(node);
        // Locals of a source file are not in scope (because they get merged into the global symbol table)
        let global_source_file = record.kind == SyntaxKind::SourceFile
            && !checker.binder.is_external_or_common_js_module_of_node(node);
        if !global_source_file {
            if let Some(locals) = checker.binder.locals_of(node) {
                if let Some(result) = lookup(locals, SymbolFlags::ALL) {
                    return Some(result);
                }
            }
        }
        let module_like = match record.kind {
            SyntaxKind::SourceFile => checker.binder.is_external_or_common_js_module_of_node(node),
            SyntaxKind::ModuleDeclaration => true,
            _ => false,
        };
        if module_like {
            if let Some(module_symbol) = checker.binder.node_symbol(node) {
                let module_exports = Arc::clone(checker.binder.symbol(module_symbol).exports());
                let external = record.kind == SyntaxKind::SourceFile
                    || (record.kind == SyntaxKind::ModuleDeclaration
                        && NodeFlags::from_bits(record.flags).intersects(NodeFlags::AMBIENT)
                        && !node_util::is_global_scope_augmentation(source, node));
                let mut skip_exports = false;
                if external {
                    // It's an external module. First see if the module has an export default and if the local
                    // name of that export default matches.
                    if let Some(&default) =
                        module_exports.get(EscapedName::escape(InternalSymbolName::DEFAULT.into()))
                    {
                        if let Some(local) = checker.local_symbol_for_export_default(default) {
                            if name_of(checker, local) == name {
                                return Some(default);
                            }
                        }
                    }
                    // A module's export specifier does not put a member in scope.
                    if let Some(&module_export) = module_exports.get(escaped) {
                        if checker.binder.symbol(module_export).flags == SymbolFlags::ALIAS
                            && (declaration_of_kind(
                                checker,
                                module_export,
                                SyntaxKind::ExportSpecifier,
                            )
                            .is_some()
                                || declaration_of_kind(
                                    checker,
                                    module_export,
                                    SyntaxKind::NamespaceExport,
                                )
                                .is_some())
                        {
                            skip_exports = true;
                        }
                    }
                }
                if !skip_exports && name != InternalSymbolName::DEFAULT {
                    if let Some(result) = lookup(&module_exports, SymbolFlags::MODULE_MEMBER) {
                        let common_js = record.kind == SyntaxKind::SourceFile
                            && checker.binder.is_common_js_module_of_node(node);
                        if !common_js
                            || checker
                                .binder
                                .symbol(result)
                                .flags
                                .intersects(SymbolFlags::TYPE)
                        {
                            return Some(result);
                        }
                    }
                }
            }
        }
        if let NodeData::ExportSpecifier(data) = &record.data {
            if last.is_some() && last == data.property_name {
                let declaration = record
                    .parent
                    .and_then(|exports| source.arena.node(exports).parent);
                if declaration.is_some_and(|declaration| {
                    matches!(&source.arena.node(declaration).data, NodeData::ExportDeclaration(data) if data.module_specifier.is_some())
                }) {
                    last = declaration;
                    current = declaration.and_then(|declaration| source.arena.node(declaration).parent);
                    continue;
                }
            }
        }
        last = Some(node);
        current = record.parent;
    }
    None
}

//! tsgo `ls/autoimport/import_adder.go`: fixes gathered over a file and made
//! at once, the imports of a module coalesced into one declaration.

use std::collections::BTreeMap;

use std::collections::HashMap;
use std::sync::Arc;
use tsc_lsproto::{AddAsTypeOnly, AutoImportFix, AutoImportFixKind, ImportKind, TextEdit};

use tsc_checker::state::{CheckResult, CheckerState};
use tsc_emitter::{TransformArena, TransformError, TransformNode, TransformSourceId};
use tsc_syntax::{NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SourceFile, SyntaxKind};
use tsc_types::{CompilerOptions, InternalSymbolName, SymbolId, TypeId};

use super::edits::{
    add_import_type, add_namespace_qualifier, add_to_existing_import, file_edits,
    get_add_to_existing_import_fix, get_new_imports, get_new_requires, insert_imports,
    NewImportBinding,
};
use super::export::Export;
use super::view::{Fix, View};
use crate::change::Tracker;
use crate::lsconv::Converters;
use crate::lsutil::{get_quote_preference, new_line_kind, FormatCodeSettings, UserPreferences};
use crate::references::collect_external_module_references;

/// tsgo `addToExistingState`: the bindings to add to an import clause or
/// binding pattern.
#[derive(Default)]
struct AddToExistingState {
    default_import: Option<NewImportBinding>,
    /// By name (tsgo `sortedNamedImports` takes them in name order).
    named_imports: BTreeMap<String, NewImportBinding>,
}

/// tsgo `importsCollection`: a new import declaration's bindings.
#[derive(Default)]
struct ImportsCollection {
    default_import: Option<NewImportBinding>,
    named_imports: BTreeMap<String, NewImportBinding>,
    namespace_like_import: Option<NewImportBinding>,
    use_require: bool,
}

/// tsgo `importAdder`.
pub(crate) struct ImportAdder<'a> {
    file: &'a SourceFile,
    options: &'a CompilerOptions,
    format_settings: FormatCodeSettings,
    converters: &'a Converters,
    preferences: &'a UserPreferences,
    /// Namespace fixes don't conflict, so just build a list
    add_to_namespace: Vec<AutoImportFix>,
    /// JSDoc type import fixes
    import_type: Vec<AutoImportFix>,
    /// importClauseOrBindingPattern -> default or named bindings
    add_to_existing: Vec<(NodeId, AddToExistingState)>,
    /// module specifier + type only -> imports
    new_imports: BTreeMap<String, ImportsCollection>,
}

impl<'a> ImportAdder<'a> {
    /// tsgo `NewImportAdder`.
    pub(crate) fn new(
        file: &'a SourceFile,
        options: &'a CompilerOptions,
        format_settings: FormatCodeSettings,
        converters: &'a Converters,
        preferences: &'a UserPreferences,
    ) -> Self {
        Self {
            file,
            options,
            format_settings,
            converters,
            preferences,
            add_to_namespace: Vec::new(),
            import_type: Vec::new(),
            add_to_existing: Vec::new(),
            new_imports: BTreeMap::new(),
        }
    }

    /// tsgo `HasFixes`.
    pub(crate) fn has_fixes(&self) -> bool {
        !self.add_to_namespace.is_empty()
            || !self.import_type.is_empty()
            || !self.add_to_existing.is_empty()
            || !self.new_imports.is_empty()
    }

    /// tsgo `Edits`: the edits of every fix added.
    pub(crate) fn edits(&self) -> Result<Vec<TextEdit>, String> {
        let file = self.file;
        let mut tracker = Tracker::new(new_line_kind(self.options), self.format_settings.clone());
        let to_error = |error: tsc_emitter::TransformError| error.to_string();
        let quote_preference = get_quote_preference(file, self.preferences);
        for fix in &self.add_to_namespace {
            add_namespace_qualifier(fix, &mut tracker, file, self.converters, None);
        }
        for fix in &self.import_type {
            add_import_type(
                fix,
                file,
                self.preferences,
                &mut tracker,
                self.converters,
                None,
            );
        }
        for (clause_or_pattern, entry) in &self.add_to_existing {
            let named_imports: Vec<NewImportBinding> =
                entry.named_imports.values().cloned().collect();
            add_to_existing_import(
                &mut tracker,
                file,
                *clause_or_pattern,
                entry.default_import.as_ref(),
                &named_imports,
                self.preferences,
            )
            .map_err(to_error)?;
        }
        let mut new_declarations = Vec::new();
        for (key, new_import) in &self.new_imports {
            let module_specifier = &key[2..]; // From `${0 | 1}|${moduleSpecifier}` format
            let named_imports: Vec<NewImportBinding> =
                new_import.named_imports.values().cloned().collect();
            let declarations = if new_import.use_require {
                get_new_requires(
                    &mut tracker,
                    file,
                    module_specifier,
                    quote_preference,
                    new_import.default_import.as_ref(),
                    &named_imports,
                    new_import.namespace_like_import.as_ref(),
                )
            } else {
                get_new_imports(
                    &mut tracker,
                    file,
                    module_specifier,
                    quote_preference,
                    new_import.default_import.as_ref(),
                    &named_imports,
                    new_import.namespace_like_import.as_ref(),
                    self.options,
                    self.preferences,
                )
            }
            .map_err(to_error)?;
            new_declarations.extend(declarations);
        }
        if !new_declarations.is_empty() {
            insert_imports(&mut tracker, file, new_declarations, true, self.preferences)
                .map_err(to_error)?;
        }
        file_edits(tracker, file, self.converters)
    }

    /// tsgo `AddImportFromExportedSymbol`: the best fix importing a symbol's
    /// export, none when the registry has no export of it (its file left
    /// out by `autoImportFileExcludePatterns`).
    pub(crate) fn add_import_from_exported_symbol(
        &mut self,
        checker: &mut CheckerState<'_>,
        view: &mut View<'_>,
        exported_symbol: SymbolId,
        is_valid_type_only_use_site: bool,
    ) -> CheckResult<()> {
        let skipped = checker.skip_alias(exported_symbol)?;
        let symbol = checker.get_merged_symbol(skipped);
        let exports = get_all_exports_for_symbol(checker, view, symbol)?;
        if exports.is_empty() {
            return Ok(());
        }
        if let Some(fix) =
            get_import_fix_for_symbol(checker, view, &exports, is_valid_type_only_use_site)?
        {
            self.add_import_fix(&fix.auto_import);
        }
        Ok(())
    }

    /// tsgo `AddImportFix`: the fix added to the others, its import merged
    /// with the others of its module.
    pub(crate) fn add_import_fix(&mut self, fix: &AutoImportFix) {
        let symbol_name = &fix.name;
        match fix.kind {
            AutoImportFixKind::USE_NAMESPACE => self.add_to_namespace.push(fix.clone()),
            AutoImportFixKind::JSDOC_TYPE_IMPORT => self.import_type.push(fix.clone()),
            AutoImportFixKind::ADD_TO_EXISTING => {
                let imports = collect_external_module_references(self.file).imports;
                let Some(&module_specifier) = imports.get(fix.import_index as usize) else {
                    panic!("import index out of range");
                };
                let (clause_or_pattern, _, named_import) =
                    get_add_to_existing_import_fix(self.file, fix, module_specifier);
                let index = match self
                    .add_to_existing
                    .iter()
                    .position(|(known, _)| *known == clause_or_pattern)
                {
                    Some(index) => index,
                    None => {
                        self.add_to_existing
                            .push((clause_or_pattern, AddToExistingState::default()));
                        self.add_to_existing.len() - 1
                    }
                };
                let entry = &mut self.add_to_existing[index].1;
                if fix.import_kind == ImportKind::NAMED {
                    let previous = entry
                        .named_imports
                        .get(symbol_name)
                        .map(|binding| binding.add_as_type_only);
                    let property_name = named_import
                        .map(|binding| binding.property_name)
                        .unwrap_or_default();
                    entry.named_imports.insert(
                        symbol_name.clone(),
                        NewImportBinding {
                            kind: ImportKind::NAMED,
                            property_name,
                            ..NewImportBinding::new(
                                symbol_name,
                                reduce_add_as_type_only_values(previous, fix.add_as_type_only),
                            )
                        },
                    );
                } else {
                    // Default import
                    assert!(
                        entry
                            .default_import
                            .as_ref()
                            .is_none_or(|binding| binding.name == *symbol_name),
                        "(Add to Existing) Default import should be missing or match symbolName"
                    );
                    let previous = entry
                        .default_import
                        .as_ref()
                        .map(|binding| binding.add_as_type_only);
                    entry.default_import = Some(NewImportBinding {
                        kind: ImportKind::DEFAULT,
                        ..NewImportBinding::new(
                            symbol_name,
                            reduce_add_as_type_only_values(previous, fix.add_as_type_only),
                        )
                    });
                }
            }
            AutoImportFixKind::ADD_NEW => {
                let verbatim = self.options.verbatim_module_syntax == Some(true);
                let entry = self.get_new_import_entry(
                    &fix.module_specifier,
                    fix.import_kind,
                    fix.use_require,
                    fix.add_as_type_only,
                );
                assert!(
                    entry.use_require == fix.use_require,
                    "(Add new) Tried to add an `import` and a `require` for the same module"
                );
                let named = |entry: &mut ImportsCollection, kind: ImportKind| {
                    let previous = entry
                        .named_imports
                        .get(symbol_name)
                        .map(|binding| binding.add_as_type_only);
                    entry.named_imports.insert(
                        symbol_name.clone(),
                        NewImportBinding {
                            kind,
                            ..NewImportBinding::new(
                                symbol_name,
                                reduce_add_as_type_only_values(previous, fix.add_as_type_only),
                            )
                        },
                    );
                };
                match fix.import_kind {
                    ImportKind::DEFAULT => {
                        assert!(
                            entry
                                .default_import
                                .as_ref()
                                .is_none_or(|binding| binding.name == *symbol_name),
                            "(Add new) Default import should be missing or match symbolName"
                        );
                        let previous = entry
                            .default_import
                            .as_ref()
                            .map(|binding| binding.add_as_type_only);
                        entry.default_import = Some(NewImportBinding {
                            kind: ImportKind::DEFAULT,
                            ..NewImportBinding::new(
                                symbol_name,
                                reduce_add_as_type_only_values(previous, fix.add_as_type_only),
                            )
                        });
                    }
                    ImportKind::NAMED => named(entry, ImportKind::NAMED),
                    ImportKind::COMMON_JS if verbatim => named(entry, ImportKind::COMMON_JS),
                    ImportKind::COMMON_JS | ImportKind::NAMESPACE => {
                        assert!(
                            entry
                                .namespace_like_import
                                .as_ref()
                                .is_none_or(|binding| binding.name == *symbol_name),
                            "Namespacelike import should be missing or match symbolName"
                        );
                        entry.namespace_like_import = Some(NewImportBinding {
                            kind: fix.import_kind,
                            ..NewImportBinding::new(symbol_name, fix.add_as_type_only)
                        });
                    }
                    _ => {}
                }
            }
            // Excluding from fix-all
            AutoImportFixKind::PROMOTE_TYPE_ONLY => {}
            kind => panic!("Unexpected fix kind: {}", kind.0),
        }
    }

    /// tsgo `getNewImportEntry`.
    fn get_new_import_entry(
        &mut self,
        module_specifier: &str,
        import_kind: ImportKind,
        use_require: bool,
        add_as_type_only: AddAsTypeOnly,
    ) -> &mut ImportsCollection {
        // A default import that requires type-only makes the whole import type-only.
        // (We could add `default` as a named import, but that style seems undesirable.)
        // Under `--preserveValueImports` and `--importsNotUsedAsValues=error`, if a
        // module default-exports a type but named-exports some values (weird), you would
        // have to use a type-only default import and non-type-only named imports. These
        // require two separate import declarations, so we build this into the map key.
        let type_only_key = new_imports_key(module_specifier, true);
        let non_type_only_key = new_imports_key(module_specifier, false);
        let new_entry = || ImportsCollection {
            use_require,
            ..ImportsCollection::default()
        };
        if import_kind == ImportKind::DEFAULT && add_as_type_only == AddAsTypeOnly::REQUIRED {
            return self
                .new_imports
                .entry(type_only_key)
                .or_insert_with(new_entry);
        }
        if add_as_type_only == AddAsTypeOnly::ALLOWED {
            if self.new_imports.contains_key(&type_only_key) {
                return self
                    .new_imports
                    .get_mut(&type_only_key)
                    .expect("checked above");
            }
            if self.new_imports.contains_key(&non_type_only_key) {
                return self
                    .new_imports
                    .get_mut(&non_type_only_key)
                    .expect("checked above");
            }
        }
        self.new_imports
            .entry(non_type_only_key)
            .or_insert_with(new_entry)
    }
}

/// tsgo `newImportsKey`.
fn new_imports_key(module_specifier: &str, top_level_type_only: bool) -> String {
    format!(
        "{}|{module_specifier}",
        if top_level_type_only { 1 } else { 0 }
    )
}

/// tsgo `reduceAddAsTypeOnlyValues`: `NotAllowed` overrides `Required`
/// because one addition of a new import might be required to be type-only
/// because of `--importsNotUsedAsValues=error`, but if a second addition of
/// the same import is `NotAllowed` to be type-only, the reason the first one
/// was `Required` - the unused runtime dependency - is now moot.
fn reduce_add_as_type_only_values(
    previous: Option<AddAsTypeOnly>,
    new: AddAsTypeOnly,
) -> AddAsTypeOnly {
    match previous {
        Some(previous) if previous.0 >= new.0 => previous,
        _ => new,
    }
}

/// tsgo `getAllExportsForSymbol`: the registry's exports of a symbol's
/// export.
fn get_all_exports_for_symbol(
    checker: &mut CheckerState<'_>,
    view: &View<'_>,
    symbol: SymbolId,
) -> CheckResult<Vec<Arc<Export>>> {
    let path_of = |file_name: &str| view.path_of(file_name);
    Ok(match super::symbol_to_export(checker, &path_of, symbol)? {
        Some(export) => view.search_by_export_id(&export.id),
        None => Vec::new(),
    })
}

/// tsgo `getImportFixForSymbol`: the best fix of any of the exports.
fn get_import_fix_for_symbol(
    checker: &mut CheckerState<'_>,
    view: &mut View<'_>,
    exports: &[Arc<Export>],
    is_valid_type_only_use_site: bool,
) -> CheckResult<Option<Fix>> {
    let mut fixes = Vec::new();
    for export in exports {
        fixes.extend(view.get_fixes(checker, export, false, is_valid_type_only_use_site, None)?);
    }
    fixes.sort_by(|a, b| view.compare_fixes_for_ranking(a, b));
    Ok(fixes.into_iter().next())
}

/// tsgo `TypeToAutoImportableTypeNode`: the type's node (built in `arena`
/// at `target` for `context_node`) with the import types the node builder
/// wrote for symbols replaced by references to them, imported (tsgo
/// `importSymbols`) by the adder there is.
pub(crate) fn type_to_auto_importable_type_node(
    checker: &mut CheckerState<'_>,
    arena: &mut TransformArena,
    target: TransformSourceId,
    adder: Option<(&mut ImportAdder<'_>, &mut View<'_>)>,
    ty: TypeId,
    context_node: NodeId,
) -> CheckResult<Option<TransformNode>> {
    let built = checker
        .type_to_type_node_in_arena(arena, target, ty, Some(context_node), 0, 0)
        .unwrap_or_else(|error| panic!("auto-importable type node: {error}"));
    let Some((type_node, id_to_symbol)) = built else {
        return Ok(None);
    };
    let (type_node, symbols) =
        try_get_auto_importable_reference_from_type_node(checker, arena, type_node, &id_to_symbol)
            .unwrap_or_else(|error| panic!("auto-importable reference: {error}"));
    if let Some((adder, view)) = adder {
        for symbol in symbols {
            adder.add_import_from_exported_symbol(checker, view, symbol, true)?;
        }
    }
    Ok(Some(type_node))
}

/// tsgo `TryGetAutoImportableReferenceFromTypeNode`: a type node with each
/// `import("./a").SomeType<…>` the node builder wrote for a symbol replaced
/// by the type reference `SomeType<…>`, and the symbols to import for it.
pub(crate) fn try_get_auto_importable_reference_from_type_node(
    checker: &CheckerState<'_>,
    arena: &mut TransformArena,
    type_node: TransformNode,
    id_to_symbol: &[(TransformNode, SymbolId)],
) -> Result<(TransformNode, Vec<SymbolId>), TransformError> {
    let mut visitor = AutoImportableReference {
        checker,
        arena,
        source: type_node.source(),
        id_to_symbol: id_to_symbol.iter().copied().collect(),
        symbols: Vec::new(),
    };
    let node = visitor.visit(type_node)?;
    Ok((node, visitor.symbols))
}

struct AutoImportableReference<'v, 'c> {
    checker: &'v CheckerState<'c>,
    arena: &'v mut TransformArena,
    source: TransformSourceId,
    id_to_symbol: HashMap<TransformNode, SymbolId>,
    symbols: Vec<SymbolId>,
}

impl AutoImportableReference<'_, '_> {
    fn visit(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        if let NodeData::ImportType(data) = &self.arena.node(node)?.data {
            let data = data.clone();
            if let (Some(qualifier), true) =
                (data.qualifier, self.is_literal_argument(data.argument)?)
            {
                let qualifier = self.node_ref(qualifier)?;
                // Symbol for the left-most thing after the dot
                let first_identifier = self.first_identifier(qualifier)?;
                if let Some(&symbol) = self.id_to_symbol.get(&first_identifier) {
                    let name = get_name_for_exported_symbol(self.checker, symbol, false);
                    let qualifier = if name != self.identifier_text(first_identifier)? {
                        let identifier =
                            self.arena.factory().create_identifier(self.source, name)?;
                        self.replace_first_identifier_of_entity_name(qualifier, identifier)?
                    } else {
                        qualifier
                    };
                    self.symbols.push(symbol);
                    let type_arguments = match data.type_arguments {
                        Some(arguments) => Some(self.visit_array(arguments)?),
                        None => None,
                    };
                    return self.arena.factory().create_type_reference_node(
                        self.source,
                        qualifier,
                        type_arguments,
                    );
                }
                // if symbol is missing then this doesn't come from a synthesized import type node
                // it has to be an import type node authored by the user and thus it has to be valid
                // it can't refer to reserved internal symbol names and such
            }
        }
        let mut data = self.arena.node(node)?.data.clone();
        let original = data.clone();
        tsc_syntax::try_visit_each_child(&mut data, self)?;
        if data == original {
            return Ok(node);
        }
        self.arena
            .factory()
            .update_node(node, data, tsc_emitter::TransformFlags::NONE)
    }

    fn node_ref(&self, node: NodeId) -> Result<TransformNode, TransformError> {
        self.arena
            .node_ref(self.source, node)
            .ok_or(TransformError::UnknownSource(self.source))
    }

    /// tsgo `IsLiteralImportTypeNode`'s argument: a literal type of a string.
    fn is_literal_argument(&self, argument: Option<NodeId>) -> Result<bool, TransformError> {
        let Some(argument) = argument else {
            return Ok(false);
        };
        let argument = self.node_ref(argument)?;
        let NodeData::LiteralType(literal) = &self.arena.node(argument)?.data else {
            return Ok(false);
        };
        let Some(literal) = literal.literal else {
            return Ok(false);
        };
        Ok(self.arena.node(self.node_ref(literal)?)?.kind == SyntaxKind::StringLiteral)
    }

    /// tsgo `GetFirstIdentifier`.
    fn first_identifier(&self, name: TransformNode) -> Result<TransformNode, TransformError> {
        match &self.arena.node(name)?.data {
            NodeData::QualifiedName(data) => match data.left {
                Some(left) => self.first_identifier(self.node_ref(left)?),
                None => Ok(name),
            },
            _ => Ok(name),
        }
    }

    fn identifier_text(&self, identifier: TransformNode) -> Result<String, TransformError> {
        Ok(match &self.arena.node(identifier)?.data {
            NodeData::Identifier(data) => data.text().to_owned(),
            _ => String::new(),
        })
    }

    /// tsgo `replaceFirstIdentifierOfEntityName`.
    fn replace_first_identifier_of_entity_name(
        &mut self,
        name: TransformNode,
        new_identifier: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::QualifiedName(data) = self.arena.node(name)?.data.clone() else {
            return Ok(new_identifier);
        };
        let left = match data.left {
            Some(left) => {
                let left = self.node_ref(left)?;
                self.replace_first_identifier_of_entity_name(left, new_identifier)?
            }
            None => new_identifier,
        };
        let right = match data.right {
            Some(right) => self.node_ref(right)?,
            None => return Ok(left),
        };
        self.arena
            .factory()
            .create_qualified_name(self.source, left, right)
    }

    fn visit_array(
        &mut self,
        array: NodeArrayId,
    ) -> Result<tsc_emitter::TransformNodeArray, TransformError> {
        let original = self
            .arena
            .node_array_ref(self.source, array)
            .ok_or(TransformError::UnknownSource(self.source))?;
        let nodes = self.arena.node_array(original)?.nodes.to_vec();
        let mut visited = Vec::with_capacity(nodes.len());
        let mut changed = false;
        for node in nodes {
            let node = self.node_ref(node)?;
            let result = self.visit(node)?;
            changed |= result != node;
            visited.push(result);
        }
        if !changed {
            return Ok(original);
        }
        self.arena.factory().create_node_array(self.source, visited)
    }
}

impl NodeDataChildVisitor for AutoImportableReference<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.node_ref(id)
            .and_then(|node| self.arena.node(node).map(|node| node.kind))
            .unwrap_or(SyntaxKind::Unknown)
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        let node = self.node_ref(id)?;
        Ok(Some(self.visit(node)?.node()))
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        Ok(Some(self.visit_array(id)?.array()))
    }

    fn required_child_removed(&mut self, _parent: SyntaxKind, _field: &'static str) -> Self::Error {
        TransformError::UnknownSource(self.source)
    }
}

/// tsgo `getNameForExportedSymbol`: a default export's local name, else its
/// module's name as an identifier; any other export's own name.
pub(crate) fn get_name_for_exported_symbol(
    checker: &CheckerState<'_>,
    symbol: SymbolId,
    prefer_capitalized: bool,
) -> String {
    let data = checker.binder.symbol(symbol);
    let name = data.escaped_name.unescape().to_string_lossy().into_owned();
    if name != InternalSymbolName::EXPORT_EQUALS && name != InternalSymbolName::DEFAULT {
        return name;
    }
    // Names for default exports:
    // - export default foo => foo
    // - export { foo as default } => foo
    // - export default 0 => filename converted to camelCase
    let name = super::extract::get_default_like_export_name_from_declaration(checker, symbol);
    if !name.is_empty() {
        return name;
    }
    let parent = data
        .parent
        .expect("Expected exported symbol to have module symbol as parent");
    // tsgo `lsutil.ModuleSymbolToValidIdentifier`.
    let module_name = checker
        .binder
        .symbol(parent)
        .escaped_name
        .unescape()
        .to_string_lossy()
        .into_owned();
    let module_name = module_name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(&module_name);
    crate::lsutil::module_specifier_to_valid_identifier(module_name, prefer_capitalized)
}

/// An `importSymbol` action of the API's import adder (tsgo
/// `ImportAdderAction`): a symbol to import, and whether its use site may
/// be type-only.
#[derive(Clone, Copy, Debug)]
pub struct ImportSymbolAction {
    pub symbol: SymbolId,
    pub is_valid_type_only_use_site: bool,
}

impl crate::language_service::LanguageService<'_> {
    /// tsgo `handleGetImportAdderEdits`' adder: the edits that import each
    /// action's symbol into the file, none without a registry; the
    /// needs-auto-imports error when the snapshot's registry is not
    /// prepared for the file.
    pub fn get_import_adder_edits(
        &mut self,
        file: usize,
        actions: &[ImportSymbolAction],
    ) -> Result<Vec<TextEdit>, crate::language_service::LsError> {
        use crate::language_service::LsError;
        let edits = self.with_program_request(false, |request| {
            let mut view = match crate::code_actions::prepared_view(request, file) {
                Ok(Some(view)) => view,
                Ok(None) => return Ok(Ok(Vec::new())),
                Err(error) => return Ok(Err(error)),
            };
            let source = request.state.binder.source(file);
            let mut adder = ImportAdder::new(
                source,
                request.state.options,
                request.preferences.format_code_settings.clone(),
                request.converters,
                request.preferences,
            );
            for action in actions {
                adder.add_import_from_exported_symbol(
                    request.state,
                    &mut view,
                    action.symbol,
                    action.is_valid_type_only_use_site,
                )?;
            }
            if !adder.has_fixes() {
                return Ok(Ok(Vec::new()));
            }
            Ok(adder.edits().map_err(LsError))
        })?;
        edits.transpose().map(Option::unwrap_or_default)
    }
}

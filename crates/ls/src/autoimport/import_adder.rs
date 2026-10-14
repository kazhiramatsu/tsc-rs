//! tsgo `ls/autoimport/import_adder.go`: fixes gathered over a file and made
//! at once, the imports of a module coalesced into one declaration.

use std::collections::BTreeMap;

use tsc_lsproto::{AddAsTypeOnly, AutoImportFix, AutoImportFixKind, ImportKind, TextEdit};
use tsc_syntax::{NodeId, SourceFile};
use tsc_types::CompilerOptions;

use super::edits::{
    add_import_type, add_namespace_qualifier, add_to_existing_import, file_edits,
    get_add_to_existing_import_fix, get_new_imports, get_new_requires, insert_imports,
    NewImportBinding,
};
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

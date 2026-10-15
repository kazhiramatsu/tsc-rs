//! tsgo `ls/autoimport/view.go` and the fixes of `fix.go`: what a file can
//! import from the registry (its project's bucket, without the file's own
//! exports, and the node_modules buckets above it, their packages filtered
//! by the package.json files above it) and the fixes that import it — a new
//! import, an addition to an existing one, a namespace's qualification —
//! ranked.

use std::cell::OnceCell;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

use tsc_checker::state::{CheckResult, CheckerState};
use tsc_checker::{ModuleSpecifierKind, ModuleSpecifierUserPreferences};
use tsc_lsproto::{AddAsTypeOnly, AutoImportFix, AutoImportFixKind, ImportKind, Position};
use tsc_program::PreparedProgram;
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{InternalSymbolName, SymbolFlags};

use super::export::{Export, ExportId, ExportSyntax, ModuleId};
use super::extract::try_get_module_id_and_file_name_of_module_symbol;
use super::package_json::add_package_json_dependencies;
use super::registry::{
    contains_lowercase_directory_sequence, parent_path, Registry, RegistryBucket,
};
use crate::references::{collect_external_module_references, statements};
use crate::utilities::path_key;
use tsc_syntax::go_unicode::{is_upper, to_upper_char};

/// tsgo `Fix`: the protocol's fix and how it ranks.
#[derive(Clone, Debug)]
pub(crate) struct Fix {
    pub(crate) auto_import: AutoImportFix,
    /// tsgo `ModuleSpecifierKind` (`ResultKindNone` when none).
    pub(crate) module_specifier_kind: Option<ModuleSpecifierKind>,
    pub(crate) is_re_export: bool,
    pub(crate) module_file_name: String,
    /// The import that made the symbol type-only (a promotion's).
    pub(crate) type_only_alias_declaration: Option<NodeId>,
}

impl Fix {
    /// A fix of the protocol's data (a completion item's), which ranks no
    /// other.
    pub(crate) fn of(auto_import: AutoImportFix) -> Self {
        Self {
            auto_import,
            module_specifier_kind: None,
            is_re_export: false,
            module_file_name: String::new(),
            type_only_alias_declaration: None,
        }
    }
}

/// tsgo `FixAndExport`.
#[derive(Clone, Debug)]
pub(crate) struct FixAndExport {
    pub(crate) fix: Fix,
    pub(crate) export: Arc<Export>,
}

/// tsgo `existingImport`.
#[derive(Clone, Debug)]
struct ExistingImport {
    node: NodeId,
    module_specifier: String,
    index: usize,
}

/// tsgo `QueryKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueryKind {
    WordPrefix,
    ExactMatch,
    CaseInsensitiveMatch,
}

/// tsgo `View`.
pub(crate) struct View<'r> {
    registry: &'r Registry,
    project_id: &'r str,
    prepared: &'r PreparedProgram,
    importing_file: usize,
    importing_file_name: String,
    importing_file_path: String,
    preferences: ModuleSpecifierUserPreferences,
    /// tsgo `conditions`: the importing file's resolution conditions.
    conditions: BTreeSet<String>,
    /// tsgo `allowedEndings`, computed when first needed.
    allowed_endings: OnceCell<Vec<tsc_checker::ModuleSpecifierEnding>>,
    /// tsgo `shouldUseUriStyleNodeCoreModules`.
    should_use_uri_style_node_core_modules: Option<bool>,
    existing_imports: Option<Vec<(ModuleId, ExistingImport)>>,
    should_use_require: Option<bool>,
}

impl<'r> View<'r> {
    /// tsgo `NewView`.
    pub(crate) fn new(
        registry: &'r Registry,
        checker: &CheckerState<'_>,
        importing_file: usize,
        project_id: &'r str,
        prepared: &'r PreparedProgram,
        preferences: ModuleSpecifierUserPreferences,
    ) -> Self {
        let file_name = checker
            .binder
            .source(importing_file)
            .file_name
            .to_string_lossy()
            .into_owned();
        let source = checker.binder.source(importing_file);
        let root = source.root;
        let should_use_uri_style_node_core_modules = should_use_uri_style_node_core_modules(source);
        let conditions = tsc_program::get_conditions(
            prepared.compiler_options(),
            resolution_mode(checker.api_default_resolution_mode_for_file(root)),
        )
        .into_iter()
        .collect();
        Self {
            registry,
            project_id,
            prepared,
            importing_file,
            importing_file_path: path_key(prepared, &file_name),
            importing_file_name: file_name,
            preferences,
            conditions,
            allowed_endings: OnceCell::new(),
            should_use_uri_style_node_core_modules,
            existing_imports: None,
            should_use_require: None,
        }
    }

    /// tsgo `getAllowedEndings`.
    fn get_allowed_endings(
        &self,
        checker: &CheckerState<'_>,
    ) -> &[tsc_checker::ModuleSpecifierEnding] {
        self.allowed_endings.get_or_init(|| {
            let root = self.source(checker).root;
            let mode = match checker.api_default_resolution_mode_for_file(root) {
                1 => tsc_emitter::EmitResolutionMode::CommonJs,
                99 => tsc_emitter::EmitResolutionMode::EsNext,
                _ => tsc_emitter::EmitResolutionMode::None,
            };
            checker.get_allowed_endings_in_preferred_order(
                root,
                self.preferences.import_module_specifier_ending,
                mode,
            )
        })
    }

    /// tsgo `ToPath` of a file name of the view's program.
    pub(crate) fn path_of(&self, file_name: &str) -> String {
        path_key(self.prepared, file_name)
    }

    fn source<'c>(&self, checker: &'c CheckerState<'_>) -> &'c SourceFile {
        checker.binder.source(self.importing_file)
    }

    /// tsgo `Search`.
    pub(crate) fn search(&self, query: &str, kind: QueryKind) -> Vec<Arc<Export>> {
        self.search_buckets(|bucket| match kind {
            QueryKind::WordPrefix => bucket.index.search_word_prefix(query),
            QueryKind::ExactMatch => bucket.index.find(query, true),
            QueryKind::CaseInsensitiveMatch => bucket.index.find(query, false),
        })
    }

    /// tsgo `SearchByExportID`.
    pub(crate) fn search_by_export_id(&self, id: &ExportId) -> Vec<Arc<Export>> {
        self.search_buckets(|bucket| bucket.index.with_export_id(id))
    }

    /// tsgo `search`: the project bucket's exports, and the exports of the
    /// packages in the node_modules buckets above the importing file that
    /// the package.json files above it depend on (a package in a lower
    /// node_modules shadowing one above).
    fn search_buckets(
        &self,
        search: impl Fn(&RegistryBucket) -> Vec<Arc<Export>>,
    ) -> Vec<Arc<Export>> {
        let mut results = Vec::new();
        let project_bucket = self.registry.project_bucket(self.project_id);
        if let Some(bucket) = project_bucket {
            // Don't auto-import from the importing file itself
            results.extend(search(bucket).into_iter().filter(|export| {
                export.id.module_id.as_path() != Some(self.importing_file_path.as_str())
            }));
        }
        let ancestors = || {
            let mut directory = parent_path(&self.importing_file_path);
            std::iter::from_fn(move || {
                let current = directory;
                let parent = parent_path(current);
                if parent == current {
                    directory = "";
                } else {
                    directory = parent;
                }
                (!current.is_empty()).then_some(current)
            })
        };
        // The packages accessible to the importing file: the dependencies of the
        // package.json files above it plus the packages the project's program files
        // import. With no package.json, every package is.
        let mut allowed_packages: Option<BTreeSet<String>> = None;
        for directory in ancestors() {
            let Some(contents) = self
                .registry
                .directory(directory)
                .and_then(|directory| directory.package_json.contents.as_ref())
                .filter(|contents| contents.parseable)
            else {
                continue;
            };
            add_package_json_dependencies(contents, allowed_packages.get_or_insert_default());
        }
        if let (Some(allowed), Some(bucket)) = (&mut allowed_packages, project_bucket) {
            if let Some(names) = &bucket.resolved_package_names {
                allowed.extend(names.iter().cloned());
            }
        }
        let mut exclude_packages = BTreeSet::new();
        for directory in ancestors() {
            let Some(bucket) = self.registry.node_modules_bucket(directory) else {
                continue;
            };

            for export in search(bucket) {
                // Exclude packages found in lower node_modules (shadowing)
                if exclude_packages.contains(&export.package_name) {
                    continue;
                }
                if allowed_packages
                    .as_ref()
                    .is_some_and(|allowed| !allowed.contains(&export.package_name))
                {
                    continue;
                }
                results.push(export);
            }
            // As we go up the directory tree, exclude packages found in lower node_modules
            exclude_packages.extend(bucket.package_files.keys().cloned());
        }
        results
    }

    /// tsgo `GetCompletions`: the best fixes of each group of exports a
    /// completion at the prefix could import, best first.
    pub(crate) fn get_completions(
        &mut self,
        checker: &mut CheckerState<'_>,
        prefix: &str,
        position: Position,
        for_jsx: bool,
        is_type_only_location: bool,
    ) -> CheckResult<Vec<FixAndExport>> {
        let results = self.search(prefix, QueryKind::WordPrefix);
        type GroupKey = (ExportId, String, String);
        let mut grouped: Vec<(GroupKey, Vec<Arc<Export>>)> = Vec::new();
        'outer: for export in results {
            let name = export.name().to_owned();
            if !tsc_syntax::is_identifier_text(&name) {
                continue;
            }
            if for_jsx && !(is_upper(char::from(name.as_bytes()[0])) || export.is_renameable()) {
                continue;
            }
            let ambient_module_or_package_name = if export.ambient_module_name().is_empty() {
                export.package_name.clone()
            } else {
                export.ambient_module_name().to_owned()
            };
            let mut key = (
                export.target_or_self().clone(),
                name,
                ambient_module_or_package_name,
            );
            if (export.package_name == "@types/node"
                || contains_lowercase_directory_sequence(
                    &export.path,
                    "/node_modules/@types/node/",
                ))
                && tsc_checker::modules::UNPREFIXED_NODE_CORE_MODULES.contains(&key.2.as_str())
            {
                // Group URI-style and non-URI style node core modules together so the
                // ranking logic is allowed to drop one if an explicit preference is
                // detected.
                key.2 = format!("node:{}", key.2);
            }
            if let Some((_, existing)) = grouped.iter_mut().find(|(known, _)| *known == key) {
                for slot in existing.iter_mut() {
                    if slot.id == export.id {
                        *slot = Arc::new(Export {
                            id: export.id.clone(),
                            module_file_name: export.module_file_name.clone(),
                            unresolved_module_specifier: export.unresolved_module_specifier.clone(),
                            package_name: export.package_name.clone(),
                            is_type_only: export.is_type_only || slot.is_type_only,
                            syntax: export.syntax.min(slot.syntax),
                            flags: export.flags | slot.flags,
                            script_element_kind: export
                                .script_element_kind
                                .min(slot.script_element_kind),
                            script_element_kind_modifiers: crate::lsutil::ScriptElementKindModifier(
                                export.script_element_kind_modifiers.0
                                    | slot.script_element_kind_modifiers.0,
                            ),
                            local_name: export.local_name.clone(),
                            through: String::new(),
                            target: export.target.clone(),
                            path: export.path.clone(),
                        });
                        continue 'outer;
                    }
                }
                existing.push(export);
            } else {
                grouped.push((key, vec![export]));
            }
        }

        let mut fixes = Vec::new();
        for (_, exports) in grouped {
            let mut fixes_for_group = Vec::new();
            for export in exports {
                for fix in self.get_fixes(
                    checker,
                    &export,
                    for_jsx,
                    is_type_only_location,
                    Some(position.clone()),
                )? {
                    fixes_for_group.push(FixAndExport {
                        fix,
                        export: Arc::clone(&export),
                    });
                }
            }
            // tsgo `core.MinAllFunc`: every fix as good as the best.
            let mut best: Vec<FixAndExport> = Vec::new();
            for candidate in fixes_for_group {
                match best.first() {
                    None => best.push(candidate),
                    Some(first) => match self.compare_fixes_for_ranking(&candidate.fix, &first.fix)
                    {
                        Ordering::Less => best = vec![candidate],
                        Ordering::Equal => best.push(candidate),
                        Ordering::Greater => {}
                    },
                }
            }
            fixes.extend(best);
        }
        // The client will do additional sorting by SortText and Label, so we don't
        // need to consider the name in our sorting here; we only need to produce a
        // stable relative ordering between completions that the client will consider
        // equivalent.
        fixes.sort_by(|a, b| self.compare_fixes_for_sorting(&a.fix, &b.fix));
        Ok(fixes)
    }

    /// tsgo `GetFixes`.
    pub(crate) fn get_fixes(
        &mut self,
        checker: &mut CheckerState<'_>,
        export: &Export,
        for_jsx: bool,
        is_valid_type_only_use_site: bool,
        usage_position: Option<Position>,
    ) -> CheckResult<Vec<Fix>> {
        let mut fixes = Vec::new();
        if let Some(namespace_fix) =
            self.try_use_existing_namespace_import(checker, export, usage_position.clone())?
        {
            fixes.push(namespace_fix);
        }
        if let Some(fix) =
            self.try_add_to_existing_import(checker, export, is_valid_type_only_use_site)?
        {
            fixes.push(fix);
            return Ok(fixes);
        }
        // !!! getNewImportFromExistingSpecifier - even worth it?
        let (module_specifier, module_specifier_kind) =
            self.get_module_specifier(checker, export)?;
        if module_specifier.is_empty() {
            return Ok(fixes);
        }
        // Check if we need a JSDoc import type fix (for JS files with type-only imports)
        let is_js = has_js_file_extension(&self.source(checker).file_name.to_string_lossy());
        let imported_symbol_has_value_meaning =
            export.flags.intersects(SymbolFlags::VALUE) || export.is_unresolved_alias();
        let is_re_export =
            export.target.as_ref().map(|target| &target.module_id) != Some(&export.id.module_id);
        if !imported_symbol_has_value_meaning && is_js && usage_position.is_some() {
            // For pure types in JS files, use JSDoc import type syntax
            return Ok(vec![Fix {
                auto_import: AutoImportFix {
                    kind: AutoImportFixKind::JSDOC_TYPE_IMPORT,
                    module_specifier,
                    name: export.name().to_owned(),
                    usage_position,
                    ..AutoImportFix::default()
                },
                module_specifier_kind,
                is_re_export,
                module_file_name: export.module_file_name.clone(),
                type_only_alias_declaration: None,
            }]);
        }
        let import_kind = self.get_import_kind(checker, export, false);
        let add_as_type_only = get_add_as_type_only(is_valid_type_only_use_site, export, checker);
        let mut name = export.name().to_owned();
        let starts_with_upper = is_upper(char::from(name.as_bytes()[0]));
        if for_jsx && !starts_with_upper {
            if export.is_renameable() {
                let first = to_upper_char(char::from(name.as_bytes()[0]));
                name = format!("{first}{}", &name[1..]);
            } else {
                return Ok(Vec::new());
            }
        }
        let use_require = self.should_use_require(checker);
        fixes.push(Fix {
            auto_import: AutoImportFix {
                kind: AutoImportFixKind::ADD_NEW,
                import_kind,
                module_specifier,
                name,
                use_require,
                add_as_type_only,
                ..AutoImportFix::default()
            },
            module_specifier_kind,
            is_re_export,
            module_file_name: export.module_file_name.clone(),
            type_only_alias_declaration: None,
        });
        Ok(fixes)
    }

    /// tsgo `tryUseExistingNamespaceImport`.
    fn try_use_existing_namespace_import(
        &mut self,
        checker: &mut CheckerState<'_>,
        export: &Export,
        usage_position: Option<Position>,
    ) -> CheckResult<Option<Fix>> {
        let Some(usage_position) = usage_position else {
            return Ok(None);
        };
        if self.get_import_kind(checker, export, false) != ImportKind::NAMED {
            return Ok(None);
        }
        let existing = self.get_existing_imports(checker)?.to_vec();
        for (module_id, existing_import) in &existing {
            if *module_id != export.id.module_id {
                continue;
            }
            let namespace_prefix =
                namespace_like_import_text(self.source(checker), existing_import.node);
            if namespace_prefix.is_empty() || existing_import.module_specifier.is_empty() {
                continue;
            }
            return Ok(Some(Fix {
                auto_import: AutoImportFix {
                    kind: AutoImportFixKind::USE_NAMESPACE,
                    name: export.name().to_owned(),
                    module_specifier: existing_import.module_specifier.clone(),
                    import_kind: ImportKind::NAMESPACE,
                    add_as_type_only: AddAsTypeOnly::ALLOWED,
                    import_index: existing_import.index as i32,
                    usage_position: Some(usage_position),
                    namespace_prefix,
                    ..AutoImportFix::default()
                },
                module_specifier_kind: None,
                is_re_export: false,
                module_file_name: String::new(),
                type_only_alias_declaration: None,
            }));
        }
        Ok(None)
    }

    /// tsgo `tryAddToExistingImport`.
    fn try_add_to_existing_import(
        &mut self,
        checker: &mut CheckerState<'_>,
        export: &Export,
        is_valid_type_only_use_site: bool,
    ) -> CheckResult<Option<Fix>> {
        let matching: Vec<ExistingImport> = self
            .get_existing_imports(checker)?
            .iter()
            .filter(|(module_id, _)| *module_id == export.id.module_id)
            .map(|(_, existing)| existing.clone())
            .collect();
        if matching.is_empty() {
            return Ok(None);
        }
        let source = self.source(checker);
        // Can't use an es6 import for a type in JS.
        if is_source_file_js(source)
            && !export.flags.intersects(SymbolFlags::VALUE)
            && !matching
                .iter()
                .all(|existing| source.arena.node(existing.node).kind == SyntaxKind::JSDocImportTag)
        {
            return Ok(None);
        }
        let import_kind = self.get_import_kind(checker, export, false);
        if import_kind == ImportKind::COMMON_JS || import_kind == ImportKind::NAMESPACE {
            return Ok(None);
        }
        let add_as_type_only = get_add_as_type_only(is_valid_type_only_use_site, export, checker);
        let source = self.source(checker);
        let fix_for = |existing: &ExistingImport| Fix {
            auto_import: AutoImportFix {
                kind: AutoImportFixKind::ADD_TO_EXISTING,
                name: export.name().to_owned(),
                import_kind,
                import_index: existing.index as i32,
                module_specifier: existing.module_specifier.clone(),
                add_as_type_only,
                ..AutoImportFix::default()
            },
            module_specifier_kind: None,
            is_re_export: false,
            module_file_name: String::new(),
            type_only_alias_declaration: None,
        };
        let mut best = None;
        for existing in &matching {
            let record = source.arena.node(existing.node);
            match &record.data {
                NodeData::ImportEqualsDeclaration(_) => continue,
                NodeData::VariableDeclaration(data) => {
                    if (import_kind == ImportKind::NAMED || import_kind == ImportKind::DEFAULT)
                        && data.name.is_some_and(|name| {
                            source.arena.node(name).kind == SyntaxKind::ObjectBindingPattern
                        })
                    {
                        let fix = fix_for(existing);
                        // Variable declarations are never type-only.
                        // Give preference to putting types in existing type-only imports and avoiding conversions
                        // of import statements to/from type-only.
                        if add_as_type_only == AddAsTypeOnly::NOT_ALLOWED {
                            return Ok(Some(fix));
                        }
                        best.get_or_insert(fix);
                    }
                    continue;
                }
                _ => {}
            }
            let Some((clause, module_specifier)) =
                import_clause_and_specifier(source, existing.node)
            else {
                // Side-effect import (no import clause) - can't add to it
                continue;
            };
            if !matches!(
                source.arena.node(module_specifier).kind,
                SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
            ) {
                continue;
            }
            let NodeData::ImportClause(clause_data) = &source.arena.node(clause).data else {
                continue;
            };
            if clause_data.phase_modifier == Some(SyntaxKind::SourceKeyword) {
                continue;
            }
            let named_bindings = clause_data.named_bindings;
            // A type-only import may not have both a default and named imports, so the only way a name can
            // be added to an existing type-only import is adding a named import to existing named bindings.
            if clause_data.is_type_only
                && !(import_kind == ImportKind::NAMED && named_bindings.is_some())
            {
                continue;
            }
            if import_kind == ImportKind::DEFAULT
                && (clause_data.name.is_some()
                    // Cannot add a default import as type-only if the import already has named bindings
                    || add_as_type_only == AddAsTypeOnly::REQUIRED && named_bindings.is_some())
            {
                continue;
            }
            // Cannot add a named import to a declaration that has a namespace import
            if import_kind == ImportKind::NAMED
                && named_bindings.is_some_and(|bindings| {
                    source.arena.node(bindings).kind == SyntaxKind::NamespaceImport
                })
            {
                continue;
            }
            let fix = fix_for(existing);
            let is_type_only = clause_data.is_type_only;
            // Give preference to putting types in existing type-only imports and avoiding conversions
            // of import statements to/from type-only.
            if (add_as_type_only != AddAsTypeOnly::NOT_ALLOWED && is_type_only)
                || (add_as_type_only == AddAsTypeOnly::NOT_ALLOWED && !is_type_only)
            {
                return Ok(Some(fix));
            }
            best.get_or_insert(fix);
        }
        Ok(best)
    }

    /// tsgo `getImportKind` of the view's file.
    pub(crate) fn get_import_kind(
        &self,
        checker: &CheckerState<'_>,
        export: &Export,
        force_import_keyword: bool,
    ) -> ImportKind {
        get_import_kind(checker, self.importing_file, export, force_import_keyword)
    }

    /// tsgo `getExistingImports`: the file's imports by the module they
    /// import.
    fn get_existing_imports(
        &mut self,
        checker: &mut CheckerState<'_>,
    ) -> CheckResult<&[(ModuleId, ExistingImport)]> {
        if self.existing_imports.is_none() {
            let source = checker.binder.source(self.importing_file);
            let imports = collect_external_module_references(source).imports;
            let path_of = |file_name: &str| path_key(self.prepared, file_name);
            let mut result = Vec::with_capacity(imports.len());
            for (index, module_specifier) in imports.into_iter().enumerate() {
                let source = checker.binder.source(self.importing_file);
                let Some(node) = try_get_import_from_module_specifier(source, module_specifier)
                else {
                    panic!(
                        "error: did not expect node kind {:?}",
                        source.arena.node(module_specifier).kind
                    );
                };
                let text = literal_text(source, module_specifier);
                let parent = source.arena.node(node).parent;
                let require_declaration = parent.filter(|&parent| {
                    is_variable_declaration_initialized_to_require(
                        checker.binder.source_of_node(parent),
                        parent,
                    )
                });
                if let Some(declaration) = require_declaration {
                    if let Some(module_symbol) =
                        checker.resolve_external_module_name_of(module_specifier)?
                    {
                        if let Some((module_id, _)) =
                            try_get_module_id_and_file_name_of_module_symbol(
                                checker,
                                &path_of,
                                module_symbol,
                            )
                        {
                            result.push((
                                module_id,
                                ExistingImport {
                                    node: declaration,
                                    module_specifier: text,
                                    index,
                                },
                            ));
                        }
                    }
                } else if matches!(
                    source.arena.node(node).kind,
                    // tsgo also names `JSDocImportTag`, but its import of an
                    // `@import` tag is the reparsed `JSImportDeclaration`, so
                    // a tag is never an existing import.
                    SyntaxKind::ImportDeclaration | SyntaxKind::ImportEqualsDeclaration
                ) {
                    if let Some(module_symbol) = checker.get_symbol_at_location(module_specifier)? {
                        if let Some((module_id, _)) =
                            try_get_module_id_and_file_name_of_module_symbol(
                                checker,
                                &path_of,
                                module_symbol,
                            )
                        {
                            result.push((
                                module_id,
                                ExistingImport {
                                    node,
                                    module_specifier: text,
                                    index,
                                },
                            ));
                        }
                    }
                }
            }
            self.existing_imports = Some(result);
        }
        Ok(self.existing_imports.as_deref().expect("computed above"))
    }

    /// tsgo `shouldUseRequire`.
    fn should_use_require(&mut self, checker: &CheckerState<'_>) -> bool {
        if let Some(should_use_require) = self.should_use_require {
            return should_use_require;
        }
        let should_use_require = self.compute_should_use_require(checker);
        self.should_use_require = Some(should_use_require);
        should_use_require
    }

    /// tsgo `computeShouldUseRequire`.
    fn compute_should_use_require(&self, checker: &CheckerState<'_>) -> bool {
        let source = self.source(checker);
        // 1. TypeScript files don't use require variable declarations
        if !has_js_file_extension(&source.file_name.to_string_lossy()) {
            return false;
        }
        // 2. If the current source file is unambiguously CJS or ESM, go with that
        match detect_syntax(checker, self.importing_file) {
            FileSyntax::Cjs => return true,
            FileSyntax::Esm => return false,
            FileSyntax::Ambiguous => {}
        }
        // 3. Use the implied node format to determine CJS vs ESM
        //    TODO: consider removing `impliedNodeFormatForEmit`
        match checker.implied_node_format_for_emit(source.root) {
            Some(tsc_checker::modules::ModuleResolutionMode::CommonJs) => return true,
            Some(tsc_checker::modules::ModuleResolutionMode::EsNext) => return false,
            _ => {}
        }
        // 4. If there's a tsconfig/jsconfig, use its module setting
        if self.prepared.program_options().config_file_path().is_some() {
            return checker.options.emit_module_kind() < 5;
        }
        // 5. Match the first other JS file in the program that's unambiguously CJS or ESM
        for file in 0..checker.binder.file_count() {
            let other = checker.binder.source(file);
            if file == self.importing_file
                || !is_source_file_js(other)
                || self
                    .prepared
                    .source_files()
                    .get(file)
                    .is_some_and(|source| source.found_searching_node_modules())
            {
                continue;
            }
            match detect_syntax(checker, file) {
                FileSyntax::Cjs => return true,
                FileSyntax::Esm => return false,
                FileSyntax::Ambiguous => {}
            }
        }
        // 6. Literally nothing to go on
        true
    }

    /// tsgo `GetModuleSpecifier`.
    pub(crate) fn get_module_specifier(
        &self,
        checker: &mut CheckerState<'_>,
        export: &Export,
    ) -> CheckResult<(String, Option<ModuleSpecifierKind>)> {
        let excluded = |specifier: &str| {
            tsc_checker::is_excluded_by_regex(
                specifier,
                &self.preferences.auto_import_specifier_exclude_regexes,
            )
        };
        if !export.unresolved_module_specifier.is_empty() {
            // tsgo `ModuleSpecifier.IsRelative`: a rooted name stays as it is.
            let mut specifier = export.unresolved_module_specifier.clone();
            if tsc_program::path_is_relative(tsc_types::JsStr::from_str(&specifier)) {
                let importing_directory = tsc_program::get_directory_path(
                    tsc_types::JsStr::from_str(&self.source(checker).file_name.to_string_lossy()),
                )
                .to_string_lossy()
                .into_owned();
                let case_sensitive = self.prepared.path_context().use_case_sensitive_file_names();
                let Some(relative) = relative_path_from_directory(
                    &importing_directory,
                    &export.module_file_name,
                    case_sensitive,
                ) else {
                    return Ok((String::new(), None));
                };
                specifier = relative;
            }
            if excluded(&specifier) {
                return Ok((String::new(), None));
            }
            return Ok((specifier, Some(ModuleSpecifierKind::Relative)));
        }
        if let Some(specifier) = export.id.module_id.as_module_specifier() {
            if excluded(specifier) {
                return Ok((String::new(), None));
            }
            return Ok((specifier.to_owned(), Some(ModuleSpecifierKind::Ambient)));
        }
        if !export.package_name.is_empty() {
            if let Some(entrypoints) = self.registry.entrypoints(&export.path) {
                for entrypoint in entrypoints {
                    if entrypoint.include_conditions.is_subset(&self.conditions)
                        && entrypoint.exclude_conditions.is_disjoint(&self.conditions)
                    {
                        let specifier = tsc_checker::process_entrypoint_ending(
                            &entrypoint.module_specifier,
                            entrypoint.ending,
                            self.get_allowed_endings(checker),
                            self.prepared.compiler_options(),
                        );
                        if !excluded(&specifier) {
                            return Ok((specifier, Some(ModuleSpecifierKind::NodeModules)));
                        }
                    }
                }
                return Ok((String::new(), None));
            }
        }
        let cache = self.registry.specifier_cache(&self.importing_file_path);
        if export.package_name.is_empty() {
            if let Some(cache) = cache {
                if let Some(specifier) = cache.lock().expect("specifier cache").get(&export.path) {
                    if specifier.is_empty() {
                        return Ok((String::new(), None));
                    }
                    return Ok((specifier.clone(), Some(ModuleSpecifierKind::Relative)));
                }
            }
        }
        let root = self.source(checker).root;
        let (specifiers, kind) = checker.get_module_specifiers_for_file_with_info(
            root,
            &tsc_types::JsString::from(export.module_file_name.as_str()),
            &self.preferences,
        )?;
        // !!! unsure when this could return multiple specifiers combined with the
        //     new node_modules code. Possibly with local symlinks, which should be
        //     very rare.
        for specifier in specifiers {
            let specifier = specifier.to_string_lossy().into_owned();
            if specifier.contains("/node_modules/") {
                continue;
            }
            if let Some(cache) = cache {
                cache
                    .lock()
                    .expect("specifier cache")
                    .insert(export.path.clone(), specifier.clone());
            }
            return Ok((specifier, kind));
        }
        if let Some(cache) = cache {
            cache
                .lock()
                .expect("specifier cache")
                .insert(export.path.clone(), String::new());
        }
        Ok((String::new(), None))
    }

    /// tsgo `CompareFixesForSorting`: by rank, then by specifier.
    pub(crate) fn compare_fixes_for_sorting(&self, a: &Fix, b: &Fix) -> Ordering {
        self.compare_fixes_for_ranking(a, b).then_with(|| {
            // Sort ./foo before ../foo for equal-length specifiers
            let (a_specifier, b_specifier) = (
                &a.auto_import.module_specifier,
                &b.auto_import.module_specifier,
            );
            match (a_specifier.starts_with("./"), b_specifier.starts_with("./")) {
                (true, false) => return Ordering::Less,
                (false, true) => return Ordering::Greater,
                _ => {}
            }
            a_specifier.cmp(b_specifier).then_with(|| {
                a.auto_import
                    .import_kind
                    .0
                    .cmp(&b.auto_import.import_kind.0)
            })
        })
    }

    /// tsgo `CompareFixesForRanking`: by kind, then by specifier.
    pub(crate) fn compare_fixes_for_ranking(&self, a: &Fix, b: &Fix) -> Ordering {
        a.auto_import
            .kind
            .0
            .cmp(&b.auto_import.kind.0)
            .then_with(|| self.compare_module_specifiers_for_ranking(a, b))
    }

    /// tsgo `compareModuleSpecifiersForRanking`.
    /// tsgo `compareNodeCoreModuleSpecifiers`: a `node:` specifier before
    /// or after an unprefixed one as the importing file prefers.
    fn compare_node_core_module_specifiers(&self, a: &str, b: &str) -> Ordering {
        let (a_prefixed, b_prefixed) = (a.starts_with("node:"), b.starts_with("node:"));
        match (
            a_prefixed == b_prefixed,
            self.should_use_uri_style_node_core_modules,
        ) {
            (false, Some(uri_style)) => {
                if a_prefixed == uri_style {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            _ => Ordering::Equal,
        }
    }

    fn compare_module_specifiers_for_ranking(&self, a: &Fix, b: &Fix) -> Ordering {
        let relative = |fix: &Fix| fix.module_specifier_kind == Some(ModuleSpecifierKind::Relative);
        // tsgo `compareModuleSpecifierRelativity`: `-1` if `a` is better than `b`.
        let preference = self.preferences.import_module_specifier_preference;
        if preference == tsc_checker::ImportModuleSpecifierPreference::NON_RELATIVE
            || preference == tsc_checker::ImportModuleSpecifierPreference::PROJECT_RELATIVE
        {
            let comparison = relative(a).cmp(&relative(b));
            if comparison != Ordering::Equal {
                return comparison;
            }
        }
        let ambient = |fix: &Fix| fix.module_specifier_kind == Some(ModuleSpecifierKind::Ambient);
        if ambient(a) && ambient(b) {
            let comparison = self.compare_node_core_module_specifiers(
                &a.auto_import.module_specifier,
                &b.auto_import.module_specifier,
            );
            if comparison != Ordering::Equal {
                return comparison;
            }
        }
        if relative(a) && relative(b) {
            let comparison = self
                .is_fix_possibly_re_exporting_importing_file(a, &self.importing_file_name)
                .cmp(
                    &self.is_fix_possibly_re_exporting_importing_file(b, &self.importing_file_name),
                );
            if comparison != Ordering::Equal {
                return comparison;
            }
        }
        // tsgo `tspath.CompareNumberOfDirectorySeparators`.
        let separators = |specifier: &str| specifier.matches('/').count();
        separators(&a.auto_import.module_specifier)
            .cmp(&separators(&b.auto_import.module_specifier))
    }

    /// tsgo `isFixPossiblyReExportingImportingFile`: a re-export from an
    /// index file in a directory containing the importing file (an import
    /// cycle with a barrel).
    fn is_fix_possibly_re_exporting_importing_file(
        &self,
        fix: &Fix,
        importing_file_name: &str,
    ) -> bool {
        if fix.is_re_export && is_index_file_name(&fix.module_file_name) {
            let directory =
                tsc_program::get_directory_path(tsc_types::JsStr::from_str(&fix.module_file_name))
                    .to_string_lossy()
                    .into_owned();
            let case_sensitive = self.prepared.path_context().use_case_sensitive_file_names();
            return starts_with_directory(importing_file_name, &directory, case_sensitive);
        }
        false
    }
}

/// tsgo `getImportKind` (and `GetImportKindForImportStatement` with
/// `force_import_keyword`): how a file imports an export.
pub(crate) fn get_import_kind(
    checker: &CheckerState<'_>,
    importing_file: usize,
    export: &Export,
    force_import_keyword: bool,
) -> ImportKind {
    let source = checker.binder.source(importing_file);
    if checker.options.verbatim_module_syntax == Some(true)
        && checker.emit_module_format_of_file(source.root) == 1
    {
        return ImportKind::COMMON_JS;
    }
    match export.syntax {
        ExportSyntax::DefaultModifier | ExportSyntax::DefaultDeclaration => ImportKind::DEFAULT,
        ExportSyntax::Named if export.id.export_name == InternalSymbolName::DEFAULT => {
            ImportKind::DEFAULT
        }
        ExportSyntax::Named
        | ExportSyntax::Modifier
        | ExportSyntax::Star
        | ExportSyntax::CommonJsExportsProperty => ImportKind::NAMED,
        ExportSyntax::Equals | ExportSyntax::CommonJsModuleExports | ExportSyntax::Umd => {
            // export.Syntax will be ExportSyntaxEquals for named exports/properties of an export='s target.
            if export.id.export_name != InternalSymbolName::EXPORT_EQUALS {
                return ImportKind::NAMED;
            }
            // `import foo` parses as an ImportEqualsDeclaration even though it could be an ImportDeclaration
            for &statement in statements(source, source.root) {
                if let NodeData::ImportEqualsDeclaration(data) = &source.arena.node(statement).data
                {
                    if data
                        .module_reference
                        .is_some_and(|reference| !is_node_missing(source, reference))
                    {
                        return ImportKind::COMMON_JS;
                    }
                }
            }
            if source.external_module_indicator.is_some()
                || force_import_keyword
                || !is_source_file_js(source)
            {
                ImportKind::DEFAULT
            } else {
                ImportKind::COMMON_JS
            }
        }
        ExportSyntax::None => panic!("unhandled export syntax kind: ExportSyntaxNone"),
    }
}

/// tsgo `getAddAsTypeOnly`.
fn get_add_as_type_only(
    is_valid_type_only_use_site: bool,
    export: &Export,
    checker: &CheckerState<'_>,
) -> AddAsTypeOnly {
    if !is_valid_type_only_use_site {
        // Can't use a type-only import if the usage is an emitting position
        return AddAsTypeOnly::NOT_ALLOWED;
    }
    let value = export.flags.intersects(SymbolFlags::VALUE);
    // A type-only import is required for this symbol if under verbatimModuleSyntax and it's purely a type
    let required = if checker.options.verbatim_module_syntax == Some(true) {
        export.is_type_only || !value
    } else {
        export.is_type_only && value
    };
    if required {
        return AddAsTypeOnly::REQUIRED;
    }
    AddAsTypeOnly::ALLOWED
}

/// tsgo `getNamespaceLikeImportText`.
fn namespace_like_import_text(source: &SourceFile, declaration: NodeId) -> String {
    let identifier_text = |node: NodeId| match &source.arena.node(node).data {
        NodeData::Identifier(data) => data.text().to_owned(),
        _ => String::new(),
    };
    match &source.arena.node(declaration).data {
        NodeData::VariableDeclaration(data) => data
            .name
            .filter(|&name| source.arena.node(name).kind == SyntaxKind::Identifier)
            .map_or_else(String::new, identifier_text),
        NodeData::ImportEqualsDeclaration(data) => {
            data.name.map_or_else(String::new, identifier_text)
        }
        NodeData::ImportDeclaration(_) | NodeData::JSDocImportTag(_) => {
            let clause = match &source.arena.node(declaration).data {
                NodeData::ImportDeclaration(data) => data.import_clause,
                NodeData::JSDocImportTag(data) => data.import_clause,
                _ => None,
            };
            clause
                .and_then(|clause| match &source.arena.node(clause).data {
                    NodeData::ImportClause(clause) => clause.named_bindings,
                    _ => None,
                })
                .and_then(|bindings| match &source.arena.node(bindings).data {
                    NodeData::NamespaceImport(namespace) => namespace.name,
                    _ => None,
                })
                .map_or_else(String::new, identifier_text)
        }
        _ => String::new(),
    }
}

/// An import declaration's (or JSDoc import tag's) clause and module
/// specifier.
fn import_clause_and_specifier(source: &SourceFile, node: NodeId) -> Option<(NodeId, NodeId)> {
    match &source.arena.node(node).data {
        NodeData::ImportDeclaration(data) => Some((data.import_clause?, data.module_specifier?)),
        NodeData::JSDocImportTag(data) => Some((data.import_clause?, data.module_specifier?)),
        _ => None,
    }
}

/// tsgo `TryGetImportFromModuleSpecifier`.
pub(super) fn try_get_import_from_module_specifier(
    source: &SourceFile,
    specifier: NodeId,
) -> Option<NodeId> {
    let parent = source.arena.node(specifier).parent?;
    match source.arena.node(parent).kind {
        SyntaxKind::ImportDeclaration
        | SyntaxKind::ExportDeclaration
        | SyntaxKind::JSDocImportTag => Some(parent),
        SyntaxKind::ExternalModuleReference => source.arena.node(parent).parent,
        SyntaxKind::CallExpression => Some(parent),
        SyntaxKind::LiteralType => {
            if source.arena.node(specifier).kind != SyntaxKind::StringLiteral {
                return None;
            }
            let grandparent = source.arena.node(parent).parent?;
            (source.arena.node(grandparent).kind == SyntaxKind::ImportType).then_some(grandparent)
        }
        _ => None,
    }
}

/// tsgo `IsVariableDeclarationInitializedToRequire`.
pub(super) fn is_variable_declaration_initialized_to_require(
    source: &SourceFile,
    node: NodeId,
) -> bool {
    let node = if source.arena.node(node).kind == SyntaxKind::BindingElement {
        source
            .arena
            .node(node)
            .parent
            .and_then(|pattern| source.arena.node(pattern).parent)
            .unwrap_or(node)
    } else {
        node
    };
    if !is_source_file_js(source) {
        return false;
    }
    let NodeData::VariableDeclaration(data) = &source.arena.node(node).data else {
        return false;
    };
    data.initializer
        .is_some_and(|initializer| crate::utilities::is_require_call(source, initializer, true))
}

/// A string literal's text.
fn literal_text(source: &SourceFile, node: NodeId) -> String {
    match &source.arena.node(node).data {
        NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
        NodeData::NoSubstitutionTemplateLiteral(data) => data.text.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

/// tsgo `NodeIsMissing`.
fn is_node_missing(source: &SourceFile, node: NodeId) -> bool {
    let record = source.arena.node(node);
    record.pos == record.end && record.pos != u32::MAX && record.kind != SyntaxKind::EndOfFileToken
}

/// tsgo `IsSourceFileJS`.
fn is_source_file_js(source: &SourceFile) -> bool {
    matches!(
        source.script_kind,
        tsc_syntax::ScriptKind::Js | tsc_syntax::ScriptKind::Jsx
    )
}

/// tsgo `RootedFilePath.HasJSFileExtension`.
fn has_js_file_extension(file_name: &str) -> bool {
    [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| file_name.ends_with(extension))
}

/// tsgo `isIndexFileName`.
fn is_index_file_name(file_name: &str) -> bool {
    let base = file_name.rsplit('/').next().unwrap_or(file_name);
    matches!(
        base,
        "index.js" | "index.jsx" | "index.d.ts" | "index.ts" | "index.tsx"
    )
}

/// tsgo `CaseSensitivity.StartsWithDirectory`.
fn starts_with_directory(file_name: &str, directory: &str, case_sensitive: bool) -> bool {
    let (file_name, directory) = if case_sensitive {
        (file_name.to_owned(), directory.to_owned())
    } else {
        (file_name.to_lowercase(), directory.to_lowercase())
    };
    let directory = directory.trim_end_matches('/');
    file_name.len() > directory.len()
        && file_name.starts_with(directory)
        && file_name.as_bytes()[directory.len()] == b'/'
}

/// tsgo `RelativePathFromDirectory(...).AsModuleSpecifier()`: a `./` or
/// `../` path.
fn relative_path_from_directory(
    directory: &str,
    file_name: &str,
    case_sensitive: bool,
) -> Option<String> {
    let relative = tsc_program::relative_path_from_directory(
        tsc_types::JsStr::from_str(directory),
        tsc_types::JsStr::from_str(file_name),
        case_sensitive,
    )
    .to_string_lossy()
    .into_owned();
    // No shared root: the file's own absolute path.
    if relative.starts_with('/') {
        return None;
    }
    Some(if relative.starts_with("../") {
        relative
    } else {
        format!("./{relative}")
    })
}

/// tsgo `fileSyntaxKind`.
enum FileSyntax {
    Ambiguous,
    Esm,
    Cjs,
}

/// tsgo `detectSyntax`.
fn detect_syntax(checker: &CheckerState<'_>, file: usize) -> FileSyntax {
    let source = checker.binder.source(file);
    let has_cjs = checker.binder.is_common_js_module_of_node(source.root);
    let force = checker.options.emit_module_detection_kind() == 3;
    let has_esm = if !force {
        // ExternalModuleIndicator is reliable when moduleDetection is not "force"
        source.external_module_indicator.is_some()
    } else if source
        .external_module_indicator
        .is_some_and(|indicator| indicator != source.root)
    {
        true
    } else {
        // Fall back to scanning Imports() for actual import/export declarations
        // (not require() calls or dynamic imports).
        collect_external_module_references(source)
            .imports
            .iter()
            .any(|&import| {
                source.arena.node(import).parent.is_some_and(|parent| {
                    matches!(
                        source.arena.node(parent).kind,
                        SyntaxKind::ImportDeclaration
                            | SyntaxKind::ExportDeclaration
                            | SyntaxKind::ExternalModuleReference
                    )
                })
            })
    };
    match (has_esm, has_cjs) {
        (false, true) => FileSyntax::Cjs,
        (true, false) => FileSyntax::Esm,
        _ => FileSyntax::Ambiguous,
    }
}

/// tsgo's `core.ResolutionMode` number as a resolution mode.
fn resolution_mode(mode: u32) -> tsc_program::ResolutionMode {
    match mode {
        1 => tsc_program::ResolutionMode::CommonJs,
        99 => tsc_program::ResolutionMode::EsNext,
        _ => tsc_program::ResolutionMode::Unspecified,
    }
}

/// tsgo `lsutil.ShouldUseUriStyleNodeCoreModules`: whether the file's first
/// import of a Node core module (that has an unprefixed name) is `node:`
/// prefixed. tsgo's program never records a fallback, so a file importing
/// none has no preference.
fn should_use_uri_style_node_core_modules(source: &SourceFile) -> Option<bool> {
    let unprefixed = tsc_checker::modules::UNPREFIXED_NODE_CORE_MODULES;
    collect_external_module_references(source)
        .imports
        .into_iter()
        .find_map(|import| {
            let NodeData::StringLiteral(literal) = &source.arena.node(import).data else {
                return None;
            };
            let text = literal.text.to_string_lossy();
            match text.strip_prefix("node:") {
                Some(name) => unprefixed.contains(&name).then_some(true),
                None => unprefixed.contains(&text.as_ref()).then_some(false),
            }
        })
}

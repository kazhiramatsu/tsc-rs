//! tsgo `ls/string_completions.go`'s path completions: the files, the
//! directories and the module names a module specifier or a triple-slash
//! reference may name (`getStringLiteralCompletionsFromModuleNames`,
//! `getTripleSlashReferenceCompletions`).
//!
//! tsc-rs's program keeps no host once loaded, so tsgo's `Program`
//! questions about package.json files (`GetPackageJsonInfo`,
//! `GetNearestAncestorDirectoryWithPackageJson`) read the manifests through
//! the language service's host, as tsgo's program reads them through its
//! own.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use tsc_checker::state::CheckResult;
use tsc_checker::{ImportModuleSpecifierEndingPreference, ModuleSpecifierEnding};
use tsc_diagnostics::{DocumentVersion, JsStr, TextSnapshot};
use tsc_emitter::EmitResolutionMode;
use tsc_lsproto::Range;
use tsc_program::{JsonObject, JsonValue, ResolutionMode};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{NodeFlags, SymbolId};

use crate::lsutil::{ScriptElementKind, ScriptElementKindModifier};

use super::context::Completions;
use super::{default_commit_characters, items, sort_text, ClientSupport, CompletionList};

/// tsgo `pathCompletion`.
pub(super) struct PathCompletion {
    name: String,
    kind: ScriptElementKind,
    extension: String,
}

/// tsgo `pathCompletions`.
pub(super) struct PathCompletions {
    entries: Vec<PathCompletion>,
    replacement_span: Option<Range>,
}

/// tsgo `moduleCompletionKind`, in the order `add` prefers.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ModuleCompletionKind {
    Directory,
    File,
    ExternalModuleName,
}

/// tsgo `moduleCompletionNameAndKind`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ModuleCompletion {
    name: String,
    kind: ModuleCompletionKind,
    extension: String,
}

impl ModuleCompletion {
    fn new(name: impl Into<String>, kind: ModuleCompletionKind) -> Self {
        Self {
            name: name.into(),
            kind,
            extension: String::new(),
        }
    }
}

/// tsgo `moduleCompletionNameAndKindSet`: a completion a name, of the
/// highest kind (tsgo's map; the client orders the items).
#[derive(Default)]
struct ModuleCompletions(BTreeMap<String, ModuleCompletion>);

impl ModuleCompletions {
    fn add(&mut self, entry: ModuleCompletion) {
        if self
            .0
            .get(&entry.name)
            .is_some_and(|existing| existing.kind >= entry.kind)
        {
            return;
        }
        self.0.insert(entry.name.clone(), entry);
    }

    fn into_values(self) -> Vec<ModuleCompletion> {
        self.0.into_values().collect()
    }
}

/// tsgo `referenceKind`.
#[derive(Clone, Copy, Eq, PartialEq)]
enum ReferenceKind {
    FileName,
    ModuleSpecifier,
}

/// tsgo `extensionOptions` (the importing file is the request's).
struct ExtensionOptions {
    extensions_to_search: Vec<String>,
    reference_kind: ReferenceKind,
    ending_preference: ImportModuleSpecifierEndingPreference,
    /// tsgo `core.ResolutionMode` (0, 1 for CommonJS, 99 for ESM).
    resolution_mode: u32,
}

/// tsgo `core.Pattern`: a text with at most one `*`.
struct Pattern {
    text: String,
    star_index: Option<usize>,
    valid: bool,
}

/// tsgo `core.TryParsePattern`.
fn try_parse_pattern(pattern: &str) -> Pattern {
    match pattern.find('*') {
        Some(star) if pattern[star + 1..].contains('*') => Pattern {
            text: String::new(),
            star_index: Some(0),
            valid: false,
        },
        star_index => Pattern {
            text: pattern.to_owned(),
            star_index,
            valid: true,
        },
    }
}

impl Pattern {
    /// tsgo `Pattern.Matches`.
    fn matches(&self, candidate: &str) -> bool {
        let Some(star) = self.star_index else {
            return self.text == candidate;
        };
        candidate.len() + 1 >= self.text.len()
            && candidate.starts_with(&self.text[..star])
            && candidate.ends_with(&self.text[star + 1..])
    }
}

impl Completions<'_, '_, '_> {
    /// tsgo `getStringLiteralCompletionsFromModuleNames`.
    pub(super) fn string_literal_completions_from_module_names(
        &mut self,
        node: NodeId,
    ) -> CheckResult<PathCompletions> {
        let text = self.literal_text(node);
        let text_start = self.node_start(node) as usize + 1;
        let replacement_span = directory_fragment_range(&text, text_start)
            .map(|range| self.request.lsp_range(self.source, range));
        let names = self.module_names(node, &text)?;
        Ok(PathCompletions {
            entries: to_path_completions(names),
            replacement_span,
        })
    }

    /// tsgo `convertPathCompletions`.
    pub(super) fn convert_path_completions(
        &self,
        completion: Option<PathCompletions>,
    ) -> Option<CompletionList> {
        let completion = completion?;
        // A path that does not exist yet is a new identifier.
        let default_commit_characters = default_commit_characters(true);
        let file_name = self.file_name();
        let mut items = completion
            .entries
            .into_iter()
            .map(|entry| {
                let mut detail = entry.name.clone();
                if !entry.name.ends_with(&entry.extension) {
                    detail.push_str(&entry.extension);
                }
                items::create_lsp_completion_item(
                    self.text(),
                    &file_name,
                    self.position,
                    items::ItemSpec {
                        name: entry.name,
                        sort_text: sort_text::LOCATION_PRIORITY.to_owned(),
                        element_kind: Some(entry.kind),
                        kind_modifiers: kind_modifiers_from_extension(&entry.extension),
                        replacement_span: completion.replacement_span.clone(),
                        detail: Some(detail),
                        ..items::ItemSpec::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        let item_defaults = items::set_item_defaults(
            ClientSupport(self.request.capabilities),
            position,
            &mut items,
            Some(default_commit_characters),
            None,
        );
        Some(CompletionList {
            is_incomplete: false,
            item_defaults,
            items,
            symbols: Vec::new(),
        })
    }

    /// tsgo `Node.Text()` of a string literal: its value.
    fn literal_text(&self, node: NodeId) -> String {
        match self.data(node) {
            NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                data.text.to_string_lossy().into_owned()
            }
            _ => String::new(),
        }
    }

    /// tsgo `getStringLiteralCompletionsFromModuleNamesWorker`.
    fn module_names(&mut self, node: NodeId, text: &str) -> CheckResult<Vec<ModuleCompletion>> {
        let literal_value = normalize_slashes(text);
        let mode = if super::context::is_string_literal_like(self.node_kind(node)) {
            self.request.state.api_mode_for_usage_location(node)
        } else {
            0
        };
        let script_path = self.script_path();
        let script_directory = get_directory_path(&self.file_name());
        let extension_options = self.extension_options(ReferenceKind::ModuleSpecifier, mode, true);
        let no_paths = self
            .request
            .prepared
            .program_options()
            .paths()
            .is_none_or(<[_]>::is_empty);
        if is_path_relative_to_script(&literal_value)
            || no_paths && (is_rooted_disk_path(&literal_value) || is_url(&literal_value))
        {
            return Ok(self.relative_modules(
                &literal_value,
                &script_directory,
                &script_path,
                &extension_options,
            ));
        }
        Ok(self.non_relative_modules(&literal_value, &script_directory, mode, &extension_options))
    }

    /// tsgo `getCompletionEntriesForNonRelativeModules`: the modules of
    /// `paths`, the ambient modules, the typings and the packages.
    fn non_relative_modules(
        &mut self,
        fragment: &str,
        script_directory: &str,
        mode: u32,
        extension_options: &ExtensionOptions,
    ) -> Vec<ModuleCompletion> {
        let mut result = ModuleCompletions::default();
        let options = &self.request.state.options;
        let module_resolution = options.emit_module_resolution_kind();
        let resolve_package_json_exports = options.resolve_package_json_exports_effective();
        let resolve_package_json_imports = options.resolve_package_json_imports_effective();
        let conditions = tsc_program::get_conditions(options, resolution_mode(mode));
        let program_options = self.request.prepared.program_options();
        if let Some(paths) = program_options.paths().filter(|paths| !paths.is_empty()) {
            let base = program_options
                .paths_base_path()
                .map(|base| base.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.base_directory());
            let keys = paths
                .iter()
                .map(|mapping| mapping.pattern().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let patterns = paths
                .iter()
                .map(|mapping| {
                    (
                        mapping.pattern().to_string_lossy().into_owned(),
                        mapping
                            .substitutions()
                            .iter()
                            .map(|substitution| substitution.to_string_lossy().into_owned())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            self.add_completion_entries_from_paths(
                &mut result,
                fragment,
                &base,
                extension_options,
                keys,
                &|key| {
                    patterns
                        .iter()
                        .find(|(pattern, _)| pattern == key)
                        .map(|(_, substitutions)| substitutions.clone())
                        .unwrap_or_default()
                },
            );
        }

        let fragment_directory = fragment_directory(fragment);
        for ambient_name in self.ambient_module_completions(fragment, &fragment_directory) {
            result.add(ModuleCompletion::new(
                ambient_name,
                ModuleCompletionKind::ExternalModuleName,
            ));
        }

        self.completion_entries_from_typings(
            script_directory,
            &fragment_directory,
            extension_options,
            &mut result,
        );

        if module_resolution_uses_node_modules(module_resolution) {
            // A global package name takes the dependencies, not everything
            // in `node_modules` (dependencies' own dependencies), unless
            // there is none (no package.json).
            let mut found_global = false;
            if fragment_directory.is_empty() {
                for module_name in self.enumerate_node_modules_visible_to_script(script_directory) {
                    if !result.0.contains_key(&module_name) {
                        found_global = true;
                        result.add(ModuleCompletion::new(
                            module_name,
                            ModuleCompletionKind::ExternalModuleName,
                        ));
                    }
                }
            }
            if !found_global {
                let mut lookup = AncestorLookup {
                    fragment,
                    fragment_directory: &fragment_directory,
                    extension_options,
                    conditions: &conditions,
                    resolve_package_json_exports,
                    resolve_package_json_imports,
                    seen_package_scope: false,
                };
                for ancestor in self.ancestors_stopping_at_global_cache(script_directory) {
                    lookup.ancestor(self, &ancestor, &mut result);
                }
            }
        }
        result.into_values()
    }

    /// tsgo `getAmbientModuleCompletions`: the ambient module names that
    /// start with the fragment, less its directory, and no pattern.
    fn ambient_module_completions(&self, fragment: &str, fragment_directory: &str) -> Vec<String> {
        let mut names = Vec::new();
        for symbol in self.request.state.get_ambient_modules() {
            let name = self.ambient_module_name(symbol);
            if name.starts_with(fragment) && !name.contains('*') {
                names.push(name);
            }
        }
        if !fragment_directory.is_empty() {
            let prefix = ensure_trailing_directory_separator(fragment_directory);
            for name in &mut names {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    *name = rest.to_owned();
                }
            }
        }
        names
    }

    /// tsgo `getAmbientModuleName`: the name of the module declaration
    /// that is no augmentation, else the symbol's name without its quotes.
    fn ambient_module_name(&self, symbol: SymbolId) -> String {
        let binder = &self.request.state.binder;
        let declaration = binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                !self.is_external_module_augmentation(declaration)
                    && !NodeFlags::from_bits(binder.node_record(declaration).flags)
                        .contains(NodeFlags::GLOBAL_AUGMENTATION)
            });
        if let Some(declaration) = declaration {
            if let NodeData::ModuleDeclaration(module) = self.data(declaration) {
                if let Some(name) = module.name {
                    if let NodeData::StringLiteral(literal) = self.data(name) {
                        return literal.text.to_string_lossy().into_owned();
                    }
                }
            }
        }
        strip_quotes(&self.raw_symbol_name(symbol)).to_owned()
    }

    /// tsgo `ast.IsExternalModuleAugmentation`: an ambient module that
    /// augments a module (at the top of a module, or in an ambient module
    /// at the top of a script).
    fn is_external_module_augmentation(&self, node: NodeId) -> bool {
        if !self.is_ambient_module(node) {
            return false;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.node_kind(parent) {
            SyntaxKind::SourceFile => self.is_external_module_file(parent),
            SyntaxKind::ModuleBlock => {
                let Some(grandparent) = self.parent_of(parent) else {
                    return false;
                };
                self.is_ambient_module(grandparent)
                    && self.parent_of(grandparent).is_some_and(|file| {
                        self.node_kind(file) == SyntaxKind::SourceFile
                            && !self.is_external_module_file(file)
                    })
            }
            _ => false,
        }
    }

    /// tsgo `ast.IsAmbientModule`.
    fn is_ambient_module(&self, node: NodeId) -> bool {
        let NodeData::ModuleDeclaration(module) = self.data(node) else {
            return false;
        };
        module
            .name
            .is_some_and(|name| self.node_kind(name) == SyntaxKind::StringLiteral)
            || NodeFlags::from_bits(self.record(node).flags)
                .contains(NodeFlags::GLOBAL_AUGMENTATION)
    }

    /// tsgo `ast.IsExternalModule` of a file node.
    fn is_external_module_file(&self, file: NodeId) -> bool {
        let index = self.request.state.binder.file_index_of_node(file);
        self.request
            .state
            .binder
            .source(index)
            .external_module_indicator
            .is_some()
    }

    /// tsgo `getCompletionEntriesFromTypings`: the packages of the type
    /// roots and of each ancestor's `node_modules/@types`.
    fn completion_entries_from_typings(
        &self,
        script_directory: &str,
        fragment_directory: &str,
        extension_options: &ExtensionOptions,
        result: &mut ModuleCompletions,
    ) {
        let mut seen = HashSet::new();
        for root in self.effective_type_roots() {
            self.completion_entries_from_typings_directory(
                &root,
                fragment_directory,
                extension_options,
                &mut seen,
                result,
            );
        }
        for directory in self.ancestors_stopping_at_global_cache(script_directory) {
            let types = resolve_directory(&directory, "node_modules/@types");
            self.completion_entries_from_typings_directory(
                &types,
                fragment_directory,
                extension_options,
                &mut seen,
                result,
            );
        }
    }

    /// tsgo `getCompletionEntriesFromTypingsDirectories`.
    fn completion_entries_from_typings_directory(
        &self,
        directory: &str,
        fragment_directory: &str,
        extension_options: &ExtensionOptions,
        seen: &mut HashSet<String>,
        result: &mut ModuleCompletions,
    ) {
        let host = self.request.host;
        if !host.directory_exists(directory) {
            return;
        }
        let types = self.request.state.options.types.as_deref().unwrap_or(&[]);
        for type_directory_name in host.get_directories(directory) {
            let package_name = unmangle_scoped_package_name(&type_directory_name);
            if !types.is_empty()
                && !types
                    .iter()
                    .any(|name| name.to_string_lossy() == package_name.as_str())
            {
                continue;
            }
            if fragment_directory.is_empty() {
                if seen.insert(package_name.clone()) {
                    result.add(ModuleCompletion::new(
                        package_name,
                        ModuleCompletionKind::ExternalModuleName,
                    ));
                }
            } else {
                let base_directory = resolve_directory(directory, &type_directory_name);
                if let Some(remaining) = try_remove_directory_prefix(
                    fragment_directory,
                    &package_name,
                    self.case_sensitive(),
                ) {
                    self.completion_entries_for_directory_fragment(
                        &remaining,
                        &base_directory,
                        extension_options,
                        false,
                        "",
                        result,
                    );
                }
            }
        }
    }

    /// tsgo `enumerateNodeModulesVisibleToScript`: the dependencies of the
    /// package.json files above the script, but `@types` packages.
    fn enumerate_node_modules_visible_to_script(&self, script_directory: &str) -> Vec<String> {
        let mut result = Vec::new();
        for directory in self.ancestors_stopping_at_global_cache(script_directory) {
            let Some(package) = self.package_json_info(&directory) else {
                continue;
            };
            for field in [
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
            ] {
                let Some(JsonValue::Object(dependencies)) =
                    tsc_program::package_json_own_property(&package, field)
                else {
                    continue;
                };
                // tsgo reads the field as a map of strings.
                if !tsc_program::package_json_entries_in_order(dependencies)
                    .all(|(_, version)| matches!(version, JsonValue::String(_)))
                {
                    continue;
                }
                for (name, _) in tsc_program::package_json_entries_in_order(dependencies) {
                    let name = name.to_string_lossy().into_owned();
                    if !name.starts_with("@types/") {
                        result.push(name);
                    }
                }
            }
        }
        result
    }

    /// tsgo `getExtensionOptions`.
    fn extension_options(
        &self,
        reference_kind: ReferenceKind,
        resolution_mode: u32,
        with_checker: bool,
    ) -> ExtensionOptions {
        ExtensionOptions {
            extensions_to_search: self.supported_extensions_for_module_resolution(with_checker),
            reference_kind,
            ending_preference: self.request.preferences.import_module_specifier_ending,
            resolution_mode,
        }
    }

    /// tsgo `getSupportedExtensionsForModuleResolution`: the extensions of
    /// the ambient `*.ext` modules, then the program's, with `.json` for a
    /// `node_modules` resolver that resolves it.
    fn supported_extensions_for_module_resolution(&self, with_checker: bool) -> Vec<String> {
        let mut extensions = Vec::new();
        if with_checker {
            for symbol in self.request.state.get_ambient_modules() {
                let name = self.ambient_module_name(symbol);
                if !name.starts_with("*.") || name.contains('/') {
                    continue;
                }
                extensions.push(name[1..].to_owned());
            }
        }
        let options = &self.request.state.options;
        for group in
            crate::utilities::supported_extensions_with(options, &self.content_mapper_extensions())
        {
            extensions.extend(group);
        }
        if module_resolution_uses_node_modules(options.emit_module_resolution_kind())
            && options.resolve_json_module_effective()
        {
            extensions.push(".json".to_owned());
        }
        extensions
    }

    /// tsgo `CommandLine().ContentMapperExtensions()`.
    fn content_mapper_extensions(&self) -> Vec<String> {
        self.request
            .prepared
            .program_options()
            .content_mapper_spec()
            .map(|spec| {
                spec.mappers
                    .iter()
                    .flat_map(|mapper| mapper.definition.extensions.iter().flatten().cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// tsgo `getCompletionEntriesForRelativeModules`.
    fn relative_modules(
        &self,
        literal_value: &str,
        script_directory: &str,
        script_path: &str,
        extension_options: &ExtensionOptions,
    ) -> Vec<ModuleCompletion> {
        let root_dirs = self.root_dirs();
        if !root_dirs.is_empty() {
            return self.completion_entries_for_directory_fragment_with_root_dirs(
                &root_dirs,
                literal_value,
                script_directory,
                script_path,
                extension_options,
            );
        }
        let mut result = ModuleCompletions::default();
        self.completion_entries_for_directory_fragment(
            literal_value,
            script_directory,
            extension_options,
            true,
            script_path,
            &mut result,
        );
        result.into_values()
    }

    /// tsgo `getCompletionEntriesForDirectoryFragmentWithRootDirs`: the
    /// fragment in every directory the `rootDirs` merge with the script's.
    fn completion_entries_for_directory_fragment_with_root_dirs(
        &self,
        root_dirs: &[String],
        fragment: &str,
        script_directory: &str,
        exclude: &str,
        extension_options: &ExtensionOptions,
    ) -> Vec<ModuleCompletion> {
        let case_sensitive = self.case_sensitive();
        // The script's directory relative to the root directory it is in.
        let relative_directory = root_dirs
            .iter()
            .find_map(|root| trim_contained_path(root, script_directory, case_sensitive))
            .unwrap_or_default();
        let mut directories = root_dirs
            .iter()
            .map(|root| {
                if relative_directory.is_empty() {
                    root.clone()
                } else {
                    resolve_directory(root, &relative_directory)
                }
            })
            .collect::<Vec<_>>();
        directories.push(script_directory.to_owned());
        let mut seen_directories = HashSet::new();
        directories.retain(|directory| seen_directories.insert(directory.clone()));

        let mut all = Vec::new();
        for base_directory in &directories {
            let mut result = ModuleCompletions::default();
            self.completion_entries_for_directory_fragment(
                fragment,
                base_directory,
                extension_options,
                true,
                exclude,
                &mut result,
            );
            all.extend(result.into_values());
        }
        // Each name, kind and extension once.
        let mut seen = HashSet::new();
        all.retain(|completion| {
            seen.insert((
                completion.name.clone(),
                completion.kind,
                completion.extension.clone(),
            ))
        });
        all
    }

    /// tsgo `getCompletionEntriesForDirectoryFragment`: the files and the
    /// directories of the fragment's directory (the client filters by the
    /// base name).
    fn completion_entries_for_directory_fragment(
        &self,
        fragment: &str,
        script_directory: &str,
        extension_options: &ExtensionOptions,
        module_specifier_is_relative: bool,
        exclude: &str,
        result: &mut ModuleCompletions,
    ) {
        let mut fragment = normalize_slashes(fragment);
        if !has_trailing_directory_separator(&fragment) {
            fragment = get_directory_path(&fragment);
        }
        if fragment.is_empty() {
            fragment = ".".to_owned();
        }
        let fragment = ensure_trailing_directory_separator(&fragment);
        let base_directory = resolve_directory(script_directory, &fragment);
        if !module_specifier_is_relative {
            // A package's `typesVersions` redirects the directory.
            if let Some(package_directory) =
                self.nearest_ancestor_directory_with_package_json(&base_directory)
            {
                let paths = self
                    .package_json_info(&package_directory)
                    .and_then(|package| version_paths(&package));
                if let Some(paths) = paths.filter(|paths| !paths.is_empty()) {
                    let mut path_in_package =
                        trim_contained_path(&package_directory, &base_directory, true)
                            .expect("the package directory is above the completion directory");
                    if !path_in_package.is_empty() {
                        path_in_package.push('/');
                    }
                    let keys = paths.iter().map(|(key, _)| key.clone()).collect();
                    if self.add_completion_entries_from_paths(
                        result,
                        &path_in_package,
                        &package_directory,
                        extension_options,
                        keys,
                        &|key| {
                            paths
                                .iter()
                                .find(|(candidate, _)| candidate == key)
                                .map(|(_, patterns)| patterns.clone())
                                .unwrap_or_default()
                        },
                    ) {
                        // A matched version path blocks the files and
                        // directories here; what it reaches is added.
                        return;
                    }
                }
            }
        }

        let host = self.request.host;
        if !host.directory_exists(&base_directory) {
            return;
        }
        let extensions = extension_options
            .extensions_to_search
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        for file_path in host.read_directory(&base_directory, &extensions, &[], &["./*"], None) {
            // No self-import.
            if exclude == self.path_key(&file_path) {
                continue;
            }
            let (name, extension) = self.file_name_with_extension_option(
                &get_base_file_name(&file_path),
                extension_options,
                false,
            );
            result.add(ModuleCompletion {
                name,
                kind: ModuleCompletionKind::File,
                extension,
            });
        }
        for directory in host.get_directories(&base_directory) {
            let directory_name = get_base_file_name(&directory);
            if directory_name != "@types" {
                result.add(ModuleCompletion::new(
                    directory_name,
                    ModuleCompletionKind::Directory,
                ));
            }
        }
    }

    /// tsgo `addCompletionEntriesFromPaths`: the `paths` (or `typesVersions`)
    /// mappings, the longest prefix first; whether the fragment matched one.
    fn add_completion_entries_from_paths(
        &self,
        result: &mut ModuleCompletions,
        fragment: &str,
        base_directory: &str,
        extension_options: &ExtensionOptions,
        keys: Vec<String>,
        patterns_for_key: &dyn Fn(&str) -> Vec<String>,
    ) -> bool {
        let compare_paths = |a: &str, b: &str| {
            let length = |key: &str| {
                let pattern = try_parse_pattern(key);
                pattern.star_index.unwrap_or(key.len())
            };
            length(b).cmp(&length(a))
        };
        self.add_completion_entries_from_paths_or_exports_or_imports(
            result,
            false,
            false,
            fragment,
            base_directory,
            extension_options,
            keys,
            patterns_for_key,
            &compare_paths,
        )
    }

    /// tsgo `addCompletionEntriesFromPathsOrExportsOrImports`: whether the
    /// fragment matched a key (then no other path completion applies).
    #[allow(clippy::too_many_arguments)]
    fn add_completion_entries_from_paths_or_exports_or_imports(
        &self,
        result: &mut ModuleCompletions,
        is_exports: bool,
        is_imports: bool,
        fragment: &str,
        base_directory: &str,
        extension_options: &ExtensionOptions,
        keys: Vec<String>,
        patterns_for_key: &dyn Fn(&str) -> Vec<String>,
        compare_paths: &dyn Fn(&str, &str) -> Ordering,
    ) -> bool {
        struct PathResult {
            results: Vec<ModuleCompletion>,
            matched: bool,
        }
        let mut path_results: Vec<PathResult> = Vec::new();
        let mut matched_path: Option<String> = None;
        for key in keys {
            if key == "." {
                continue;
            }
            let mut normalized_key = key.strip_prefix("./").unwrap_or(&key).to_owned();
            if (is_exports || is_imports) && key.ends_with('/') {
                normalized_key.push('*');
            }
            let patterns = patterns_for_key(&key);
            if patterns.is_empty() {
                continue;
            }
            let path_pattern = try_parse_pattern(&normalized_key);
            if !path_pattern.valid {
                continue;
            }
            let is_match = path_pattern.matches(fragment);
            let is_longest_match = is_match
                && matched_path.as_deref().is_none_or(|matched| {
                    compare_paths(&normalized_key, matched) == Ordering::Less
                });
            if is_longest_match {
                // A higher-priority match invalidates the results of the
                // earlier matches (pathCompletionsTypesVersionsWildcard6).
                matched_path = Some(normalized_key.clone());
                path_results.retain(|path_result| !path_result.matched);
            }
            if path_pattern.star_index.is_none()
                || matched_path.as_deref().is_none_or(|matched| {
                    compare_paths(&normalized_key, matched) != Ordering::Greater
                })
            {
                path_results.push(PathResult {
                    matched: is_match,
                    results: self.completions_for_path_mapping(
                        &normalized_key,
                        &patterns,
                        fragment,
                        base_directory,
                        is_exports,
                        is_imports,
                        extension_options,
                    ),
                });
            }
        }
        for path_result in path_results {
            for completion in path_result.results {
                result.add(completion);
            }
        }
        matched_path.is_some()
    }

    /// tsgo `getCompletionsForPathMapping`.
    #[allow(clippy::too_many_arguments)]
    fn completions_for_path_mapping(
        &self,
        path: &str,
        patterns: &[String],
        fragment: &str,
        package_directory: &str,
        is_exports: bool,
        is_imports: bool,
        extension_options: &ExtensionOptions,
    ) -> Vec<ModuleCompletion> {
        let mut fragment_directory = fragment_directory(fragment);
        if !fragment_directory.is_empty() {
            fragment_directory = ensure_trailing_directory_separator(&fragment_directory);
        }
        let just_path_mapping_name =
            |name: &str, kind: ModuleCompletionKind, extension: &str| -> Vec<ModuleCompletion> {
                if !name.starts_with(fragment) {
                    return Vec::new();
                }
                let mut name = remove_trailing_directory_separator(name);
                if !fragment_directory.is_empty() {
                    if let Some(rest) = name.strip_prefix(&fragment_directory) {
                        name = rest.to_owned();
                    }
                }
                vec![ModuleCompletion {
                    name,
                    kind,
                    extension: extension.to_owned(),
                }]
            };
        let parsed_path = try_parse_pattern(path);
        if !parsed_path.valid {
            return Vec::new();
        }
        let Some(star_index) = parsed_path.star_index else {
            // `"foo": ["/x/y/z.ts"]` completes `foo` itself.
            let pattern = patterns.first().map(String::as_str).unwrap_or_default();
            return just_path_mapping_name(
                path,
                ModuleCompletionKind::File,
                &file_extension(pattern),
            );
        };
        let path_prefix = &parsed_path.text[..star_index];
        let path_suffix = &parsed_path.text[star_index + 1..];
        let with_prefix_and_suffix = |prefix: &str, mut modules: Vec<ModuleCompletion>| {
            for module in &mut modules {
                let suffix = if module.kind == ModuleCompletionKind::File {
                    path_suffix
                } else {
                    ""
                };
                module.name = format!("{prefix}{}{suffix}", module.name);
            }
            modules
        };
        if !fragment.starts_with(path_prefix) {
            // The fragment cannot extend the mapping's prefix.
            if !path_prefix.starts_with(fragment) {
                return Vec::new();
            }
            if path.ends_with("/*") {
                return just_path_mapping_name(path_prefix, ModuleCompletionKind::Directory, "");
            }
            // `foo/bar/*` with `foo/b` leaves `bar/`.
            let remaining_directory_prefix =
                path_prefix.get(fragment_directory.len()..).unwrap_or("");
            let mut completions = Vec::new();
            for pattern in patterns {
                let modules = self.modules_for_paths_pattern(
                    "",
                    package_directory,
                    pattern,
                    is_exports,
                    is_imports,
                    extension_options,
                );
                completions.extend(with_prefix_and_suffix(remaining_directory_prefix, modules));
            }
            return completions;
        }
        let remaining_fragment = &fragment[path_prefix.len()..];
        let remaining_directory_fragment = if fragment_directory.starts_with(path_prefix) {
            ""
        } else {
            path_prefix.get(fragment_directory.len()..).unwrap_or("")
        };
        patterns
            .iter()
            .flat_map(|pattern| {
                let modules = self.modules_for_paths_pattern(
                    remaining_fragment,
                    package_directory,
                    pattern,
                    is_exports,
                    is_imports,
                    extension_options,
                );
                with_prefix_and_suffix(remaining_directory_fragment, modules)
            })
            .collect()
    }

    /// tsgo `getModulesForPathsPattern`: the files and the directories a
    /// pattern's `*` reaches, relative to its prefix and the fragment's
    /// directory.
    fn modules_for_paths_pattern(
        &self,
        fragment: &str,
        package_directory: &str,
        pattern: &str,
        is_exports: bool,
        is_imports: bool,
        extension_options: &ExtensionOptions,
    ) -> Vec<ModuleCompletion> {
        let parsed = try_parse_pattern(pattern);
        let Some(star_index) = parsed.star_index.filter(|_| parsed.valid) else {
            return Vec::new();
        };
        let prefix = &parsed.text[..star_index];
        let suffix = &parsed.text[star_index + 1..];

        // The prefix's directory and the part of its base name before the
        // `*` (`directory/path/of/prefix/base*`).
        let normalized_prefix = resolve_path(prefix);
        let (normalized_prefix_directory, normalized_prefix_base) =
            if has_trailing_directory_separator(prefix) {
                (normalized_prefix, String::new())
            } else {
                (
                    get_directory_path(&normalized_prefix),
                    get_base_file_name(&normalized_prefix),
                )
            };
        let fragment_has_path = fragment.contains('/');
        let fragment_directory = if !fragment_has_path {
            String::new()
        } else if has_trailing_directory_separator(fragment) {
            fragment.to_owned()
        } else {
            get_directory_path(fragment)
        };

        // The fragment's path narrows the listing.
        let expanded_prefix_directory = if fragment_has_path {
            combine_paths(
                &normalized_prefix_directory,
                &format!("{normalized_prefix_base}{fragment_directory}"),
            )
        } else {
            normalized_prefix_directory
        };
        let base_directory = resolve_directory(package_directory, &expanded_prefix_directory);

        let options = &self.request.state.options;
        let output_directory = |directory: Option<&tsc_types::JsString>| {
            directory.map(|directory| {
                self.possible_original_input_path(&base_directory, &directory.to_string_lossy())
            })
        };
        let (input_for_out_dir, input_for_declaration_dir) = if is_imports {
            (
                output_directory(options.out_dir.as_ref()),
                output_directory(options.declaration_dir.as_ref()),
            )
        } else {
            (None, None)
        };

        let normalized_suffix = normalize_path(suffix);
        let mut matching_suffixes = Vec::new();
        if !normalized_suffix.is_empty() {
            let probe = format!("_{normalized_suffix}");
            matching_suffixes.push(change_extension(
                &normalized_suffix,
                &declaration_emit_extension_for_path(&probe),
            ));
            for extension in possible_original_input_extension_for_extension(&probe) {
                matching_suffixes.push(change_extension(&normalized_suffix, &extension));
            }
        }
        matching_suffixes.push(normalized_suffix.clone());

        // A suffix reads the whole tree, so that only the directories that
        // lead to a matching file come back.
        let include_globs = if normalized_suffix.is_empty() {
            vec!["./*".to_owned()]
        } else {
            matching_suffixes
                .iter()
                .map(|suffix| format!("**/*{suffix}"))
                .collect()
        };
        let is_exports_or_imports_wildcard = (is_exports || is_imports) && pattern.ends_with("/*");

        let trim_prefix_and_suffix = |path: &str, prefix: &str| -> String {
            let normalized = normalize_path(path);
            for suffix in &matching_suffixes {
                if normalized.len() >= prefix.len() + suffix.len()
                    && normalized.starts_with(prefix)
                    && normalized.ends_with(suffix.as_str())
                {
                    let inner = &normalized[prefix.len()..normalized.len() - suffix.len()];
                    return inner.strip_prefix('/').unwrap_or(inner).to_owned();
                }
            }
            String::new()
        };
        let host = self.request.host;
        let extensions = extension_options
            .extensions_to_search
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let includes = include_globs.iter().map(String::as_str).collect::<Vec<_>>();
        let matches_with_prefix = |directory: &str| -> Vec<ModuleCompletion> {
            let complete_prefix = if fragment_has_path {
                directory.to_owned()
            } else {
                format!(
                    "{}{normalized_prefix_base}",
                    ensure_trailing_directory_separator(directory)
                )
            };
            let mut result = Vec::new();
            for found in host.read_directory(directory, &extensions, &[], &includes, None) {
                let trimmed = trim_prefix_and_suffix(&found, &complete_prefix);
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.contains('/') {
                    let components =
                        get_path_components(trimmed.strip_prefix('/').unwrap_or(&trimmed));
                    if let Some(name) = components.get(1) {
                        result.push(ModuleCompletion::new(
                            name.clone(),
                            ModuleCompletionKind::Directory,
                        ));
                    }
                } else {
                    let (name, mut extension) = self.file_name_with_extension_option(
                        &trimmed,
                        extension_options,
                        is_exports_or_imports_wildcard,
                    );
                    if extension.is_empty() {
                        extension = file_extension(&found);
                    }
                    result.push(ModuleCompletion {
                        name,
                        kind: ModuleCompletionKind::File,
                        extension,
                    });
                }
            }
            result
        };
        let directory_matches = |directory: &str| -> Vec<ModuleCompletion> {
            host.get_directories(directory)
                .into_iter()
                .filter(|name| name != "node_modules")
                .map(|name| ModuleCompletion::new(name, ModuleCompletionKind::Directory))
                .collect()
        };

        let mut matches = matches_with_prefix(&base_directory);
        for directory in [&input_for_out_dir, &input_for_declaration_dir]
            .into_iter()
            .flatten()
        {
            matches.extend(matches_with_prefix(directory));
        }
        // Without a suffix, any directory may have something to import.
        if normalized_suffix.is_empty() {
            matches.extend(directory_matches(&base_directory));
            for directory in [&input_for_out_dir, &input_for_declaration_dir]
                .into_iter()
                .flatten()
            {
                matches.extend(directory_matches(directory));
            }
        }
        matches
    }

    /// tsgo `getPossibleOriginalInputPathWithoutChangingExt`: a directory of
    /// the output directory, as the common source directory's.
    fn possible_original_input_path(&self, directory: &str, output_directory: &str) -> String {
        let output_directory = self.absolute(output_directory);
        let relative = tsc_program::relative_path_from_directory(
            JsStr::from_str(&output_directory),
            JsStr::from_str(directory),
            self.case_sensitive(),
        )
        .to_string_lossy()
        .into_owned();
        if encoded_root_length(&relative) != 0 {
            return directory.to_owned();
        }
        let common = self
            .request
            .prepared
            .common_source_directory()
            .to_string_lossy()
            .into_owned();
        resolve_directory(&common, &relative)
    }

    /// tsgo `getFilenameWithExtensionOption`: the name a file is imported
    /// by, and its extension.
    fn file_name_with_extension_option(
        &self,
        name: &str,
        extension_options: &ExtensionOptions,
        is_exports_or_imports_wildcard: bool,
    ) -> (String, String) {
        if let Some(non_js) = tsc_checker::try_get_real_file_name_for_non_js_declaration_file_name(
            JsStr::from_str(name),
        ) {
            let non_js = non_js.to_string_lossy().into_owned();
            let extension = try_get_extension_from_path(&non_js).to_owned();
            return (non_js, extension);
        }
        if extension_options.reference_kind == ReferenceKind::FileName {
            return (
                name.to_owned(),
                try_get_extension_from_path(name).to_owned(),
            );
        }
        let mut allowed_endings = self.request.state.get_allowed_endings_in_preferred_order(
            self.source.root,
            extension_options.ending_preference,
            emit_resolution_mode(extension_options.resolution_mode),
        );
        if is_exports_or_imports_wildcard {
            // `"exports": { "./*": "./src/*" }` takes a (maybe
            // extension-swapped) file name, no dropped extension or index.
            allowed_endings.retain(|ending| {
                !matches!(
                    ending,
                    ModuleSpecifierEnding::Minimal | ModuleSpecifierEnding::Index
                )
            });
        }
        let output_extension = |name: &str| {
            self.request
                .state
                .try_get_js_extension_for_file(JsStr::from_str(name))
        };
        if allowed_endings.first() == Some(&ModuleSpecifierEnding::TsExtension) {
            if file_extension_is_one_of(name, &[".ts", ".tsx", ".mts", ".cts"]) {
                return (
                    name.to_owned(),
                    try_get_extension_from_path(name).to_owned(),
                );
            }
            if let Some(extension) = output_extension(name) {
                return (change_extension(name, extension), extension.to_owned());
            }
            return (
                name.to_owned(),
                try_get_extension_from_path(name).to_owned(),
            );
        }
        if !is_exports_or_imports_wildcard
            && matches!(
                allowed_endings.first(),
                Some(ModuleSpecifierEnding::Minimal | ModuleSpecifierEnding::Index)
            )
            && file_extension_is_one_of(name, &[".js", ".jsx", ".ts", ".tsx", ".d.ts"])
        {
            return (
                remove_file_extension(name).to_owned(),
                try_get_extension_from_path(name).to_owned(),
            );
        }
        if let Some(extension) = output_extension(name) {
            return (change_extension(name, extension), extension.to_owned());
        }
        (
            name.to_owned(),
            try_get_extension_from_path(name).to_owned(),
        )
    }

    /// tsgo `isInReferenceComment`: whether the position is in a `/// <`
    /// comment.
    pub(super) fn is_in_reference_comment(&self) -> bool {
        let position = self.position;
        let token = self.token_at(position);
        let preceding = self.preceding_token(position);
        let Some((pos, end)) = self.enclosing_comment(position, preceding, token) else {
            return false;
        };
        let text = &self.text()[pos as usize..end as usize];
        text.starts_with("///") && text[3..].trim_start().starts_with('<')
    }

    /// tsgo `getTripleSlashReferenceCompletions`: the files of a `path`
    /// reference, or the typings of a `types` one.
    pub(super) fn triple_slash_reference_completions(&self) -> Option<PathCompletions> {
        let position = self.position as usize;
        let token = self.token_at(self.position);
        let found = tsc_syntax::comment_ranges(self.text(), self.pos(token) as usize, false)
            .into_iter()
            .find(|&(pos, end)| pos <= position && position <= end)?;
        let text = &self.text()[found.0..position];
        let (prefix, kind, to_complete) = parse_triple_slash_directive_fragment(text)?;
        let replacement_span = directory_fragment_range(to_complete, found.0 + prefix.len())
            .map(|range| self.request.lsp_range(self.source, range));
        let script_directory = get_directory_path(&self.file_name());
        let mut result = ModuleCompletions::default();
        match kind {
            "path" => {
                let extension_options = self.extension_options(ReferenceKind::FileName, 0, false);
                self.completion_entries_for_directory_fragment(
                    to_complete,
                    &script_directory,
                    &extension_options,
                    true,
                    &self.script_path(),
                    &mut result,
                );
            }
            _ => {
                let extension_options =
                    self.extension_options(ReferenceKind::ModuleSpecifier, 0, false);
                self.completion_entries_from_typings(
                    &script_directory,
                    &fragment_directory(to_complete),
                    &extension_options,
                    &mut result,
                );
            }
        }
        Some(PathCompletions {
            entries: to_path_completions(result.into_values()),
            replacement_span,
        })
    }

    /// tsgo `Program.GetPackageJsonInfo` of `directory/package.json`: the
    /// manifest there, read through the host.
    fn package_json_info(&self, directory: &str) -> Option<JsonObject> {
        let file_name = combine_paths(directory, "package.json");
        if !self.request.host.file_exists(&file_name) {
            return None;
        }
        let text = self.request.host.read_file(&file_name)?;
        Some(tsc_program::read_package_json_object_from_snapshot(
            JsStr::from_str(&file_name),
            &TextSnapshot::new(text, DocumentVersion::default()),
        ))
    }

    /// tsgo `Program.GetNearestAncestorDirectoryWithPackageJson`.
    fn nearest_ancestor_directory_with_package_json(&self, directory: &str) -> Option<String> {
        self.ancestors_stopping_at_global_cache(directory)
            .into_iter()
            .find(|ancestor| {
                self.request
                    .host
                    .file_exists(&combine_paths(ancestor, "package.json"))
            })
    }

    /// tsgo `ForEachAncestorDirectoryPathStoppingAtGlobalCache`'s
    /// directories: `directory` and its ancestors (tsc-rs's programs keep
    /// no global typings cache).
    fn ancestors_stopping_at_global_cache(&self, directory: &str) -> Vec<String> {
        let mut ancestors = Vec::new();
        let mut current = directory.to_owned();
        loop {
            ancestors.push(current.clone());
            let parent = get_directory_path(&current);
            if parent == current {
                break;
            }
            current = parent;
        }
        ancestors
    }

    /// tsgo `CompilerOptions.GetEffectiveTypeRoots(program.BaseDirectory())`.
    fn effective_type_roots(&self) -> Vec<String> {
        let program_options = self.request.prepared.program_options();
        if let Some(roots) = program_options.type_roots() {
            return roots
                .iter()
                .map(|root| root.display().to_string_lossy().into_owned())
                .collect();
        }
        let base = match program_options.config_file_path() {
            Some(config) => get_directory_path(&config.display().to_string_lossy()),
            None => self.base_directory(),
        };
        tsc_program::default_type_roots(JsStr::from_str(&base))
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect()
    }

    /// tsgo `CompilerOptions.GetEffectiveRootDirs`.
    fn root_dirs(&self) -> Vec<String> {
        self.request
            .prepared
            .program_options()
            .root_dirs()
            .unwrap_or(&[])
            .iter()
            .map(|root| root.display().to_string_lossy().into_owned())
            .collect()
    }

    /// tsgo `Program.BaseDirectory`.
    fn base_directory(&self) -> String {
        self.request
            .prepared
            .current_directory()
            .display()
            .to_string_lossy()
            .into_owned()
    }

    /// A path from the program's current directory.
    fn absolute(&self, path: &str) -> String {
        tsc_program::get_normalized_absolute_path(
            JsStr::from_str(path),
            JsStr::from_str(&self.base_directory()),
        )
        .to_string_lossy()
        .into_owned()
    }

    /// tsgo `SourceFile.PathKey` of the request's file.
    fn script_path(&self) -> String {
        self.path_key(&self.file_name())
    }

    /// tsgo `Program.PathKeyForFileName`.
    fn path_key(&self, file_name: &str) -> String {
        crate::utilities::path_key(self.request.prepared, file_name)
    }

    fn case_sensitive(&self) -> bool {
        self.request
            .prepared
            .path_context()
            .use_case_sensitive_file_names()
    }
}

/// The `node_modules` lookups of `getCompletionEntriesForNonRelativeModules`
/// at each ancestor directory.
struct AncestorLookup<'f> {
    fragment: &'f str,
    fragment_directory: &'f str,
    extension_options: &'f ExtensionOptions,
    conditions: &'f [String],
    resolve_package_json_exports: bool,
    resolve_package_json_imports: bool,
    seen_package_scope: bool,
}

impl AncestorLookup<'_> {
    /// tsgo's `ancestorLookup`: the package's `exports` for a fragment in
    /// a package, else the directory's `node_modules` and `imports`.
    fn ancestor(
        &mut self,
        completions: &Completions<'_, '_, '_>,
        ancestor: &str,
        result: &mut ModuleCompletions,
    ) {
        if self.fragment_directory.is_empty() || !self.resolve_package_json_exports {
            self.node_modules_directory_or_imports(completions, ancestor, result);
            return;
        }
        let mut components = get_path_components(self.fragment);
        components.remove(0);
        if components.is_empty() {
            self.node_modules_directory_or_imports(completions, ancestor, result);
            return;
        }
        let mut package_path = components.remove(0);
        if package_path.starts_with('@') {
            if components.is_empty() {
                self.node_modules_directory_or_imports(completions, ancestor, result);
                return;
            }
            let sub_name = components.remove(0);
            package_path = combine_paths(&package_path, &sub_name);
        }
        if self.resolve_package_json_imports && package_path.starts_with('#') {
            self.imports(completions, ancestor, result);
            return;
        }
        let package_directory =
            resolve_directory(ancestor, &combine_paths("node_modules", &package_path));
        if let Some(package) = completions.package_json_info(&package_directory) {
            let mut fragment_subpath = components.join("/");
            if !components.is_empty() && has_trailing_directory_separator(self.fragment) {
                fragment_subpath.push('/');
            }
            if self.exports_or_imports(
                completions,
                tsc_program::package_json_own_property(&package, "exports"),
                &fragment_subpath,
                &package_directory,
                true,
                false,
                result,
            ) {
                return;
            }
        }
        self.node_modules_directory_or_imports(completions, ancestor, result);
    }

    /// tsgo's first `ancestorLookup`: the directory's `node_modules`, then
    /// the `imports` of its package.
    fn node_modules_directory_or_imports(
        &mut self,
        completions: &Completions<'_, '_, '_>,
        ancestor: &str,
        result: &mut ModuleCompletions,
    ) {
        let node_modules = resolve_directory(ancestor, "node_modules");
        if completions.request.host.directory_exists(&node_modules) {
            completions.completion_entries_for_directory_fragment(
                self.fragment,
                &node_modules,
                self.extension_options,
                false,
                "",
                result,
            );
        }
        self.imports(completions, ancestor, result);
    }

    /// tsgo `importsLookup`: the `imports` of the nearest package, once.
    fn imports(
        &mut self,
        completions: &Completions<'_, '_, '_>,
        directory: &str,
        result: &mut ModuleCompletions,
    ) {
        if !self.resolve_package_json_imports || self.seen_package_scope {
            return;
        }
        let Some(package) = completions.package_json_info(directory) else {
            return;
        };
        self.seen_package_scope = true;
        let fragment = self.fragment;
        self.exports_or_imports(
            completions,
            tsc_program::package_json_own_property(&package, "imports"),
            fragment,
            directory,
            false,
            true,
            result,
        );
    }

    /// tsgo `exportsOrImportsLookup`: the completions of an `exports` or
    /// `imports` map; whether the search stops (a present field).
    #[allow(clippy::too_many_arguments)]
    fn exports_or_imports(
        &self,
        completions: &Completions<'_, '_, '_>,
        lookup_table: Option<&JsonValue>,
        fragment: &str,
        base_directory: &str,
        is_exports: bool,
        is_imports: bool,
        result: &mut ModuleCompletions,
    ) -> bool {
        let Some(lookup_table) = lookup_table else {
            return false;
        };
        let JsonValue::Object(table) = lookup_table else {
            return true;
        };
        let keys = tsc_program::package_json_entries_in_order(table)
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let conditions = self.conditions;
        completions.add_completion_entries_from_paths_or_exports_or_imports(
            result,
            is_exports,
            is_imports,
            fragment,
            base_directory,
            self.extension_options,
            keys,
            &|key| {
                let Some(value) = tsc_program::package_json_entries_in_order(table)
                    .find(|(candidate, _)| candidate.to_string_lossy() == key)
                    .map(|(_, value)| value)
                else {
                    return Vec::new();
                };
                let pattern = pattern_from_first_matching_condition(value, conditions);
                if pattern.is_empty() {
                    return Vec::new();
                }
                if key.ends_with('/') && pattern.ends_with('/') {
                    return vec![format!("{pattern}*")];
                }
                vec![pattern]
            },
            &|a, b| tsc_program::compare_pattern_keys(JsStr::from_str(a), JsStr::from_str(b)),
        );
        true
    }
}

/// tsgo `getPatternFromFirstMatchingCondition`: a string target, or the
/// target of the first condition that applies.
fn pattern_from_first_matching_condition(target: &JsonValue, conditions: &[String]) -> String {
    match target {
        JsonValue::String(pattern) => pattern.to_string_lossy().into_owned(),
        JsonValue::Object(object) => {
            for (condition, value) in tsc_program::package_json_entries_in_order(object) {
                let condition = condition.to_string_lossy();
                if condition == "default"
                    || conditions.iter().any(|known| *known == condition)
                    || conditions.iter().any(|known| known == "types")
                        && is_applicable_versioned_types_key(&condition)
                {
                    return pattern_from_first_matching_condition(value, conditions);
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

/// tsgo `module.IsApplicableVersionedTypesKey`: `types@<range>` with a
/// range the compiler version satisfies.
fn is_applicable_versioned_types_key(key: &str) -> bool {
    key.strip_prefix("types@")
        .and_then(tsc_types::compiler_version_satisfies)
        == Some(true)
}

/// tsgo `GetVersionPaths(nil).GetPaths()`: the selected `typesVersions`
/// paths, each key's string targets (another value is empty).
fn version_paths(package: &JsonObject) -> Option<Vec<(String, Vec<String>)>> {
    let Some(JsonValue::Object(types_versions)) =
        tsc_program::package_json_own_property(package, "typesVersions")
    else {
        return None;
    };
    let (_, paths) = tsc_program::types_versions_paths(types_versions)?;
    Some(
        tsc_program::package_json_entries_in_order(paths)
            .filter_map(|(key, value)| match value {
                JsonValue::Array(targets) => Some((
                    key.to_string_lossy().into_owned(),
                    targets
                        .iter()
                        .map(|target| match target {
                            JsonValue::String(target) => target.to_string_lossy().into_owned(),
                            _ => String::new(),
                        })
                        .collect(),
                )),
                _ => None,
            })
            .collect(),
    )
}

/// tsgo `toPathCompletions`.
fn to_path_completions(names: Vec<ModuleCompletion>) -> Vec<PathCompletion> {
    names
        .into_iter()
        .map(|completion| PathCompletion {
            name: completion.name,
            kind: match completion.kind {
                ModuleCompletionKind::Directory => ScriptElementKind::Directory,
                ModuleCompletionKind::File => ScriptElementKind::ScriptElement,
                ModuleCompletionKind::ExternalModuleName => ScriptElementKind::ExternalModuleName,
            },
            extension: completion.extension,
        })
        .collect()
}

/// tsgo `kindModifiersFromExtension`.
fn kind_modifiers_from_extension(extension: &str) -> ScriptElementKindModifier {
    match extension {
        ".d.ts" => ScriptElementKindModifier::DTS,
        ".js" => ScriptElementKindModifier::JS,
        ".json" => ScriptElementKindModifier::JSON,
        ".jsx" => ScriptElementKindModifier::JSX,
        ".ts" => ScriptElementKindModifier::TS,
        ".tsx" => ScriptElementKindModifier::TSX,
        ".d.mts" => ScriptElementKindModifier::DMTS,
        ".mjs" => ScriptElementKindModifier::MJS,
        ".mts" => ScriptElementKindModifier::MTS,
        ".d.cts" => ScriptElementKindModifier::DCTS,
        ".cjs" => ScriptElementKindModifier::CJS,
        ".cts" => ScriptElementKindModifier::CTS,
        _ => ScriptElementKindModifier::NONE,
    }
}

/// tsgo `getDirectoryFragmentRange`: the text after the last directory
/// separator, none when it is empty.
fn directory_fragment_range(text: &str, text_start: usize) -> Option<std::ops::Range<usize>> {
    let offset = text.rfind(['/', '\\']).map_or(0, |index| index + 1);
    let length = text.len() - offset;
    (length != 0).then(|| text_start + offset..text_start + offset + length)
}

/// tsgo `parseTripleSlashDirectiveFragment`: the text up to and with the
/// opening quote, `path` or `types`, and the unterminated fragment after
/// the quote.
fn parse_triple_slash_directive_fragment(text: &str) -> Option<(&str, &str, &str)> {
    let is_white_space_like = tsc_syntax::is_whitespace_like;
    let rest = text
        .strip_prefix("///")?
        .trim_start_matches(is_white_space_like);
    let rest = rest.strip_prefix("<reference")?;
    if !rest.starts_with(is_white_space_like) {
        return None;
    }
    let rest = rest.trim_start_matches(is_white_space_like);
    let (kind, rest) = if let Some(rest) = rest.strip_prefix("path") {
        ("path", rest)
    } else {
        ("types", rest.strip_prefix("types")?)
    };
    let rest = rest
        .trim_start_matches(is_white_space_like)
        .strip_prefix('=')?
        .trim_start_matches(is_white_space_like);
    let rest = rest.strip_prefix('\'').or_else(|| rest.strip_prefix('"'))?;
    if rest.contains(['\'', '"']) {
        return None;
    }
    Some((&text[..text.len() - rest.len()], kind, rest))
}

/// tsgo `core.ResolutionMode` as the resolver keys it.
fn resolution_mode(mode: u32) -> ResolutionMode {
    match mode {
        1 => ResolutionMode::CommonJs,
        99 => ResolutionMode::EsNext,
        _ => ResolutionMode::Unspecified,
    }
}

/// tsgo `core.ResolutionMode` as the module specifier code takes it.
fn emit_resolution_mode(mode: u32) -> EmitResolutionMode {
    match mode {
        1 => EmitResolutionMode::CommonJs,
        99 => EmitResolutionMode::EsNext,
        _ => EmitResolutionMode::None,
    }
}

/// tsgo `moduleResolutionUsesNodeModules`: node16, nodenext or bundler.
fn module_resolution_uses_node_modules(kind: i32) -> bool {
    (3..=99).contains(&kind) || kind == 100
}

/// tsgo `isPathRelativeToScript`.
fn is_path_relative_to_script(path: &str) -> bool {
    path.starts_with("./") || path.starts_with("../")
}

/// tsgo `getFragmentDirectory`.
fn fragment_directory(fragment: &str) -> String {
    if !fragment.contains('/') {
        String::new()
    } else if has_trailing_directory_separator(fragment) {
        fragment.to_owned()
    } else {
        get_directory_path(fragment)
    }
}

/// tsgo `tryRemoveDirectoryPrefix`.
fn try_remove_directory_prefix(path: &str, prefix: &str, case_sensitive: bool) -> Option<String> {
    let without_prefix = trim_prefix(path, prefix, case_sensitive)?;
    Some(
        without_prefix
            .strip_prefix(['/', '\\'])
            .unwrap_or(without_prefix)
            .to_owned(),
    )
}

/// tsgo `CaseSensitivity.TrimPrefix`.
fn trim_prefix<'t>(text: &'t str, prefix: &str, case_sensitive: bool) -> Option<&'t str> {
    if case_sensitive {
        return text.strip_prefix(prefix);
    }
    let lower = |text: &str| {
        tsc_program::to_file_name_lower_case_js(JsStr::from_str(text))
            .to_string_lossy()
            .into_owned()
    };
    let canonical_prefix = lower(prefix);
    if !lower(text).starts_with(&canonical_prefix) {
        return None;
    }
    let count = canonical_prefix.chars().count();
    Some(
        &text[text
            .char_indices()
            .nth(count)
            .map_or(text.len(), |(index, _)| index)..],
    )
}

/// tsgo `CaseSensitivity.trimContainedPath`: `child` relative to `parent`
/// when it is in it.
pub(crate) fn trim_contained_path(
    parent: &str,
    child: &str,
    case_sensitive: bool,
) -> Option<String> {
    if parent.is_empty() || child.is_empty() {
        return None;
    }
    let parent_root_length = root_length(parent);
    let child_root_length = root_length(child);
    if !parent[..parent_root_length].eq_ignore_ascii_case(&child[..child_root_length]) {
        return None;
    }
    let relative = trim_prefix(
        &child[child_root_length..],
        &parent[parent_root_length..],
        case_sensitive,
    )?;
    if !relative.is_empty()
        && !has_trailing_directory_separator(parent)
        && parent.len() != parent_root_length
        && !relative.starts_with('/')
    {
        return None;
    }
    Some(relative.strip_prefix('/').unwrap_or(relative).to_owned())
}

/// tsgo `module.UnmangleScopedPackageName`.
fn unmangle_scoped_package_name(package_name: &str) -> String {
    match package_name.split_once("__") {
        Some((scope, name)) => format!("@{scope}/{name}"),
        None => package_name.to_owned(),
    }
}

/// tsgo `stringutil.StripQuotes`.
fn strip_quotes(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 2
        && bytes[0] == bytes[bytes.len() - 1]
        && matches!(bytes[0], b'"' | b'\'' | b'`')
    {
        return &name[1..name.len() - 1];
    }
    name
}

/// tsgo `getFileExtension`: a known extension, else any.
fn file_extension(file_name: &str) -> String {
    let extension = try_get_extension_from_path(file_name);
    if !extension.is_empty() {
        return extension.to_owned();
    }
    let base = get_base_file_name(file_name);
    base.rfind('.')
        .map(|index| base[index..].to_owned())
        .unwrap_or_default()
}

/// tsgo `extensionsToRemove`.
const EXTENSIONS_TO_REMOVE: [&str; 12] = [
    ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
    ".json",
];

/// tsgo `FileExtensionIs`.
fn file_extension_is(path: &str, extension: &str) -> bool {
    path.len() > extension.len() && path.ends_with(extension)
}

/// tsgo `FileExtensionIsOneOf`.
fn file_extension_is_one_of(path: &str, extensions: &[&str]) -> bool {
    extensions
        .iter()
        .any(|extension| file_extension_is(path, extension))
}

/// tsgo `TryGetExtensionFromPath`.
fn try_get_extension_from_path(path: &str) -> &'static str {
    EXTENSIONS_TO_REMOVE
        .iter()
        .find(|extension| file_extension_is(path, extension))
        .copied()
        .unwrap_or_default()
}

/// tsgo `RemoveFileExtension`.
fn remove_file_extension(path: &str) -> &str {
    EXTENSIONS_TO_REMOVE
        .iter()
        .find_map(|extension| path.strip_suffix(extension))
        .unwrap_or(path)
}

/// tsgo `ChangeExtension`: a known extension replaced.
fn change_extension(path: &str, extension: &str) -> String {
    for known in EXTENSIONS_TO_REMOVE {
        if path.len() >= known.len()
            && path.ends_with(known)
            && path.as_bytes()[path.len() - known.len()] == b'.'
        {
            let extension = if extension.starts_with('.') {
                extension.to_owned()
            } else {
                format!(".{extension}")
            };
            return format!("{}{extension}", &path[..path.len() - known.len()]);
        }
    }
    path.to_owned()
}

/// tsgo `GetDeclarationEmitExtensionForPath`.
fn declaration_emit_extension_for_path(path: &str) -> String {
    if file_extension_is_one_of(path, &[".mjs", ".mts"]) {
        ".d.mts".to_owned()
    } else if file_extension_is_one_of(path, &[".cjs", ".cts"]) {
        ".d.cts".to_owned()
    } else if file_extension_is_one_of(path, &[".ts", ".tsx", ".js", ".jsx"]) {
        ".d.ts".to_owned()
    } else {
        let base = get_base_file_name(path);
        match base.rfind('.') {
            Some(index) => format!(".d{}.ts", &base[index..]),
            None => ".d.ts".to_owned(),
        }
    }
}

/// tsgo `GetPossibleOriginalInputExtensionForExtension`.
pub(crate) fn possible_original_input_extension_for_extension(path: &str) -> Vec<String> {
    if file_extension_is_one_of(path, &[".d.mts", ".mjs", ".mts"]) {
        return vec![".mts".to_owned(), ".mjs".to_owned()];
    }
    if file_extension_is_one_of(path, &[".d.cts", ".cjs", ".cts"]) {
        return vec![".cts".to_owned(), ".cjs".to_owned()];
    }
    // A custom `.d.x.ts` is `.x`.
    let declaration = tsc_program::declaration_file_extension(path);
    if !declaration.is_empty() && declaration != ".d.ts" {
        let inner = &declaration[".d.".len()..declaration.len() - ".ts".len()];
        return vec![format!(".{inner}")];
    }
    [".tsx", ".ts", ".jsx", ".js"]
        .iter()
        .map(|&extension| extension.to_owned())
        .collect()
}

/// tsgo `GetEncodedRootLength`: negative for a URL.
fn encoded_root_length(path: &str) -> isize {
    let bytes = path.as_bytes();
    let Some(&first) = bytes.first() else {
        return 0;
    };
    if first == b'/' || first == b'\\' {
        if bytes.len() == 1 || bytes[1] != first {
            return 1;
        }
        return match path[2..].find(first as char) {
            Some(index) => (index + 3) as isize,
            None => bytes.len() as isize,
        };
    }
    if first.is_ascii_alphabetic() && bytes.get(1) == Some(&b':') {
        if bytes.len() == 2 {
            return 2;
        }
        if matches!(bytes[2], b'/' | b'\\') {
            return 3;
        }
    }
    if let Some(scheme_end) = path.find("://") {
        let authority_start = scheme_end + 3;
        if let Some(authority_end) = path[authority_start..].find('/') {
            let authority_end = authority_start + authority_end;
            let scheme = &path[..scheme_end];
            let authority = &path[authority_start..authority_end];
            if scheme == "file"
                && (authority.is_empty() || authority == "localhost")
                && path
                    .as_bytes()
                    .get(authority_end + 1)
                    .is_some_and(u8::is_ascii_alphabetic)
            {
                let volume_end = authority_end + 2;
                let rest = &path[volume_end.min(path.len())..];
                if let Some(after) = rest
                    .strip_prefix(':')
                    .or_else(|| rest.strip_prefix("%3a"))
                    .or_else(|| rest.strip_prefix("%3A"))
                {
                    if after.is_empty() || after.starts_with('/') {
                        let end = path.len() - after.len();
                        return !(end as isize) - if after.starts_with('/') { 1 } else { 0 };
                    }
                }
            }
            return !((authority_end + 1) as isize);
        }
        return !(path.len() as isize);
    }
    0
}

/// tsgo `GetRootLength`.
fn root_length(path: &str) -> usize {
    let length = encoded_root_length(path);
    (if length < 0 { !length } else { length }) as usize
}

/// tsgo `IsRootedDiskPath`.
fn is_rooted_disk_path(path: &str) -> bool {
    encoded_root_length(path) > 0
}

/// tsgo `IsUrl`.
fn is_url(path: &str) -> bool {
    encoded_root_length(path) < 0
}

/// tsgo `NormalizeSlashes`.
fn normalize_slashes(path: &str) -> String {
    tsc_program::normalize_slashes(JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `HasTrailingDirectorySeparator`.
fn has_trailing_directory_separator(path: &str) -> bool {
    path.ends_with(['/', '\\'])
}

/// tsgo `EnsureTrailingDirectorySeparator`.
fn ensure_trailing_directory_separator(path: &str) -> String {
    if has_trailing_directory_separator(path) {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

/// tsgo `RemoveTrailingDirectorySeparator`.
fn remove_trailing_directory_separator(path: &str) -> String {
    if has_trailing_directory_separator(path) && root_length(path) < path.len() {
        path[..path.len() - 1].to_owned()
    } else {
        path.to_owned()
    }
}

/// tsgo `GetDirectoryPath`.
fn get_directory_path(path: &str) -> String {
    tsc_program::get_directory_path(JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `GetBaseFileName`.
fn get_base_file_name(path: &str) -> String {
    tsc_program::base_file_name(JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `CombinePaths`.
fn combine_paths(parent: &str, child: &str) -> String {
    tsc_program::combine_paths(JsStr::from_str(parent), JsStr::from_str(child))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `NormalizePath`.
fn normalize_path(path: &str) -> String {
    tsc_program::normalize_path(JsStr::from_str(path))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `ResolvePath` of one path.
fn resolve_path(path: &str) -> String {
    normalize_path(&normalize_slashes(path))
}

/// tsgo `RootedDirectoryPath.ResolveDirectory`: `path` from `directory`,
/// normalized, with no trailing separator.
fn resolve_directory(directory: &str, path: &str) -> String {
    if path.is_empty() {
        return directory.to_owned();
    }
    let resolved = tsc_program::get_normalized_absolute_path(
        JsStr::from_str(path),
        JsStr::from_str(directory),
    )
    .to_string_lossy()
    .into_owned();
    remove_trailing_directory_separator(&resolved)
}

/// tsgo `GetPathComponents`: the root, then each component.
fn get_path_components(path: &str) -> Vec<String> {
    let path = normalize_slashes(path);
    let root = root_length(&path);
    let mut components = vec![path[..root].to_owned()];
    let rest = &path[root..];
    let mut parts = rest.split('/').map(str::to_owned).collect::<Vec<_>>();
    if parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    components.extend(parts);
    components
}
